use crate::ai;
use crate::auth;
use crate::collab;use crate::config::{self, Config};
use crate::formats;
use crate::notes;
use crate::scanner::{self, FileInfo, FolderInfo};
use crate::thumbnail;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;
use tower::ServiceExt;

/// État partagé par les handlers HTTP et WebSocket.
///
/// `root` et `config` sont **mutables à chaud** : le watcher peut les mettre à
/// jour lorsqu'on modifie `config.yml` (dossier des modèles, mode d'affichage),
/// sans redémarrer le serveur ni recharger le frontend.
#[derive(Clone)]
pub struct AppState {
    /// Racine courante des modèles (peut changer via la config).
    pub root: Arc<RwLock<PathBuf>>,
    /// Configuration courante (rechargée à chaud).
    pub config: Arc<RwLock<Config>>,
    /// Fichier YAML dont vient la config, réécrit par `PUT /config`.
    pub config_path: Arc<PathBuf>,
    /// Canal broadcast : diffuse la liste des modèles (JSON) à tous les WS.
    pub ws: broadcast::Sender<String>,
    /// Documents collaboratifs ouverts (édition temps réel des notes).
    pub collab: Arc<collab::Rooms>,
    /// Adresses connues des fournisseurs d'IA (`ai-presets.yml`).
    ///
    /// Rechargées à chaud comme la configuration : c'est un fichier que l'on
    /// édite pendant que le serveur tourne (voir [`crate::ai::presets`]).
    pub presets: Arc<RwLock<ai::presets::Presets>>,
    /// Dernière liste diffusée aux clients WS (JSON).
    ///
    /// Référence du **re-scan périodique** ([`refresh_if_changed`]) : sans elle,
    /// chaque tic rediffuserait un contenu identique et le frontend, qui
    /// remplace son état à chaque message, se re-rendrait pour rien.
    pub last_snapshot: Arc<RwLock<String>>,
    /// Comptes utilisateurs et sessions.
    ///
    /// **Éteint par défaut** ([`auth::Auth::disabled`]) : sans `EASY3D_AUTH`, le
    /// middleware laisse passer tout le monde et le serveur se comporte comme
    /// avant l'existence des comptes.
    pub auth: Arc<auth::Auth>,
    /// Réglages SSO venus de l'**environnement** (`EASY3D_OIDC_*`).
    ///
    /// Ils gagnent sur `config.yml` : voir [`auth::oidc::Env`]. Vide par défaut
    /// — comme [`AppState::auth`], c'est un champ de l'état (rempli par `main`)
    /// et non une lecture d'environnement à chaque requête, ce qui rend le flux
    /// testable.
    pub oidc: Arc<auth::oidc::Env>,
    /// Un service MCP **par compte** (voir [`AppState::mcp_service`]).
    pub mcp: Arc<Mutex<HashMap<String, StreamableHttpService<crate::mcp::Easy3dMcp, LocalSessionManager>>>>,
}

impl AppState {
    /// Construit un état partagé à partir des valeurs initiales.
    pub fn new(root: impl Into<PathBuf>, ws: broadcast::Sender<String>, config: Config) -> Self {
        Self {
            root: Arc::new(RwLock::new(root.into())),
            config: Arc::new(RwLock::new(config)),
            config_path: Arc::new(Config::config_path()),
            ws,
            collab: Arc::new(collab::Rooms::new()),
            // Adresses livrées par défaut : le fichier du dossier de
            // configuration les remplace s'il existe (voir `main.rs`).
            presets: Arc::new(RwLock::new(ai::presets::defaults())),
            last_snapshot: Arc::new(RwLock::new(String::new())),
            auth: Arc::new(auth::Auth::disabled()),
            oidc: Arc::new(auth::oidc::Env::empty()),
            mcp: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Service MCP d'un compte, créé à sa première utilisation.
    ///
    /// La fabrique de rmcp ne reçoit **pas** la requête HTTP (elle ne prend aucun
    /// argument) : l'identité doit donc être capturée au moment où le service est
    /// créé. Un service par compte est de toute façon le bon découpage — chaque
    /// agent garde ses propres sessions, et l'un ne peut pas hériter de celles de
    /// l'autre.
    ///
    /// Le service est **conservé** : ses sessions doivent survivre d'une requête
    /// à l'autre, c'est ainsi que fonctionne le transport Streamable HTTP.
    pub fn mcp_service(
        &self,
        user: Option<String>,
    ) -> StreamableHttpService<crate::mcp::Easy3dMcp, LocalSessionManager> {
        // Clé vide = installation sans comptes : un seul service, comme avant.
        let cle = user.clone().unwrap_or_default();
        if let Some(service) = self.mcp.lock().unwrap().get(&cle) {
            return service.clone();
        }

        let state = self.clone();
        let service = StreamableHttpService::new(
            move || Ok(crate::mcp::Easy3dMcp::for_user(state.clone(), user.clone())),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        self.mcp.lock().unwrap().insert(cle, service.clone());
        service
    }

    /// Active la gestion des comptes (voir `auth::Auth::from_env`).
    ///
    /// Passer par un champ de l'état — et non par une variable d'environnement
    /// relue à chaque requête — est ce qui rend l'authentification **testable**
    /// sans jouer avec l'environnement du processus (même choix que les
    /// permissions du serveur MCP).
    pub fn with_auth(mut self, auth: auth::Auth) -> Self {
        self.auth = Arc::new(auth);
        self
    }

    /// Fixe les réglages SSO venus de l'environnement (voir [`auth::oidc::Env`]).
    pub fn with_oidc(mut self, oidc: auth::oidc::Env) -> Self {
        self.oidc = Arc::new(oidc);
        self
    }

    /// Remplace les adresses connues (démarrage, ou relecture à chaud).
    pub fn with_presets(mut self, presets: ai::presets::Presets) -> Self {
        self.presets = Arc::new(RwLock::new(presets));
        self
    }

    /// Copie des adresses connues, telles que servies à l'interface.
    pub fn presets(&self) -> ai::presets::Presets {
        self.presets.read().unwrap().clone()
    }

    /// Redirige la réécriture de config vers un autre fichier.
    ///
    /// Les tests s'en servent pour écrire dans un dossier temporaire plutôt que
    /// dans le `config.yml` du dépôt.
    pub fn with_config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Arc::new(path.into());
        self
    }

    /// Copie la racine courante des modèles.
    pub fn root(&self) -> PathBuf {
        self.root.read().unwrap().clone()
    }

    /// Copie la configuration courante.
    pub fn config(&self) -> Config {
        self.config.read().unwrap().clone()
    }
}

/// Démarre le serveur HTTP et bloque jusqu'à son arrêt.
pub async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let app = routes(state);

    // Adresse d'écoute. Le défaut est la boucle locale : l'API n'a **aucune
    // authentification**, elle ne doit pas être joignable depuis le réseau.
    //
    // Un conteneur fait exception : la publication de port (`-p 8090:8090`) ne
    // relaie pas vers la boucle locale du conteneur, il faut donc écouter sur
    // toutes ses interfaces (`EASY3D_HOST=0.0.0.0`) — le port publié, lui, reste
    // une décision explicite de celui qui lance le conteneur.
    //
    // Nom préfixé pour ne pas entrer en collision avec `HOST`, que lit aussi le
    // serveur Nitro du frontend (même conteneur en image unique).
    let host = std::env::var("EASY3D_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8090".to_string());
    let addr = format!("{host}:{port}").parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("API HTTP : http://{addr}/models");
    println!(
        "MCP      : http://{addr}/mcp (outils destructeurs : {})",
        if crate::mcp::destructive_allowed_from_env() {
            "activés"
        } else {
            "désactivés — EASY3D_MCP_ALLOW_WRITE=1 pour les activer"
        }
    );
    axum::serve(listener, app).await?;
    Ok(())
}

/// Table des routes du serveur.
///
/// Extraite de [`serve`] pour que les tests montent exactement le même
/// serveur que la production.
pub fn routes(state: AppState) -> Router {
    // Routes **publiques** : la santé, et de quoi se connecter. Sans ces
    // dernières, un serveur fermé le resterait pour tout le monde.
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth/me", get(auth::me))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        // SSO : le navigateur part d'ici et revient sur `…/callback`. Les deux
        // doivent être publiques par nature — on n'est pas encore authentifié.
        .route("/auth/oidc/start", get(auth::oidc::start))
        .route("/auth/oidc/callback", get(auth::oidc::callback));

    // Les routes protégées sont groupées **par droit exigé** : un groupe, un
    // droit, une ligne ([`gated`]). La table des routes se lit donc comme la
    // liste des droits (`auth::permissions`) : un droit ajouté se voit ici tout
    // de suite, et un droit oublié aussi.
    //
    // Le serveur MCP reste derrière `require_auth` seul : ses outils ont des
    // droits différents et seront vérifiés **outil par outil** (étape des jetons
    // d'API), ce qu'un chemin unique ne permet pas de faire.
    let catalogue = gated(
        Router::new()
            .route("/models", get(list_models))
            .route("/file", get(get_file))
            .route("/note", get(get_note))
            .route("/ws", get(ws_models)),
        &state,
        auth::permissions::CATALOG_READ,
    );

    // Les notes sont dans le catalogue **en aperçu** (le JSON diffusé porte le
    // Markdown rendu) : écrire dans l'éditeur temps réel demande donc le droit
    // d'écriture, pas celui de lecture. Le canal CRDT ne sait pas distinguer une
    // lecture d'une écriture — c'est le prix, assumé, de l'édition collaborative.
    let notes = gated(
        Router::new().route("/collab/{*rel}", get(ws_collab)),
        &state,
        auth::permissions::NOTE_WRITE,
    );

    let uploads = gated(
        Router::new().route(
            "/upload",
            post(post_upload).layer(DefaultBodyLimit::max(UPLOAD_MAX_BYTES)),
        ),
        &state,
        auth::permissions::MODEL_UPLOAD,
    );
    let renames = gated(
        Router::new().route("/rename", post(post_rename)),
        &state,
        auth::permissions::MODEL_RENAME,
    );
    let deletes = gated(
        Router::new().route("/delete", post(post_delete)),
        &state,
        auth::permissions::MODEL_DELETE,
    );

    // `/config` apparaît deux fois : lire les réglages et les modifier sont deux
    // droits distincts, et axum fusionne sans problème les méthodes d'un même
    // chemin.
    let settings_read = gated(
        Router::new().route("/config", get(get_config)),
        &state,
        auth::permissions::CONFIG_READ,
    );
    let settings_write = gated(
        Router::new()
            .route("/config", put(put_config))
            // Contrôle du fournisseur d'identité : il fait partir une requête
            // vers l'adresse enregistrée, donc il appartient à qui peut écrire
            // la configuration.
            .route("/auth/oidc/check", get(auth::oidc::check)),
        &state,
        auth::permissions::CONFIG_WRITE,
    );

    // Les adresses connues des fournisseurs accompagnent l'écran de recherche :
    // il faut pouvoir chercher pour en avoir besoin.
    let ai = gated(
        Router::new()
            .route("/ai/providers", get(ai_providers))
            .route("/ai/search", post(ai_search)),
        &state,
        auth::permissions::AI_USE,
    );
    let ai_config = gated(
        Router::new().route("/ai/models", post(ai_models)),
        &state,
        auth::permissions::AI_CONFIG,
    );

    // Ce qui ne demande qu'être **connecté** : son propre mot de passe, et le
    // catalogue des droits (l'interface en a besoin pour construire ses cases à
    // cocher ; la liste est publique dans le binaire).
    let authenticated = Router::new()
        .route("/auth/ws-ticket", post(auth::ws_ticket))
        .route("/auth/password", post(auth::rbac::change_password))
        .route("/permissions", get(auth::rbac::list_permissions))
        // Les jetons d'API sont **personnels** : chacun gère les siens, il n'y a
        // donc aucun droit à exiger (ceux des autres ne sont jamais montrés).
        .route(
            "/tokens",
            get(auth::tokens::list).post(auth::tokens::create),
        )
        .route("/tokens/{uuid}", axum::routing::delete(auth::tokens::revoke));

    let accounts_read = gated(
        Router::new()
            .route("/users", get(auth::rbac::list_users))
            // Le journal d'audit se lit avec les comptes : c'est le même sujet,
            // et il ne doit pas être lisible par un simple lecteur du catalogue.
            .route("/journal", get(auth::journal::list)),
        &state,
        auth::permissions::USERS_READ,
    );
    let roles_read = gated(
        Router::new().route("/roles", get(auth::rbac::list_roles)),
        &state,
        auth::permissions::ROLES_READ,
    );
    let accounts_write = gated(
        Router::new()
            .route("/users", post(auth::rbac::create_user))
            .route(
                "/users/{uuid}",
                put(auth::rbac::update_user).delete(auth::rbac::delete_user),
            ),
        &state,
        auth::permissions::USERS_WRITE,
    );
    let roles_write = gated(
        Router::new()
            .route("/roles", post(auth::rbac::create_role))
            .route("/roles/default", put(auth::rbac::set_default_role))
            .route(
                "/roles/{uuid}",
                put(auth::rbac::update_role).delete(auth::rbac::delete_role),
            ),
        &state,
        auth::permissions::ROLES_WRITE,
    );

    let protected = Router::new()
        .merge(catalogue)
        .merge(notes)
        .merge(uploads)
        .merge(renames)
        .merge(deletes)
        .merge(settings_read)
        .merge(settings_write)
        .merge(ai)
        .merge(ai_config)
        .merge(authenticated)
        .merge(accounts_read)
        .merge(roles_read)
        .merge(accounts_write)
        .merge(roles_write)
        // Le serveur MCP n'est pas monté par `nest_service` : il faut choisir le
        // service **du compte** à chaque requête (voir `AppState::mcp_service`).
        .route("/mcp", any(mcp))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    public.merge(protected).with_state(state)
}

/// Point d'entrée du serveur MCP (`/mcp`).
///
/// L'identité est celle que `require_auth` a résolue — cookie de session ou
/// **jeton d'API** — et chaque compte a son propre service MCP : les outils
/// savent donc à qui ils parlent, et chacun vérifie le droit qu'il exige
/// (`permissions::for_tool`).
async fn mcp(
    State(state): State<AppState>,
    auth: Option<auth::AuthUser>,
    request: Request,
) -> Response {
    let user = auth.map(|auth::AuthUser(user)| user.uuid);
    let service = state.mcp_service(user);

    match service.oneshot(request).await {
        Ok(response) => response.map(axum::body::Body::new).into_response(),
        // `Infallible` : le service ne peut pas échouer autrement qu'en répondant.
        Err(never) => match never {},
    }
}

/// Ajoute à un groupe de routes le contrôle du droit exigé.
///
/// Cette petite fonction existe parce que la couche renvoyée par
/// `middleware::from_fn_with_state` n'a pas de type nommable : on la fabrique
/// ici, une fois, au lieu de répéter le tuple dans chaque groupe.
fn gated(routes: Router<AppState>, state: &AppState, permission: &'static str) -> Router<AppState> {
    routes.route_layer(middleware::from_fn_with_state(
        (state.clone(), permission),
        auth::require_permission,
    ))
}

/// Point de santé : renvoie `ok` (200).
async fn health() -> &'static str {
    "ok"
}

/// Fournisseurs de modèles connus, pour l'écran d'administration.
///
/// Les identifiants, libellés et valeurs par défaut viennent du backend : le
/// frontend n'a aucune liste de fournisseurs codée en dur. Les **adresses**
/// connues viennent du fichier `ai-presets.yml` ([`ai::presets`]) : elles sont
/// donc relues à chaque appel de la route, et une modification du fichier se
/// voit sans redémarrer.
async fn ai_providers(State(state): State<AppState>) -> Json<Vec<ai::ProviderInfo>> {
    Json(ai::describe(&state.presets()))
}

/// Demande de recherche assistée.
#[derive(Deserialize)]
pub struct AiSearchRequest {
    /// Description en langage naturel de ce que l'utilisateur cherche.
    pub query: String,
}

/// Recherche assistée : le modèle interroge le catalogue et propose des
/// éléments.
///
/// C'est un appel long (plusieurs allers-retours vers le fournisseur) : le
/// frontend affiche un état d'attente. Les erreurs remontent telles quelles —
/// elles sont destinées à être lues par l'utilisateur (« clé API manquante »,
/// « appel de … impossible »).
async fn ai_search(
    State(state): State<AppState>,
    Json(request): Json<AiSearchRequest>,
) -> Result<Json<ai::Outcome>, (StatusCode, String)> {
    ai::search(&state, &request.query)
        .await
        .map(Json)
        .map_err(ai_error)
}

/// Réponse de `/ai/models`.
#[derive(Serialize)]
pub struct AiModelsResponse {
    /// Identifiants proposés par le fournisseur, dans son ordre.
    pub models: Vec<String>,
}

/// Demande de liste des modèles : les valeurs du formulaire d'administration.
///
/// Elles ne sont **pas** enregistrées : on interroge le fournisseur avant de
/// valider, ce qui évite d'écrire une clé ou une adresse encore approximative.
#[derive(Deserialize)]
pub struct AiModelsRequest {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    /// Clé telle que l'interface la connaît : `***` (ou absente) signifie
    /// « garde celle qui est enregistrée ».
    pub api_key: Option<String>,
}

/// Liste les modèles accessibles avec la clé fournie.
///
/// C'est le bouton « Tester » de l'administration : il remplit la liste de choix
/// du modèle, et vaut vérification de la clé (une clé refusée remonte le message
/// du fournisseur).
async fn ai_models(
    State(state): State<AppState>,
    Json(request): Json<AiModelsRequest>,
) -> Result<Json<AiModelsResponse>, (StatusCode, String)> {
    // La clé enregistrée n'est jamais renvoyée à l'interface : `***` veut dire
    // « celle que tu as », exactement comme pour `PUT /config`.
    let stored = state.config().ai;
    let ai = config::Ai {
        provider: request.provider,
        base_url: request.base_url,
        model: None,
        api_key: stored.merge_key(request.api_key.as_deref()),
        models: Vec::new(),
    };

    ai::list_models(&ai)
        .await
        .map(|models| Json(AiModelsResponse { models }))
        .map_err(ai_error)
}

/// Erreur d'un appel d'IA : configuration incomplète ou fournisseur muet.
///
/// Le corps est le message lui-même (comme les autres routes) : l'interface
/// n'affiche rien d'autre, et une erreur de fournisseur se corrige dans
/// l'administration.
fn ai_error(message: String) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message)
}

/// Réponse de `/models` : fichiers racine, sous-dossiers groupés, total.
///
/// C'est aussi le payload diffusé sur le WebSocket `/ws`.
#[derive(Serialize)]
pub struct ModelsResponse {
    pub folders: BTreeMap<String, FolderInfo>,
    pub files: Vec<FileInfo>,
    pub count: usize,
    /// Formats reconnus (extensions, aperçu, visionneuse) : le frontend n'a
    /// ainsi aucune extension codée en dur.
    pub formats: Vec<formats::FormatInfo>,
    /// Adresses connues des fournisseurs d'IA, par identifiant de fournisseur.
    ///
    /// Diffusées avec le reste (et non seulement par `/ai/providers`) : le
    /// fichier `ai-presets.yml` peut être modifié pendant que la page
    /// d'administration est ouverte, et les puces suivent.
    pub presets: ai::presets::Presets,
    /// Configuration applicative, exposée au frontend.
    pub config: Config,
}

impl ModelsResponse {
    /// Construit une réponse à partir d'un scan.
    pub fn from_scan(
        scan: scanner::ModelsScan,
        config: &Config,
        presets: &ai::presets::Presets,
    ) -> Self {
        let count = scan.total();
        ModelsResponse {
            folders: scan.folders,
            files: scan.files,
            count,
            formats: formats::describe(),
            presets: presets.clone(),
            // La clé d'API ne sort jamais du backend, même par le WebSocket.
            config: config.redacted(),
        }
    }
}

/// Renvoie le contenu structuré : fichiers racine + dossiers + total.
async fn list_models(State(state): State<AppState>) -> Json<ModelsResponse> {
    Json(ModelsResponse::from_scan(
        scanner::scan_models(&state.root()),
        &state.config(),
        &state.presets(),
    ))
}

/// Réponse de `/config` : la configuration, plus le dossier réellement surveillé.
///
/// Le chemin résolu est ce qui intéresse l'interface : `models_root` peut être
/// vide (valeur par défaut) ou relatif à `backend/`.
#[derive(Serialize)]
pub struct ConfigResponse {
    pub config: Config,
    pub models_root: String,
    /// Surveillance du dossier : de quoi expliquer, dans l'interface, *pourquoi*
    /// la valeur recommandée est celle-là.
    pub watch: WatchInfo,
    /// État du SSO : ce que `config.yml` ne peut plus régler, et ce qui manque.
    pub oidc: OidcInfo,
}

/// Ce que l'interface doit savoir du SSO.
#[derive(Serialize)]
pub struct OidcInfo {
    /// Champs du bloc `oidc` **figés par l'environnement** (`EASY3D_OIDC_*`) :
    /// l'interface les montre grisés, avec la raison.
    pub locked: Vec<&'static str>,
    /// `true` si le SSO est réellement utilisable.
    pub active: bool,
    /// Ce qui manque, quand le SSO est demandé sans être configurable :
    /// `issuer`, `client_id` ou `client_secret` (l'interface traduit).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Ce que l'interface doit savoir sur le re-scan périodique.
///
/// Les phrases sont construites côté frontend (i18n) : ici, uniquement des faits.
#[derive(Serialize)]
pub struct WatchInfo {
    /// Intervalle **recommandé** pour cette installation, en secondes.
    pub recommended: u64,
    /// Intervalle réellement appliqué par le backend (`0` = aucun re-scan).
    pub effective: u64,
    /// Type du système de fichiers portant `models/` (renseigné sous Linux),
    /// ex : `ext4`, `virtiofs` — c'est lui qui décide de la recommandation.
    pub filesystem: Option<String>,
}

impl ConfigResponse {
    /// Réponse pour l'état courant : c'est lui qui sait ce que l'environnement
    /// impose (voir [`auth::oidc::Env`]).
    fn of_state(state: &AppState) -> Self {
        Self::of(&state.config(), &state.oidc)
    }

    fn of(config: &Config, oidc_env: &auth::oidc::Env) -> Self {
        // Canonicalisé quand c'est possible : le chemin affiché dans
        // l'interface ne doit pas contenir de `..` (ex : `backend/../models`).
        let path = config.resolve_models_root();
        let models_root = std::fs::canonicalize(&path).unwrap_or(path);

        // Recommandation pour **ce** dossier : c'est le système de fichiers qui
        // décide, pas l'OS. Dans un conteneur Docker Desktop, le backend tourne
        // sous Linux alors que le dossier vient de Windows.
        let (recommended, filesystem) = config::watch_poll_recommendation(&models_root);
        let effective =
            config::watch_poll_interval(config, &models_root).map_or(0, |d| d.as_secs());

        // Le SSO peut être demandé sans être configurable : l'interface doit le
        // dire (sinon elle afficherait un interrupteur qui ne fait rien). Le
        // code — et non une phrase — pour que le message suive la langue de
        // l'interface.
        let (active, problem) = match auth::oidc::resolve(&config.oidc, oidc_env) {
            Ok(Some(_)) => (true, None),
            Ok(None) => (false, None),
            Err(manque) => (false, Some(manque.id().to_string())),
        };

        Self {
            // Configuration telle qu'elle peut être montrée : les secrets (clé
            // d'API, secret client OIDC) y sont remplacés par un marqueur (voir
            // [`config::KEY_PLACEHOLDER`]).
            config: config.redacted(),
            models_root: tidy_path(&models_root),
            watch: WatchInfo {
                recommended,
                effective,
                filesystem,
            },
            oidc: OidcInfo {
                locked: oidc_env.locked(),
                active,
                problem,
            },
        }
    }
}

/// Chemin lisible : Windows préfixe ses chemins canoniques par `\\?\`, ce qui
/// n'a d'intérêt que pour le système de fichiers.
pub fn tidy_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// Renvoie la configuration **appliquée** (celle de l'état, pas du fichier).
async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(ConfigResponse::of_state(&state))
}

/// Remplace la configuration : écrit le YAML, que le watcher recharge à chaud.
///
/// On n'applique volontairement rien ici : le watcher est le **seul** à écrire
/// dans l'état partagé (il compare le fichier rechargé à la config courante pour
/// décider de rebasculer le dossier surveillé). Double emploi = il ne verrait
/// plus de différence et ne changerait pas de dossier.
///
/// Le frontend reçoit donc la config demandée en réponse, puis la version
/// appliquée via le WebSocket (~100 ms plus tard).
async fn put_config(
    State(state): State<AppState>,
    Json(config): Json<Config>,
) -> Result<Json<ConfigResponse>, (StatusCode, String)> {
    apply_config(&state, config).map(Json)
}

/// Valide, enregistre et applique une configuration.
///
/// Utilisé par `PUT /config` **et** par les outils MCP de configuration : un seul
/// comportement, donc un seul jeu de règles (dossier existant, écriture du YAML,
/// application immédiate si la racine ne change pas).
pub fn apply_config(
    state: &AppState,
    config: Config,
) -> Result<ConfigResponse, (StatusCode, String)> {
    // La clé d'API ne revient jamais en clair depuis l'interface : le marqueur
    // signifie « garde celle que tu as », la chaîne vide « efface-la ».
    let mut config = config;
    let stored = state.config();
    config.ai.api_key = stored.ai.merge_key(config.ai.api_key.as_deref());
    // La liste des modèles est nettoyée, et une liste vide veut dire « garde la
    // précédente » — mais seulement pour le **même** fournisseur à la même
    // adresse : une liste appartient à un point d'entrée, pas à l'application.
    config.ai.models = config::Ai::normalized_models(&config.ai.models);
    if config.ai.models.is_empty()
        && config.ai.provider == stored.ai.provider
        && config.ai.base_url == stored.ai.base_url
    {
        config.ai.models = stored.ai.models.clone();
    }
    // Sans fournisseur, rien à conserver : ni clé, ni adresse, ni modèle. Le
    // bloc disparaît alors du YAML (`skip_serializing_if`), exactement comme
    // s'il n'avait jamais été configuré — sinon un modèle du fournisseur
    // précédent réapparaîtrait dans la liste au prochain paramétrage.
    if !config.ai.is_configured() {
        config.ai = config::Ai::default();
    }

    // Le secret client OIDC suit la même convention que la clé d'API : `***`
    // veut dire « garde le secret enregistré », la chaîne vide « efface-le ».
    // Sans ça, enregistrer un changement d'émetteur effacerait le secret.
    config.oidc.client_secret = stored
        .oidc
        .merge_secret(config.oidc.client_secret.as_deref());

    // Refuse une config qui pointerait vers un dossier inexistant : le watcher
    // basculerait dessus et le catalogue se viderait.
    let root = config.resolve_models_root();
    if !root.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Dossier des modèles introuvable : {}", root.display()),
        ));
    }

    // Rien à écrire si rien n'a changé : ça évite de réécrire le fichier (et de
    // perdre ses commentaires) pour rien.
    if config == state.config() {
        return Ok(ConfigResponse::of(&config, &state.oidc));
    }

    let path = state.config_path.as_ref();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(internal_error)?;
    }
    config.save(path).map_err(internal_error)?;

    // Application immédiate **si le dossier ne change pas** : sinon il faut
    // attendre le watcher, qui n'applique qu'après son délai de regroupement
    // (~50 ms) plus un rescan complet — un aller-retour inutile pour une simple
    // bascule d'affichage.
    //
    // Si le dossier change, on ne touche à rien : le watcher est le seul à
    // pouvoir rebasculer la surveillance du répertoire, et il ne le ferait pas
    // s'il trouvait déjà la nouvelle config dans l'état partagé.
    let same_root = std::fs::canonicalize(&root).ok() == std::fs::canonicalize(state.root()).ok();
    if same_root {
        *state.config.write().unwrap() = config.clone();
        broadcast_snapshot(state);
    }

    Ok(ConfigResponse::of(&config, &state.oidc))
}

/// Erreur interne (écriture du fichier, sérialisation) → 500 avec le message.
fn internal_error(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// WebSocket `/collab/{rel}` : édition collaborative de la note d'un élément.
///
/// `rel` est le chemin relatif de la note dans le catalogue (`DemaAuto`,
/// `DemaAuto/boitier.stl`), transmis tel quel comme nom de « room » par le
/// client `y-websocket`.
async fn ws_collab(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    axum::extract::Path(rel): axum::extract::Path<String>,
) -> Response {
    ws.on_upgrade(move |socket| collab::handle_socket(socket, state, rel))
}

/// WebSocket : connexion persistante qui pousse la liste des modèles à chaque
/// changement détecté par le watcher (via le canal broadcast), y compris les
/// rechargements de configuration.
async fn ws_models(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Snapshot complet de la liste des modèles, sérialisé en JSON.
///
/// Relit l'état partagé à chaque appel : la racine et la config peuvent avoir
/// changé à chaud depuis la connexion.
fn snapshot(state: &AppState) -> String {
    serde_json::to_string(&ModelsResponse::from_scan(
        scanner::scan_models(&state.root()),
        &state.config(),
        &state.presets(),
    ))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Boucle d'une connexion WS : snapshot initial, puis diffusion en continu.
///
/// Si le client est en retard (`Lagged`), on renvoie un snapshot complet plutôt
/// que des MAJ partielles perdues. Fermeture / erreur → on sort de la boucle.
async fn handle_ws(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();

    if sink.send(Message::text(snapshot(&state))).await.is_err() {
        return;
    }

    let mut rx = state.ws.subscribe();

    loop {
        tokio::select! {
            // Coté client : on ne traite que la fermeture ; pings/pongs ignorés.
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            // MAJ diffusée par le watcher sur le canal broadcast.
            res = rx.recv() => {
                let json = match res {
                    Ok(json) => json,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        snapshot(&state)
                    }
                    Err(_) => break,
                };
                if sink.send(Message::text(json)).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Query params de `/file` : chemin relatif au dossier des modèles, et version
/// attendue du fichier (facultative).
#[derive(Deserialize)]
struct FileQuery {
    path: String,
    /// Version annoncée par le scan (voir [`scanner::version_token`]), recopiée
    /// par le frontend dans l'URL de l'aperçu (`?v=`).
    #[serde(default)]
    v: Option<String>,
}

/// Sert le contenu binaire d'un fichier du répertoire modèles.
///
/// `path` est relatif à `state.root` (ex : `DemaAuto/boitier.stl`).
/// Sécurisé contre la traversée de dossier (`..`, absolu).
///
/// Politique de cache, pour que le navigateur ne retélécharge pas les aperçus à
/// chaque chargement de page :
///
/// - `?v=<version>` **à jour** → `immutable` : cette URL ne changera jamais de
///   contenu (un fichier réécrit change de version, donc d'URL) ;
/// - sinon → `no-cache` : le navigateur peut stocker, mais doit revalider ;
/// - dans les deux cas un `ETag` (la version du fichier) permet de répondre
///   `304`, sans corps.
async fn get_file(
    State(state): State<AppState>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
) -> Response {
    let root = state.root();
    let Some(relative) = safe_join(&root, &q.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let Ok(meta) = tokio::fs::metadata(&relative).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !meta.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(version) = scanner::version_token(&meta) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let cache = if q.v.as_deref() == Some(version.as_str()) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let etag = format!("\"{version}\"");
    let validators = [
        (header::ETAG, etag.clone()),
        (header::CACHE_CONTROL, cache.to_string()),
    ];

    // Le client a déjà cette version : `304`, en-têtes seuls, aucun octet relu.
    if let Some(inm) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && inm.split(',').any(|t| {
            let t = t.trim();
            t == "*" || t.trim_start_matches("W/") == etag
        })
    {
        return (validators, StatusCode::NOT_MODIFIED).into_response();
    }

    match tokio::fs::read(&relative).await {
        Ok(bytes) => (
            validators,
            [(header::CONTENT_TYPE, formats::content_type(&q.path))],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Construit un chemin sous `root`, en rejetant toute traversée.
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    if rel.is_absolute() {
        return None;
    }
    // Rejette `.`, `..` et les préfixes d'emplacement (Windows).
    if rel.components().any(|c| {
        matches!(
            c,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(root.join(rel))
}

// ─────────────────────────────────────────────────────────────────────────
// Envoi de fichiers
// ─────────────────────────────────────────────────────────────────────────

/// Taille maximale acceptée par `POST /upload`.
///
/// axum plafonne à 2 Mo par défaut : le moindre G-code dépasserait la limite
/// avant même d'atteindre le handler.
const UPLOAD_MAX_BYTES: usize = 1024 * 1024 * 1024;

/// Query param de `/upload` : chemin relatif du fichier à écrire.
#[derive(Deserialize)]
struct UploadQuery {
    path: String,
}

/// Réponse d'un envoi réussi.
#[derive(Serialize)]
struct UploadResponse {
    /// Chemin relatif écrit, tel qu'il apparaîtra dans le catalogue.
    rel: String,
    /// Nombre d'octets écrits.
    bytes: u64,
}

/// Écrit un fichier dans le dossier des modèles (`POST /upload?path=<rel>`).
///
/// Le corps de la requête **est** le contenu du fichier : ni `multipart`, ni
/// dépendance supplémentaire, et le navigateur peut envoyer un `File` tel quel
/// (`fetch(url, { method: 'POST', body: file })`). Un envoi de dossier se fait
/// donc fichier par fichier, le client recopiant l'arborescence dans `path`.
///
/// Écriture **atomique** : le contenu part dans un fichier temporaire caché du
/// même dossier (donc invisible du scan), puis est renommé sur sa destination.
/// Sans ça, le watcher pourrait scanner — et un aperçu être généré à partir de —
/// un fichier à moitié écrit.
///
/// Écraser est permis : réenvoyer un modèle corrigé est un besoin courant, et
/// c'est sans danger pour les caches (la version du fichier change, donc son
/// `ETag`, l'URL de son aperçu, et l'aperçu lui-même est régénéré).
async fn post_upload(
    State(state): State<AppState>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<UploadResponse>, (StatusCode, String)> {
    let refuse = |msg: String| (StatusCode::BAD_REQUEST, msg);

    // `path` vient du client : mêmes garde-fous que la lecture (voir `safe_join`).
    let Some(relative) = safe_join(&state.root(), &q.path) else {
        return Err(refuse(format!("chemin invalide : « {} »", q.path)));
    };
    // L'API sépare les composants par `/` : une contre-oblique serait un nom de
    // fichier littéral sous Linux mais un séparateur sous Windows. Refusée des
    // deux côtés, pour que le chemin écrit soit celui qui sera relu.
    //
    // Ni dossier, ni élément caché : `.easy3d-thumbs` et `.easy3d-notes`
    // appartiennent au backend, et le scan ignore de toute façon tout ce qui
    // commence par un point.
    if !valid_rel(&q.path) {
        return Err(refuse(format!("chemin réservé : « {} »", q.path)));
    }
    if body.is_empty() {
        return Err(refuse(format!("fichier vide : « {} »", q.path)));
    }
    if relative.is_dir() {
        return Err(refuse(format!(
            "un dossier porte déjà ce nom : « {} »",
            q.path
        )));
    }

    // Les dossiers manquants sont créés : un envoi de dossier arrive fichier par
    // fichier, dans un ordre qui n'est pas garanti.
    if let Some(dir) = relative.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(internal_error)?;
    }

    let tmp = temp_path(&relative);
    tokio::fs::write(&tmp, &body)
        .await
        .map_err(internal_error)?;
    // Sous Windows, `rename` refuse d'écraser : on efface la destination d'abord.
    // Le contenu complet est déjà dans le temporaire, la fenêtre est minuscule.
    if relative.exists() {
        tokio::fs::remove_file(&relative)
            .await
            .map_err(internal_error)?;
    }
    if let Err(e) = tokio::fs::rename(&tmp, &relative).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(internal_error(e));
    }

    // L'écriture vient de l'API elle-même : on ne peut pas se reposer sur le
    // watcher pour l'annoncer. Sous Docker Desktop, le partage de fichiers ne
    // remonte que les événements de la **racine** du montage : un modèle déposé
    // dans un sous-dossier n'en produit aucun, et l'interface resterait figée
    // (dossier affiché vide) jusqu'au rechargement de la page.
    //
    // L'aperçu est donc généré ici (le watcher ne le fera pas non plus), puis
    // la liste est rediffusée à tous les clients WebSocket.
    let root = state.root();
    thumbnail::ensure_for_changed(&root, &root.join(thumbnail::THUMB_DIR), &relative);
    broadcast_snapshot(&state);

    Ok(Json(UploadResponse {
        rel: q.path,
        bytes: body.len() as u64,
    }))
}

/// Chemin du fichier temporaire d'écriture : **caché** (le scan et le
/// générateur d'aperçus l'ignorent), dans le dossier de destination — un
/// `rename` ne traverse pas les systèmes de fichiers.
fn temp_path(target: &Path) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let name = format!(".easy3d-part-{}-{unique}", std::process::id());
    target
        .parent()
        .map_or_else(|| PathBuf::from(&name), |dir| dir.join(&name))
}

/// `true` si un chemin relatif venu du client est acceptable (envoi, renommage).
///
/// L'API sépare les composants par `/` : une contre-oblique serait un nom de
/// fichier littéral sous Linux mais un séparateur sous Windows. Refusée des deux
/// côtés, pour que le chemin écrit soit celui qui sera relu. Ni dossier (pas de
/// `/` final), ni élément caché : `.easy3d-thumbs` et `.easy3d-notes`
/// appartiennent au backend, et le scan ignore de toute façon tout ce qui
/// commence par un point.
fn valid_rel(rel: &str) -> bool {
    !rel.contains('\\')
        && !rel.ends_with('/')
        && !rel
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.'))
}

/// `true` si `name` peut servir de **nom** (dernier segment d'un chemin).
///
/// Un renommage ne déplace rien : seul le dernier composant change, donc aucun
/// séparateur. Sont refusés aussi `.`/`..`, les noms cachés, les caractères de
/// contrôle et ceux que Windows interdit ou tronque — le même binaire tourne des
/// deux côtés, et un nom impossible à écrire là-bas ne doit pas être accepté
/// ici.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && !name.chars().any(char::is_control)
        && !name.starts_with('.')
        && !name.ends_with([' ', '.'])
}

/// Corps de `POST /rename`.
#[derive(Deserialize)]
struct RenameRequest {
    /// Chemin relatif de l'élément à renommer (fichier ou dossier).
    path: String,
    /// Nouveau nom : le **dernier** segment du chemin, sans `/`.
    name: String,
}

/// Réponse d'un renommage réussi : le nouveau chemin relatif.
#[derive(Serialize)]
struct RenameResponse {
    rel: String,
}

/// Renomme un fichier ou un dossier (`POST /rename {path, name}`).
///
/// Un renommage ne change que le **dernier** segment : l'élément reste à sa
/// place. C'est le pendant de l'explorateur de fichiers, et le pendant du
/// `rename` de l'upload — atomique du point de vue du catalogue.
///
/// La note (`.easy3d-notes`) et les aperçus (`.easy3d-thumbs`) suivent : sous
/// Docker Desktop, le watcher ne verrait rien de cette opération (aucun
/// événement pour un montage virtualisé), donc c'est ici que tout se fait, y
/// compris la rediffusion de la liste.
async fn post_rename(
    State(state): State<AppState>,
    Json(request): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, (StatusCode, String)> {
    let root = state.root();

    let Some(from) = safe_join(&root, &request.path) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("chemin invalide : « {} »", request.path),
        ));
    };
    if !valid_rel(&request.path) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("chemin réservé : « {} »", request.path),
        ));
    }
    if !valid_name(&request.name) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("nom invalide : « {} »", request.name),
        ));
    }
    if !from.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("introuvable : « {} »", request.path),
        ));
    }

    // Dernier segment remplacé, le reste du chemin est conservé.
    let to = from.with_file_name(&request.name);
    let to_rel = match request.path.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{}", request.name),
        None => request.name.clone(),
    };
    // Renommer sur soi-même (même nom, à la casse près) : rien à faire, et
    // surtout pas « la destination existe déjà ».
    if to == from {
        return Ok(Json(RenameResponse { rel: request.path }));
    }
    if to.exists() {
        return Err((
            StatusCode::CONFLICT,
            format!("« {} » existe déjà", request.name),
        ));
    }

    // 1. Le modèle lui-même : si ça échoue, rien d'autre n'a bougé.
    tokio::fs::rename(&from, &to)
        .await
        .map_err(internal_error)?;

    // 2. Ce qui l'accompagne, en signalant les échecs sans faire échouer
    //    l'opération : le catalogue, lui, est déjà à jour.
    if let Err(e) = notes::move_for_path(&root, &from, &to) {
        eprintln!(
            "⚠️  Notes non déplacées ({} → {}) : {e}",
            request.path, to_rel
        );
    }
    thumbnail::move_for_path(&root, &root.join(thumbnail::THUMB_DIR), &from, &to);

    // 3. Les clients : la liste complète, comme après un envoi.
    broadcast_snapshot(&state);

    Ok(Json(RenameResponse { rel: to_rel }))
}

/// Corps de `POST /delete`.
#[derive(Deserialize)]
struct DeleteRequest {
    /// Chemin relatif de l'élément à supprimer (fichier ou dossier).
    path: String,
}

/// Réponse d'une suppression réussie.
#[derive(Serialize)]
struct DeleteResponse {
    rel: String,
}

/// Supprime un fichier ou un dossier (`POST /delete {path}`).
///
/// Un dossier part avec **tout son contenu** — c'est ce que fait un explorateur
/// de fichiers, et l'interface prévient avant (nombre de fichiers).
///
/// Ce qui l'accompagne part aussi : la note (`.easy3d-notes`, sous-arbre
/// compris) et les aperçus (`.easy3d-thumbs`). Comme pour le renommage, tout se
/// fait **ici** : sous Docker Desktop, le watcher ne verrait rien de l'opération.
///
/// Les documents collaboratifs encore ouverts sont oubliés au passage : sans
/// ça, une note restée ouverte dans un onglet se réécrirait toute seule sur
/// disque, sous un modèle qui n'existe plus.
async fn post_delete(
    State(state): State<AppState>,
    Json(request): Json<DeleteRequest>,
) -> Result<Json<DeleteResponse>, (StatusCode, String)> {
    let root = state.root();

    let Some(path) = safe_join(&root, &request.path) else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("chemin invalide : « {} »", request.path),
        ));
    };
    // Refuse aussi la racine elle-même : un chemin vide n'est pas un chemin.
    if !valid_rel(&request.path) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("chemin réservé : « {} »", request.path),
        ));
    }
    if !path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("introuvable : « {} »", request.path),
        ));
    }

    // Les documents collaboratifs d'abord : vidés et retirés du registre, ils ne
    // pourront plus réécrire la note après coup.
    collab::Rooms::forget(&state, &request.path);

    // 1. Le modèle — et tout son contenu, si c'est un dossier. Si ça échoue,
    //    rien d'autre n'a bougé.
    let removed = if path.is_dir() {
        tokio::fs::remove_dir_all(&path).await
    } else {
        tokio::fs::remove_file(&path).await
    };
    removed.map_err(internal_error)?;

    // 2. Ce qui l'accompagnait, en signalant les échecs sans faire échouer
    //    l'opération : le catalogue, lui, est déjà à jour.
    notes::remove_for_path(&root, &request.path);
    thumbnail::remove_for_path(&root, &root.join(thumbnail::THUMB_DIR), &path);

    // 3. Les clients : la liste complète, comme après un envoi.
    broadcast_snapshot(&state);

    Ok(Json(DeleteResponse { rel: request.path }))
}

// ─────────────────────────────────────────────────────────────────────────
// Notes Markdown
// ─────────────────────────────────────────────────────────────────────────

/// Réponse de `GET /note` : contenu de la note (vide si aucune).
#[derive(Serialize)]
struct NoteResponse {
    content: String,
}

/// Lit la note d'un dossier ou d'un fichier (`?path=<rel>`).
///
/// Renvoie `{ content: "" }` quand aucune note n'existe : le frontend n'a ainsi
/// pas à distinguer « pas de note » d'une erreur.
///
/// L'écriture, elle, ne passe plus par HTTP : elle est faite par le serveur de
/// synchronisation (voir [`crate::collab`]), seul à même de fusionner des
/// modifications concurrentes.
async fn get_note(State(state): State<AppState>, Query(q): Query<FileQuery>) -> Json<NoteResponse> {
    Json(NoteResponse {
        content: notes::read(&state.root(), &q.path).unwrap_or_default(),
    })
}

/// Rescanne l'état courant et diffuse la liste des modèles aux clients WS.
///
/// Diffuse **toujours** (l'appelant sait qu'il a quelque chose à annoncer) :
/// c'est le re-scan périodique, lui, qui filtre les contenus inchangés.
pub fn broadcast_snapshot(state: &AppState) {
    let payload = snapshot(state);
    *state.last_snapshot.write().unwrap() = payload.clone();
    let _ = state.ws.send(payload);
}

/// Re-scanne le catalogue et ne rediffuse **que si le contenu a changé**.
///
/// Appelé à intervalle régulier quand un re-scan est configuré ou recommandé
/// (`watch.poll_seconds`, voir `main.rs`) : les systèmes de fichiers virtualisés
/// ou réseau (partages de fichiers Docker Desktop, montages `nfs`…) ne remontent
/// aucun événement, donc un dossier ajouté hors de l'interface n'annoncerait
/// rien. Les aperçus manquants sont générés au passage — le watcher, lui, les
/// régénère au fil de ses événements.
///
/// Retourne `true` si une liste a été diffusée.
pub fn refresh_if_changed(state: &AppState) -> bool {
    let unchanged = snapshot(state) == *state.last_snapshot.read().unwrap();
    if unchanged {
        return false;
    }

    let root = state.root();
    thumbnail::generate_all(&root, &root.join(thumbnail::THUMB_DIR));
    broadcast_snapshot(state);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app(root: &str) -> Router {
        let (ws, _) = broadcast::channel::<String>(16);
        routes(AppState::new(root, ws, crate::config::Config::default()))
    }

    #[tokio::test]
    async fn health_renvoie_ok() {
        let app = test_app(".");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn put_config_ecrit_le_fichier_et_renvoie_la_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();

        let (ws, _) = broadcast::channel::<String>(16);
        let app = routes(AppState::new(".", ws, Config::default()).with_config_path(&config_path));

        let body = serde_json::json!({
            "models_root": models.to_string_lossy(),
            "display": { "mode": "image" }
        })
        .to_string();

        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        // La réponse porte la config demandée et le dossier résolu.
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["config"]["display"]["mode"], "image");
        assert_eq!(
            v["models_root"].as_str().unwrap(),
            models.to_string_lossy().as_ref()
        );

        // Le fichier est écrit dans le format que `Config::load_from` relit —
        // c'est exactement ce que fait le watcher pour appliquer à chaud.
        let reloaded = Config::load_from(&config_path);
        assert!(reloaded.is_image_mode());
        assert_eq!(
            reloaded.models_root.as_deref(),
            Some(models.to_string_lossy().as_ref())
        );
    }

    /// Le re-scan périodique se règle depuis l'interface : sa valeur est écrite
    /// dans le YAML (donc relue par le watcher et conservée au redémarrage).
    #[tokio::test]
    async fn put_config_enregistre_le_re_scan_periodique() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();

        let (ws, _) = broadcast::channel::<String>(16);
        let app = routes(AppState::new(".", ws, Config::default()).with_config_path(&config_path));

        let body = serde_json::json!({
            "models_root": models.to_string_lossy(),
            "display": { "mode": "3d" },
            "watch": { "poll_seconds": 15 }
        })
        .to_string();

        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // La réponse dit ce qui est appliqué, en secondes.
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["watch"]["effective"], 15);
        assert_eq!(Config::load_from(&config_path).watch.poll_seconds, Some(15));

        // `0` — désactivé — est une valeur choisie, pas « automatique ».
        assert_eq!(Config::default().watch.poll_seconds, None);
    }

    /// `/config` expose de quoi expliquer la recommandation dans l'interface :
    /// l'intervalle effectif, l'intervalle recommandé, et le système de fichiers
    /// qui les décide.
    #[tokio::test]
    async fn config_expose_la_recommandation_de_re_scan() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();

        let (ws, _) = broadcast::channel::<String>(16);
        let state = AppState::new(models.clone(), ws, Config::default());
        let app = routes(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Un dossier temporaire est sur un disque local : aucun re-scan n'est
        // recommandé, et rien n'est appliqué tant que l'utilisateur n'a rien dit.
        assert_eq!(v["watch"]["recommended"], 0);
        assert_eq!(v["watch"]["effective"], 0);
    }

    #[tokio::test]
    async fn put_config_refuse_un_dossier_introuvable() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let missing = dir.path().join("pas-la");

        let (ws, _) = broadcast::channel::<String>(16);
        let app = routes(AppState::new(".", ws, Config::default()).with_config_path(&config_path));

        let body = serde_json::json!({ "models_root": missing.to_string_lossy() }).to_string();

        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        // Rien n'a été écrit : on n'enregistre pas une config inutilisable.
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn models_renvoie_racine_et_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        // Fichier à la racine.
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        // Fichiers rangés dans un sous-dossier.
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "y").unwrap();
        std::fs::write(dir.path().join("sub/c.txt"), "z").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Total global = 1 racine + 2 dans le dossier.
        assert_eq!(v["count"], 3);

        // Le fichier racine est bien séparé des dossiers.
        let root_files = v["files"].as_array().unwrap();
        assert_eq!(root_files.len(), 1);
        assert!(root_files[0]["path"].as_str().unwrap().ends_with("a.txt"));

        // Le dossier `sub` regroupe ses 2 fichiers + un compteur.
        let folder = &v["folders"]["sub"];
        assert_eq!(folder["count"], 2);
        let folder_files = folder["files"].as_array().unwrap();
        assert_eq!(folder_files.len(), 2);

        // Les formats reconnus accompagnent la liste : le frontend s'en sert
        // pour l'affichage, sans extensions codées en dur.
        let formats = v["formats"].as_array().unwrap();
        let gcode = formats
            .iter()
            .find(|f| f["name"] == "G-code")
            .expect("le G-code est annoncé");
        assert_eq!(gcode["extensions"], serde_json::json!(["gcode", "gco"]));
        assert_eq!(gcode["viewer"], "gcode");
        assert_eq!(gcode["preview"], true);

        let stl = formats.iter().find(|f| f["name"] == "STL").unwrap();
        assert_eq!(stl["viewer"], "mesh");
    }

    #[tokio::test]
    async fn route_inconnue_renvoie_404() {
        let app = test_app(".");
        let res = app
            .oneshot(Request::builder().uri("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    /// État partagé configuré avec un fournisseur d'IA et une clé.
    fn ai_state(dir: &Path, key: &str) -> AppState {
        let mut config = Config::default();
        config.ai = config::Ai {
            provider: Some("openai".to_string()),
            api_key: Some(key.to_string()),
            ..Default::default()
        };
        let (ws, _) = broadcast::channel::<String>(16);
        AppState::new(".", ws, config).with_config_path(dir.join("config.yml"))
    }

    async fn json_of(res: Response) -> serde_json::Value {
        let body = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).unwrap()
    }

    /// La clé d'API est un secret : elle ne sort **jamais** du backend, ni par
    /// `/config` ni par `/models` (qui est aussi le contenu diffusé en WebSocket).
    #[tokio::test]
    async fn la_cle_d_api_ne_sort_jamais_du_backend() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state.clone());

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v = json_of(res).await;
        assert_eq!(v["config"]["ai"]["provider"], "openai");
        assert_eq!(v["config"]["ai"]["api_key"], config::KEY_PLACEHOLDER);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v = json_of(res).await;
        assert_eq!(v["config"]["ai"]["api_key"], config::KEY_PLACEHOLDER);

        // …alors que l'état, lui, la conserve pour appeler le fournisseur.
        assert_eq!(state.config().ai.api_key.as_deref(), Some("sk-secret"));
    }

    /// Le formulaire d'administration ne connaît pas la clé qu'il affiche : le
    /// marqueur veut dire « garde-la », la chaîne vide « efface-la ».
    #[tokio::test]
    async fn put_config_gere_la_cle_d_api() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state.clone());

        let put = |body: String| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/config")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        // Le marqueur renvoyé tel quel : la clé enregistrée est conservée.
        let res = put(serde_json::json!({
            "models_root": ".",
            "ai": { "provider": "openai", "api_key": config::KEY_PLACEHOLDER }
        })
        .to_string())
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(state.config().ai.api_key.as_deref(), Some("sk-secret"));

        // Un champ absent : conservée aussi (l'interface n'est pas obligée de
        // renvoyer le bloc entier).
        let res = put(
            serde_json::json!({ "models_root": ".", "ai": { "provider": "openai" } }).to_string(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(state.config().ai.api_key.as_deref(), Some("sk-secret"));

        // Une nouvelle clé la remplace…
        let res = put(serde_json::json!({
            "models_root": ".",
            "ai": { "provider": "openai", "api_key": "sk-new" }
        })
        .to_string())
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(state.config().ai.api_key.as_deref(), Some("sk-new"));

        // …et une chaîne vide la supprime.
        let res = put(
            serde_json::json!({ "models_root": ".", "ai": { "provider": "openai", "api_key": "" } })
                .to_string(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(state.config().ai.api_key.is_none());
    }

    /// La liste des modèles voyage avec le reste : nettoyée à l'écriture, et
    /// conservée tant qu'on reste sur le même fournisseur et la même adresse.
    #[tokio::test]
    async fn put_config_gere_la_liste_des_modeles() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state.clone());

        let put = |body: serde_json::Value| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/config")
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        // Une liste reçue est nettoyée : espaces, doublons et entrées vides.
        let res = put(serde_json::json!({
            "models_root": ".",
            "ai": { "provider": "openai", "models": ["gpt-4o", " gpt-4o ", "", "o3-mini"] }
        }))
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(state.config().ai.models, vec!["gpt-4o", "o3-mini"]);

        // Un enregistrement qui ne la contient pas la laisse tranquille.
        let res = put(serde_json::json!({
            "models_root": ".",
            "ai": { "provider": "openai", "model": "o3-mini" }
        }))
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(state.config().ai.models, vec!["gpt-4o", "o3-mini"]);

        // Changer d'adresse repart d'une liste vide : elle appartenait à
        // l'ancien point d'entrée.
        let res = put(serde_json::json!({
            "models_root": ".",
            "ai": { "provider": "openai", "base_url": "http://127.0.0.1:9/v1" }
        }))
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(state.config().ai.models.is_empty());
    }

    /// Pas de valeur orpheline : sans fournisseur, la clé **et** le modèle
    /// disparaissent du fichier.
    #[tokio::test]
    async fn changer_de_fournisseur_sans_cle_efface_la_cle() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state.clone());

        // Un modèle enregistré, comme le laisserait un paramétrage précédent.
        state.config.write().unwrap().ai.model = Some("gpt-4o-mini".to_string());

        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "models_root": ".",
                            "ai": { "provider": null, "api_key": "sk-orph", "model": "gpt-4o-mini" }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let saved = state.config().ai;
        assert!(saved.api_key.is_none());
        assert!(saved.model.is_none());
        assert!(!saved.is_configured());
    }

    /// Les fournisseurs sont annoncés par le backend : le frontend n'en connaît
    /// aucun en dur, et sait lequel se passe de clé.
    #[tokio::test]
    async fn les_fournisseurs_d_ia_sont_annonces() {
        let app = test_app(".");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/ai/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let v = json_of(res).await;
        let providers = v.as_array().unwrap();
        let ids: Vec<&str> = providers
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["openai", "anthropic", "ollama"]);

        for provider in providers {
            assert!(provider["label"].as_str().unwrap().len() > 2);
            assert!(provider["base_url"].as_str().unwrap().starts_with("http"));
            assert!(!provider["model"].as_str().unwrap().is_empty());

            // Les adresses connues voyagent avec le fournisseur : l'interface
            // n'a donc aucune adresse en dur, et la première est celle du
            // fournisseur lui-même (un champ vide vaut ce défaut).
            let presets = provider["presets"].as_array().unwrap();
            assert!(!presets.is_empty());
            assert!(
                presets
                    .iter()
                    .all(|p| !p["label"].as_str().unwrap().is_empty())
            );
            assert_eq!(presets[0]["base_url"], provider["base_url"]);
        }

        let ollama = providers.iter().find(|p| p["id"] == "ollama").unwrap();
        assert_eq!(ollama["needs_key"], false);
        let openai = providers.iter().find(|p| p["id"] == "openai").unwrap();
        assert_eq!(openai["needs_key"], true);
    }

    /// Sans fournisseur configuré, la recherche IA répond une erreur **lisible** :
    /// c'est le message que l'utilisateur verra s'il force l'appel.
    #[tokio::test]
    async fn la_recherche_ia_refuse_une_configuration_absente() {
        let app = test_app(".");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/search")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"une pièce en PETG"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let message = String::from_utf8_lossy(&body);
        assert!(message.contains("fournisseur"), "{message}");

        // Une question vide ne part pas non plus.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/search")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("décris"));
    }

    /// Le fournisseur est injoignable : l'erreur d'appel remonte telle quelle
    /// (ici, un port fermé), sans jamais citer la clé.
    #[tokio::test]
    async fn la_liste_des_modeles_remonte_l_erreur_du_fournisseur() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/models")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "provider": "openai",
                            "base_url": "http://127.0.0.1:9/v1",
                            // Le marqueur : la clé enregistrée est réutilisée,
                            // sans jamais repasser par le navigateur.
                            "api_key": config::KEY_PLACEHOLDER
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let message = String::from_utf8_lossy(&body);
        assert!(message.contains("impossible"), "{message}");
        assert!(message.contains("127.0.0.1:9"), "{message}");
        assert!(!message.contains("sk-secret"), "{message}");

        // Sans fournisseur, la même route le dit avant tout appel réseau.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/models")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("fournisseur"));
    }

    #[tokio::test]
    async fn file_sert_le_contenu() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/model.stl"), b"solid x").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/file?path=sub%2Fmodel.stl")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
        assert_eq!(ct, "model/stl");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"solid x");
    }

    /// Sans `?v=`, la réponse n'est pas immuable mais reste revalidable :
    /// le second appel avec le même `ETag` ne renvoie aucun octet.
    #[tokio::test]
    async fn file_repond_304_si_etag_identique() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.stl"), b"solid x").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/file?path=model.stl")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(first.headers().get("cache-control").unwrap(), "no-cache");
        let etag = first
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let second = app
            .oneshot(
                Request::builder()
                    .uri("/file?path=model.stl")
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        let body = second.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "un 304 ne porte pas de corps");
    }

    /// La version annoncée par le scan, recopiée dans l'URL, rend la réponse
    /// cacheable « pour toujours » ; une version périmée retombe en `no-cache`.
    #[tokio::test]
    async fn file_immuable_quand_la_version_correspond() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("piece.gcode"), "G1 X0").unwrap();
        std::fs::write(dir.path().join("piece.png"), b"faux png").unwrap();

        let files = scanner::scan_files(dir.path());
        let piece = files.iter().find(|f| f.rel == "piece.gcode").unwrap();
        let version = piece.image_version.clone().expect("version de l'aperçu");

        let app = test_app(dir.path().to_str().unwrap());
        let fresh = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/file?path=piece.png&v={version}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(fresh.status(), StatusCode::OK);
        assert_eq!(
            fresh.headers().get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );
        assert!(fresh.headers().get("etag").is_some());

        let stale = app
            .oneshot(
                Request::builder()
                    .uri("/file?path=piece.png&v=0-0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::OK);
        assert_eq!(stale.headers().get("cache-control").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn file_rejette_traversee() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("secret.txt"), "x").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        // `..` (encodé) est rejeté → 400, on ne lit pas hors de la racine.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/file?path=..%2Fsecret.txt")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upload_ecrit_le_fichier_et_cree_les_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload?path=sous%2Fnouveau.stl")
                    .body(Body::from("solid x"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(json["rel"], "sous/nouveau.stl");
        assert_eq!(json["bytes"], 7);

        let written = std::fs::read_to_string(dir.path().join("sous/nouveau.stl")).unwrap();
        assert_eq!(written, "solid x");
        // Aucun temporaire laissé derrière (il est caché, donc jamais scanné).
        let leftovers = std::fs::read_dir(dir.path().join("sous"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".easy3d-part"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[tokio::test]
    async fn upload_remplace_le_fichier_existant() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.stl"), "ancien").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload?path=model.stl")
                    .body(Body::from("nouveau"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let written = std::fs::read_to_string(dir.path().join("model.stl")).unwrap();
        assert_eq!(written, "nouveau");
    }

    /// Chemins que l'écriture doit refuser : hors racine, réservés, ou visant un
    /// dossier.
    #[tokio::test]
    async fn upload_refuse_les_chemins_invalides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("DemaAuto")).unwrap();
        let app = test_app(dir.path().to_str().unwrap());

        for rel in [
            "..%2Fsecret.stl",        // hors de la racine
            "%2Fabsolu.stl",          // chemin absolu
            "a%5Cb.stl",              // contre-oblique (séparateur Windows)
            "sous%2F",                // se termine par un séparateur
            ".easy3d-thumbs%2Fx.png", // dossier interne du backend
            "DemaAuto",               // un dossier porte déjà ce nom
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/upload?path={rel}"))
                        .body(Body::from("x"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::BAD_REQUEST,
                "devrait refuser {rel}"
            );
        }
    }

    /// Un envoi rediffuse **immédiatement** la liste, sans attendre le watcher.
    ///
    /// C'est ce qui fait apparaître un sous-dossier fraîchement créé : le
    /// watcher ne peut pas s'en charger (sous Docker Desktop, le partage de
    /// fichiers ne remonte que les événements de la racine du montage).
    #[tokio::test]
    async fn upload_rediffuse_la_liste_sans_le_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let mut updates = state.ws.subscribe();

        let res = routes(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload?path=Maison%2FToit%2Fpiece.stl")
                    .body(Body::from("solid x\nendsolid x\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Au moins une rediffusion, dont le contenu voit déjà le fichier : c'est
        // la condition pour que le dossier n'apparaisse pas « vide ».
        let payload = updates
            .try_recv()
            .expect("aucune rediffusion après l'envoi");
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["folders"]["Maison"]["subfolders"][0]["count"], 1);
        assert_eq!(
            v["folders"]["Maison"]["subfolders"][0]["rel"],
            "Maison/Toit"
        );
    }

    /// Le re-scan périodique rattrape ce qu'aucun événement n'a signalé : un
    /// dossier ajouté **hors de l'interface**, comme dans le partage de fichiers
    /// d'un conteneur Docker Desktop (qui ne remonte rien du tout).
    #[tokio::test]
    async fn rescan_periodique_rediffuse_un_ajout_hors_interface() {
        let dir = tempfile::tempdir().unwrap();
        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let mut updates = state.ws.subscribe();

        // Premier tic : rien à comparer, la liste (vide) est publiée une fois.
        assert!(refresh_if_changed(&state));
        let _ = updates.try_recv().unwrap();

        // Rien n'a bougé → aucune rediffusion : le frontend remplace son état à
        // chaque message, un tic ne doit pas provoquer de re-rendu.
        assert!(!refresh_if_changed(&state));
        assert!(updates.try_recv().is_err(), "rediffusion inutile");

        // L'ajout, fait « à la main » : aucun événement n'entre en jeu ici.
        std::fs::create_dir_all(dir.path().join("Maison/Toit")).unwrap();
        std::fs::write(
            dir.path().join("Maison/Toit/piece.stl"),
            "solid piece\n\
             facet normal 0 0 1\n\
             outer loop\n\
             vertex 0 0 0\n\
             vertex 1 0 0\n\
             vertex 0 1 0\n\
             endloop\n\
             endfacet\n\
             endsolid piece\n",
        )
        .unwrap();

        assert!(refresh_if_changed(&state));
        let payload = updates
            .try_recv()
            .expect("aucune rediffusion après l'ajout");
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["folders"]["Maison"]["subfolders"][0]["count"], 1);
        // Aperçu généré au passage : la carte a une vignette, comme après un
        // envoi par l'interface.
        assert!(
            v["folders"]["Maison"]["files"][0]["image"].is_string(),
            "aperçu manquant : {v}"
        );
        // Stabilisé : plus rien à annoncer tant que rien ne bouge.
        assert!(!refresh_if_changed(&state));
    }

    #[tokio::test]
    async fn upload_refuse_un_fichier_vide() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/upload?path=vide.stl")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(!dir.path().join("vide.stl").exists());
    }

    /// STL minimal mais valide (une facette) : l'aperçu doit pouvoir le lire.
    const STL: &str = "solid piece\n\
         facet normal 0 0 1\n\
         outer loop\n\
         vertex 0 0 0\n\
         vertex 1 0 0\n\
         vertex 0 1 0\n\
         endloop\n\
         endfacet\n\
         endsolid piece\n";

    /// Renommage d'un fichier : le fichier, sa note et son aperçu suivent, et la
    /// liste est rediffusée — le watcher ne verrait rien de tout ça sous Docker
    /// Desktop (montage virtualisé, aucun événement).
    #[tokio::test]
    async fn rename_deplace_fichier_note_et_apercu() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison")).unwrap();
        std::fs::write(dir.path().join("Maison/piece.stl"), STL).unwrap();
        notes::write(dir.path(), "Maison/piece.stl", "# ma note").unwrap();
        // Un aperçu déjà généré, à l'ancien emplacement.
        std::fs::create_dir_all(dir.path().join(thumbnail::THUMB_DIR).join("Maison")).unwrap();
        std::fs::write(
            dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/piece.stl.png"),
            "png",
        )
        .unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let mut updates = state.ws.subscribe();

        let res = routes(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/rename")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"path":"Maison/piece.stl","name":"toit.stl"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["rel"], "Maison/toit.stl");

        assert!(!dir.path().join("Maison/piece.stl").exists());
        assert!(dir.path().join("Maison/toit.stl").is_file());

        // La note a suivi son fichier, l'ancienne place est libre.
        assert_eq!(
            notes::read(dir.path(), "Maison/toit.stl").as_deref(),
            Some("# ma note")
        );
        assert!(notes::read(dir.path(), "Maison/piece.stl").is_none());

        // L'aperçu a été **déplacé**, pas régénéré : l'octet d'origine est là.
        assert_eq!(
            std::fs::read_to_string(
                dir.path()
                    .join(thumbnail::THUMB_DIR)
                    .join("Maison/toit.stl.png")
            )
            .unwrap(),
            "png"
        );

        // Diffusion immédiate : le client voit le nouveau nom sans recharger.
        let payload = updates
            .try_recv()
            .expect("aucune rediffusion après le renommage");
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["folders"]["Maison"]["files"][0]["rel"], "Maison/toit.stl");
    }

    /// Renommer un dossier emporte tout son sous-arbre : notes et aperçus.
    #[tokio::test]
    async fn rename_de_dossier_emporte_le_sous_arbre() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison/Toit")).unwrap();
        std::fs::write(dir.path().join("Maison/Toit/piece.stl"), STL).unwrap();
        notes::write(dir.path(), "Maison/Toit", "## le toit").unwrap();
        notes::write(dir.path(), "Maison/Toit/piece.stl", "# la pièce").unwrap();
        for rel in ["Maison/Toit/piece.stl.png", "Maison/piece.stl.png"] {
            let path = dir.path().join(thumbnail::THUMB_DIR).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "png").unwrap();
        }

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );

        let res = routes(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/rename")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"Maison/Toit","name":"Toiture"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        assert!(dir.path().join("Maison/Toiture/piece.stl").is_file());
        // Les notes du dossier **et** de ses descendants.
        assert_eq!(
            notes::read(dir.path(), "Maison/Toiture").as_deref(),
            Some("## le toit")
        );
        assert_eq!(
            notes::read(dir.path(), "Maison/Toiture/piece.stl").as_deref(),
            Some("# la pièce")
        );
        // Les aperçus du sous-arbre, en bloc (l'aperçu du dossier parent, lui,
        // n'a pas bougé).
        assert!(
            dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/Toiture/piece.stl.png")
                .is_file()
        );
        assert!(
            dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/piece.stl.png")
                .is_file()
        );
    }

    /// Garde-fous du renommage : ce que l'API doit refuser, et pourquoi.
    #[tokio::test]
    async fn rename_refuse_les_cas_invalides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison")).unwrap();
        std::fs::write(dir.path().join("Maison/piece.stl"), STL).unwrap();
        std::fs::write(dir.path().join("Maison/vis.stl"), STL).unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let app = routes(state);

        for (path, name, attendu) in [
            // Un renommage ne déplace pas : pas de séparateur dans le nom.
            ("Maison/piece.stl", "sous/toit.stl", StatusCode::BAD_REQUEST),
            // Ni remontée, ni nom caché, ni nom vide.
            ("Maison/piece.stl", "..", StatusCode::BAD_REQUEST),
            ("Maison/piece.stl", ".cache", StatusCode::BAD_REQUEST),
            ("Maison/piece.stl", "", StatusCode::BAD_REQUEST),
            // Caractères refusés sous Windows (le même binaire y tourne).
            ("Maison/piece.stl", "toit?.stl", StatusCode::BAD_REQUEST),
            // Hors de la racine des modèles.
            ("../secret.stl", "x.stl", StatusCode::BAD_REQUEST),
            // La destination est déjà prise.
            ("Maison/piece.stl", "vis.stl", StatusCode::CONFLICT),
            // La source n'existe pas.
            ("Maison/absent.stl", "toit.stl", StatusCode::NOT_FOUND),
        ] {
            let body = serde_json::json!({ "path": path, "name": name }).to_string();
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/rename")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), attendu, "« {path} » → « {name} »");
        }

        // Renommer sur soi-même est sans effet (et sans erreur) : le client peut
        // valider sans avoir rien changé.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/rename")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"path":"Maison/piece.stl","name":"piece.stl"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(dir.path().join("Maison/piece.stl").is_file());
    }

    /// Suppression d'un fichier : le fichier, sa note et son aperçu partent, et
    /// la liste est rediffusée — le watcher ne verrait rien de tout ça sous
    /// Docker Desktop.
    #[tokio::test]
    async fn delete_supprime_fichier_note_et_apercu() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison")).unwrap();
        std::fs::write(dir.path().join("Maison/piece.stl"), STL).unwrap();
        std::fs::write(dir.path().join("Maison/garde.stl"), STL).unwrap();
        notes::write(dir.path(), "Maison/piece.stl", "# à jeter").unwrap();
        notes::write(dir.path(), "Maison/garde.stl", "# à garder").unwrap();
        for rel in ["Maison/piece.stl.png", "Maison/garde.stl.png"] {
            let path = dir.path().join(thumbnail::THUMB_DIR).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "png").unwrap();
        }

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let mut updates = state.ws.subscribe();

        let res = routes(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/delete")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"Maison/piece.stl"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        assert!(!dir.path().join("Maison/piece.stl").exists());
        assert!(notes::read(dir.path(), "Maison/piece.stl").is_none());
        assert!(
            !dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/piece.stl.png")
                .exists()
        );

        // Le voisin est intact : ni son fichier, ni sa note, ni son aperçu.
        assert!(dir.path().join("Maison/garde.stl").is_file());
        assert_eq!(
            notes::read(dir.path(), "Maison/garde.stl").as_deref(),
            Some("# à garder")
        );
        assert!(
            dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/garde.stl.png")
                .is_file()
        );

        let payload = updates
            .try_recv()
            .expect("aucune rediffusion après la suppression");
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["folders"]["Maison"]["files"].as_array().unwrap().len(), 1);
    }

    /// Un dossier part avec **tout** son contenu : modèles, notes et aperçus du
    /// sous-arbre. Ceux d'à côté restent.
    #[tokio::test]
    async fn delete_de_dossier_emporte_le_sous_arbre() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison/Toit")).unwrap();
        std::fs::write(dir.path().join("Maison/Toit/piece.stl"), STL).unwrap();
        std::fs::write(dir.path().join("Maison/garde.stl"), STL).unwrap();
        notes::write(dir.path(), "Maison/Toit", "## le toit").unwrap();
        notes::write(dir.path(), "Maison/Toit/piece.stl", "# la pièce").unwrap();
        notes::write(dir.path(), "Maison/garde.stl", "# à garder").unwrap();
        for rel in ["Maison/Toit/piece.stl.png", "Maison/garde.stl.png"] {
            let path = dir.path().join(thumbnail::THUMB_DIR).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "png").unwrap();
        }

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );

        let res = routes(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/delete")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"Maison/Toit"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        assert!(!dir.path().join("Maison/Toit").exists());
        assert!(notes::read(dir.path(), "Maison/Toit").is_none());
        assert!(notes::read(dir.path(), "Maison/Toit/piece.stl").is_none());
        assert!(
            !dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/Toit")
                .exists()
        );

        // Le reste du dossier, lui, n'a pas bougé.
        assert!(dir.path().join("Maison/garde.stl").is_file());
        assert_eq!(
            notes::read(dir.path(), "Maison/garde.stl").as_deref(),
            Some("# à garder")
        );
        assert!(
            dir.path()
                .join(thumbnail::THUMB_DIR)
                .join("Maison/garde.stl.png")
                .is_file()
        );
    }

    /// Une note **encore ouverte** dans un onglet ne ressuscite pas après la
    /// suppression de son fichier : le document collaboratif est oublié avec.
    #[tokio::test]
    async fn delete_oublie_les_documents_collaboratifs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("piece.stl"), STL).unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );

        // Une note ouverte : son document vit en mémoire, comme dans un onglet
        // resté connecté.
        collab::Rooms::set_text(&state, "piece.stl", "# note ouverte").unwrap();
        assert_eq!(
            collab::Rooms::live_text(&state, "piece.stl").as_deref(),
            Some("# note ouverte")
        );

        let res = routes(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/delete")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"path":"piece.stl"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Le document est oublié : il ne réécrira pas la note du fichier
        // supprimé.
        assert!(collab::Rooms::live_text(&state, "piece.stl").is_none());
        assert!(notes::read(dir.path(), "piece.stl").is_none());
        assert!(!dir.path().join("piece.stl").exists());
    }

    /// Garde-fous de la suppression : ce que l'API doit refuser.
    #[tokio::test]
    async fn delete_refuse_les_chemins_invalides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Maison")).unwrap();
        std::fs::create_dir_all(dir.path().join(thumbnail::THUMB_DIR)).unwrap();
        std::fs::write(dir.path().join("Maison/piece.stl"), STL).unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let app = routes(state);

        for (path, attendu) in [
            // La racine des modèles n'est pas un élément du catalogue.
            ("", StatusCode::BAD_REQUEST),
            ("/", StatusCode::BAD_REQUEST),
            // Ni un dossier interne du backend, ni un chemin caché.
            (".easy3d-thumbs", StatusCode::BAD_REQUEST),
            ("Maison/.cache", StatusCode::BAD_REQUEST),
            // Hors de la racine.
            ("../secret.stl", StatusCode::BAD_REQUEST),
            // Déjà absent.
            ("Maison/absent.stl", StatusCode::NOT_FOUND),
        ] {
            let body = serde_json::json!({ "path": path }).to_string();
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/delete")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), attendu, "chemin « {path} »");
        }

        // Rien n'a été touché au passage.
        assert!(dir.path().join("Maison/piece.stl").is_file());
        assert!(dir.path().join(thumbnail::THUMB_DIR).is_dir());
    }

    /// Intégration WebSocket : connexion réelle → snapshot initial, puis
    /// réception d'une MAJ diffusée sur le canal broadcast.
    #[tokio::test]
    async fn ws_pousse_snapshot_puis_diffusion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "y").unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx.clone(),
            crate::config::Config::default(),
        );
        let app = Router::new().route("/ws", get(ws_models)).with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();

        // Snapshot initial envoyé à la connexion : 1 fichier racine + 1 dossier.
        let snap = socket.next().await.unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(snap.to_text().unwrap()).unwrap();
        assert_eq!(v["count"], 2);

        // Diffusion d'une mise à jour sur le canal broadcast.
        let update = serde_json::json!({ "count": 3, "folders": {}, "files": [] }).to_string();
        ws_tx.send(update).unwrap();

        // Le client reçoit bien la MAJ diffusée.
        let msg = socket.next().await.unwrap().unwrap();
        let v2: serde_json::Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        assert_eq!(v2["count"], 3);
    }

    #[tokio::test]
    async fn note_absente_renvoie_un_contenu_vide() {
        let dir = tempfile::tempdir().unwrap();
        let app = test_app(dir.path().to_str().unwrap());

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/note?path=sub%2Fmodel.stl")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["content"], "");
    }

    #[tokio::test]
    async fn note_lue_depuis_le_disque() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(notes::NOTES_DIR).join("sub")).unwrap();
        std::fs::write(
            dir.path().join(notes::NOTES_DIR).join("sub/model.stl.md"),
            "# Titre\n**gras**",
        )
        .unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/note?path=sub%2Fmodel.stl")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["content"], "# Titre\n**gras**");
    }

    #[tokio::test]
    async fn note_de_dossier_est_exposee_par_le_scan() {
        let dir = tempfile::tempdir().unwrap();
        // Le dossier doit exister pour apparaître dans le scan.
        std::fs::create_dir_all(dir.path().join("DemaAuto")).unwrap();
        std::fs::create_dir_all(dir.path().join(notes::NOTES_DIR)).unwrap();
        std::fs::write(
            dir.path().join(notes::NOTES_DIR).join("DemaAuto.md"),
            "## Dossier",
        )
        .unwrap();

        // La note du dossier est exposée par le scan (dossier sans fichier).
        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["folders"]["DemaAuto"]["note"], "## Dossier");
    }

    /// La liste diffusée aux clients WebSocket reflète les notes écrites sur
    /// disque : c'est elle qui alimente la pastille 📝 des cartes.
    #[tokio::test]
    async fn une_note_apparait_dans_la_liste_diffusee() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState::new(
            dir.path().to_path_buf(),
            ws_tx,
            crate::config::Config::default(),
        );
        let app = routes(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        // Snapshot initial (aucune note).
        let snap = socket.next().await.unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(snap.to_text().unwrap()).unwrap();
        assert!(v["files"][0]["note"].is_null());

        // La note est écrite sur disque — comme le fait le serveur collaboratif —
        // puis la liste est rediffusée.
        std::fs::create_dir_all(dir.path().join(notes::NOTES_DIR)).unwrap();
        std::fs::write(
            dir.path().join(notes::NOTES_DIR).join("a.txt.md"),
            "la note",
        )
        .unwrap();
        broadcast_snapshot(&state);

        // Le client WS reçoit la liste mise à jour, note comprise.
        let msg = socket.next().await.unwrap().unwrap();
        let v: serde_json::Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        assert_eq!(v["files"][0]["note"], "la note");
    }

    // ── Comptes utilisateurs ────────────────────────────────────────────────
    //
    // Les tests parlent à la même table de routes que la production : c'est
    // elle qui décide ce qui est public, pas une variante de test.

    /// Application dont l'authentification **est active**, avec un compte
    /// administrateur créé comme au démarrage du serveur.
    ///
    /// `prepare` reçoit la base ouverte pour les cas particuliers (compte
    /// désactivé, second compte…). Le dossier temporaire est rendu à l'appelant
    /// pour rester vivant pendant le test.
    fn app_avec_comptes(prepare: impl FnOnce(&auth::Auth)) -> (Router, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        // Catalogue à part : les tests qui écrivent ou suppriment ne doivent pas
        // toucher aux dossiers du dépôt.
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        let comptes = auth::Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
        let seed = auth::AdminSeed {
            username: "remi".into(),
            email: "remi@exemple.fr".into(),
            password: "motdepasse".into(),
        };
        auth::bootstrap_admin(&comptes, Some(seed)).unwrap();
        prepare(&comptes);

        let (ws, _) = broadcast::channel::<String>(16);
        let state = AppState::new(models, ws, Config::default()).with_auth(comptes);
        (routes(state), dir)
    }

    /// `POST` JSON, avec un cookie de session en option.
    async fn post_json(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        app.clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    /// `GET`, avec un cookie de session en option.
    async fn get_with(app: &Router, uri: &str, cookie: Option<&str>) -> Response {
        let mut request = Request::builder().uri(uri);
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        app.clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    /// Cookie `nom=valeur` posé par une réponse (sans ses attributs).
    fn cookie_de(res: &Response) -> String {
        res.headers()
            .get(header::SET_COOKIE)
            .expect("la réponse doit poser un cookie de session")
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn body_text(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    async fn body_json(res: Response) -> serde_json::Value {
        serde_json::from_str(&body_text(res).await).unwrap()
    }

    /// Le test de non-régression le plus important du lot : **sans**
    /// `EASY3D_AUTH`, le serveur se comporte exactement comme avant.
    #[tokio::test]
    async fn sans_authentification_le_catalogue_reste_ouvert() {
        let app = test_app(".");

        let res = get_with(&app, "/models", None).await;
        assert_eq!(res.status(), StatusCode::OK);

        // Et la route de connexion ne devient pas une porte dérobée : elle dit
        // qu'il n'y a rien à quoi se connecter.
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "auth_disabled");

        let res = get_with(&app, "/auth/me", None).await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["enabled"], false);
        assert!(v["user"].is_null());
    }

    #[tokio::test]
    async fn avec_authentification_tout_est_ferme_sans_cookie() {
        let (app, _dir) = app_avec_comptes(|_| {});

        for uri in ["/models", "/config", "/note?path=x"] {
            let res = get_with(&app, uri, None).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "uri : {uri}");
            assert_eq!(body_text(res).await, "unauthenticated");
        }

        // Le serveur MCP est protégé lui aussi : c'est la route qui écrit dans
        // les notes, elle ne doit pas rester ouverte aux anonymes. Un agent
        // s'authentifiera par jeton (étape suivante), pas par cookie.
        let res = post_json(&app, "/mcp", serde_json::json!({}), None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn la_connexion_ouvre_l_acces_et_la_deconnexion_le_referme() {
        let (app, _dir) = app_avec_comptes(|_| {});

        // Connexion par **e-mail** (le nom d'utilisateur marche aussi).
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi@exemple.fr", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = cookie_de(&res);
        assert!(cookie.starts_with("easy3d_session="));

        let v = body_json(res).await;
        assert_eq!(v["username"], "remi");
        assert_eq!(v["roles"][0], "admin");
        assert!(v.get("password_hash").is_none(), "json : {v}");

        // Le catalogue s'ouvre avec le cookie de session.
        let res = get_with(&app, "/models", Some(&cookie)).await;
        assert_eq!(res.status(), StatusCode::OK);

        let res = get_with(&app, "/auth/me", Some(&cookie)).await;
        let v = body_json(res).await;
        assert_eq!(v["enabled"], true);
        assert_eq!(v["user"]["username"], "remi");

        // Déconnexion : la session est fermée **côté serveur** (le cookie volé
        // ne vaut plus rien) et effacée côté navigateur.
        let res = post_json(
            &app,
            "/auth/logout",
            serde_json::json!({}),
            Some(&cookie),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        // Le cookie de fermeture garde les mêmes attributs et expire tout de
        // suite : le navigateur l'oublie.
        let ferme = res
            .headers()
            .get(header::SET_COOKIE)
            .expect("la déconnexion doit effacer le cookie")
            .to_str()
            .unwrap()
            .to_string();
        assert!(ferme.contains("Max-Age=0"), "cookie : {ferme}");
        assert!(ferme.contains("HttpOnly"), "cookie : {ferme}");

        let res = get_with(&app, "/models", Some(&cookie)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn un_mauvais_mot_de_passe_et_un_compte_inconnu_donnent_le_meme_refus() {
        let (app, _dir) = app_avec_comptes(|_| {});

        let connu = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "mauvais" }),
            None,
        )
        .await;
        assert_eq!(connu.status(), StatusCode::UNAUTHORIZED);
        assert!(
            connu.headers().get(header::SET_COOKIE).is_none(),
            "un échec ne doit jamais ouvrir de session"
        );
        let corps = body_text(connu).await;

        let inconnu = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "personne", "password": "mauvais" }),
            None,
        )
        .await;
        assert_eq!(inconnu.status(), StatusCode::UNAUTHORIZED);
        // Même code, donc même message : rien ne dit si le compte existe.
        assert_eq!(body_text(inconnu).await, corps);
        assert_eq!(corps, "invalid_credentials");
    }

    #[tokio::test]
    async fn un_compte_desactive_ne_peut_plus_se_connecter() {
        let (app, _dir) = app_avec_comptes(|comptes| {
            comptes
                .db(|conn| {
                    conn.execute("UPDATE users SET disabled = 1", [])
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .unwrap();
        });

        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(res.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn trop_de_connexions_ratees_finissent_en_429() {
        let (app, _dir) = app_avec_comptes(|_| {});

        for essai in 1..=5 {
            let res = post_json(
                &app,
                "/auth/login",
                serde_json::json!({ "login": "remi", "password": "mauvais" }),
                None,
            )
            .await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "essai {essai}");
        }

        // Le mot de passe **correct** non plus n'a plus le droit de passer : le
        // blocage porte sur l'identifiant, pas sur ce qui a été essayé.
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body_text(res).await, "rate_limited");
    }

    /// Sans authentification, le relais n'a rien à demander : il se connecte
    /// directement, comme avant l'existence des comptes.
    #[tokio::test]
    async fn sans_authentification_aucun_ticket_n_est_necessaire() {
        let app = test_app(".");
        let res = post_json(&app, "/auth/ws-ticket", serde_json::json!({}), None).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["ticket"], serde_json::Value::Null);
    }

    /// Le ticket est ce qui permet au relais Nitro (qui ne peut pas joindre le
    /// cookie du navigateur à la connexion amont) d'ouvrir `/ws` et
    /// `/collab/*`. Il doit donc valoir à la place du cookie — et seulement pour
    /// un compte authentifié.
    #[tokio::test]
    async fn le_ticket_ouvre_l_acces_sans_cookie() {
        let (app, _dir) = app_avec_comptes(|_| {});

        // Sans compte, pas de ticket.
        let res = post_json(&app, "/auth/ws-ticket", serde_json::json!({}), None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        let cookie = cookie_de(&res);

        let res = post_json(
            &app,
            "/auth/ws-ticket",
            serde_json::json!({}),
            Some(&cookie),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let ticket = body_json(res).await["ticket"]
            .as_str()
            .expect("un ticket pour une connexion authentifiée")
            .to_string();
        assert_eq!(ticket.len(), 64);

        // Le ticket vaut à la place du cookie…
        let res = get_with(&app, &format!("/models?ticket={ticket}"), None).await;
        assert_eq!(res.status(), StatusCode::OK);

        // …et un ticket inventé ne vaut rien.
        let res = get_with(&app, "/models?ticket=0000", None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Un paramètre vide non plus (il ne doit pas faire passer pour absent).
        let res = get_with(&app, "/models?ticket=", None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// `PUT` JSON, avec un cookie de session en option.
    async fn put_json(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("PUT")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        app.clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }

    /// `DELETE`, avec un cookie de session en option.
    async fn delete_with(app: &Router, uri: &str, cookie: Option<&str>) -> Response {
        let mut request = Request::builder().method("DELETE").uri(uri);
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        app.clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    /// Connexion d'un compte ; renvoie son cookie de session.
    async fn connecte(app: &Router, login: &str, password: &str) -> String {
        let res = post_json(
            app,
            "/auth/login",
            serde_json::json!({ "login": login, "password": password }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "connexion de {login}");
        cookie_de(&res)
    }

    /// UUID d'un rôle, par son nom.
    async fn role_uuid(app: &Router, cookie: &str, name: &str) -> String {
        let v = body_json(get_with(app, "/roles", Some(cookie)).await).await;
        v["roles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|role| role["name"] == name)
            .unwrap_or_else(|| panic!("rôle « {name} » absent"))["uuid"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Crée un compte avec les rôles donnés (par leur UUID).
    async fn cree_compte(
        app: &Router,
        admin: &str,
        username: &str,
        roles: Vec<String>,
    ) -> Response {
        post_json(
            app,
            "/users",
            serde_json::json!({
                "username": username,
                "email": format!("{username}@exemple.fr"),
                "password": "motdepasse",
                "roles": roles,
            }),
            Some(admin),
        )
        .await
    }

    /// L'administrateur a **tous** les droits sans qu'on ait à les lui lister.
    #[tokio::test]
    async fn un_administrateur_a_tous_les_droits() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let cookie = connecte(&app, "remi", "motdepasse").await;

        let v = body_json(get_with(&app, "/auth/me", Some(&cookie)).await).await;
        assert_eq!(v["user"]["roles"][0], "admin");
        assert_eq!(
            v["user"]["permissions"].as_array().unwrap().len(),
            crate::auth::permissions::ALL.len()
        );

        // Le catalogue des droits est servi par le backend : l'interface n'en a
        // aucune liste en dur.
        let v = body_json(get_with(&app, "/permissions", Some(&cookie)).await).await;
        assert!(v.as_array().unwrap().iter().any(|p| p["id"] == "model.delete"));
    }

    /// Le rôle livré `lecteur` : voir le catalogue, rien de plus.
    #[tokio::test]
    async fn un_lecteur_consulte_sans_pouvoir_modifier() {
        let (app, dir) = app_avec_comptes(|_| {});
        std::fs::write(dir.path().join("models/a.txt"), "x").unwrap();

        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        assert_eq!(
            cree_compte(&app, &admin, "lecteur", vec![lecteur]).await.status(),
            StatusCode::CREATED
        );

        let cookie = connecte(&app, "lecteur", "motdepasse").await;

        // Il lit le catalogue et les notes…
        assert_eq!(
            get_with(&app, "/models", Some(&cookie)).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            get_with(&app, "/note?path=a.txt", Some(&cookie)).await.status(),
            StatusCode::OK
        );

        // …mais ne peut ni écrire, ni lire les réglages, ni toucher aux comptes.
        for (method, uri, body) in [
            ("POST", "/delete", serde_json::json!({ "path": "a.txt" })),
            ("POST", "/rename", serde_json::json!({ "path": "a.txt", "name": "b.txt" })),
            ("POST", "/upload?path=b.txt", serde_json::json!({})),
            ("PUT", "/config", serde_json::json!({ "display": { "mode": "3d" } })),
            ("POST", "/ai/search", serde_json::json!({ "query": "x" })),
        ] {
            let res = if method == "PUT" {
                put_json(&app, uri, body, Some(&cookie)).await
            } else {
                post_json(&app, uri, body, Some(&cookie)).await
            };
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{method} {uri}");
            assert_eq!(body_text(res).await, "forbidden");
        }

        for uri in ["/config", "/users", "/roles"] {
            assert_eq!(
                get_with(&app, uri, Some(&cookie)).await.status(),
                StatusCode::FORBIDDEN,
                "{uri}"
            );
        }

        // Le fichier est toujours là : le refus n'a rien modifié au passage.
        assert!(dir.path().join("models/a.txt").exists());
    }

    /// Un droit posé **directement** sur un compte s'ajoute à ses rôles.
    #[tokio::test]
    async fn un_droit_direct_s_ajoute_aux_roles() {
        let (app, dir) = app_avec_comptes(|_| {});
        std::fs::write(dir.path().join("models/a.txt"), "x").unwrap();

        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        assert_eq!(
            cree_compte(&app, &admin, "supprimant", vec![lecteur]).await.status(),
            StatusCode::CREATED
        );

        let v = body_json(get_with(&app, "/users", Some(&admin)).await).await;
        let uuid = v["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|user| user["username"] == "supprimant")
            .unwrap()["uuid"]
            .as_str()
            .unwrap()
            .to_string();

        let cookie = connecte(&app, "supprimant", "motdepasse").await;
        assert_eq!(
            post_json(&app, "/delete", serde_json::json!({ "path": "a.txt" }), Some(&cookie))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );

        // On ajoute le droit seul, sans toucher au rôle.
        assert_eq!(
            put_json(
                &app,
                &format!("/users/{uuid}"),
                serde_json::json!({ "permissions": ["model.delete"] }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );

        assert_eq!(
            post_json(&app, "/delete", serde_json::json!({ "path": "a.txt" }), Some(&cookie))
                .await
                .status(),
            StatusCode::OK
        );
        assert!(!dir.path().join("models/a.txt").exists());
    }

    /// Le dernier administrateur actif ne peut être ni désactivé, ni supprimé,
    /// ni privé de son rôle : sinon plus personne ne peut administrer.
    #[tokio::test]
    async fn le_dernier_administrateur_est_protege() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let uuid = body_json(get_with(&app, "/auth/me", Some(&admin)).await).await["user"]["uuid"]
            .as_str()
            .unwrap()
            .to_string();
        let lecteur = role_uuid(&app, &admin, "lecteur").await;

        for (nom, res) in [
            (
                "désactivation",
                put_json(
                    &app,
                    &format!("/users/{uuid}"),
                    serde_json::json!({ "disabled": true }),
                    Some(&admin),
                )
                .await,
            ),
            (
                "retrait du rôle",
                put_json(
                    &app,
                    &format!("/users/{uuid}"),
                    serde_json::json!({ "roles": [lecteur.clone()] }),
                    Some(&admin),
                )
                .await,
            ),
            ("suppression", delete_with(&app, &format!("/users/{uuid}"), Some(&admin)).await),
        ] {
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{nom}");
        }

        // Il reste administrateur, et toujours connecté.
        assert_eq!(
            get_with(&app, "/users", Some(&admin)).await.status(),
            StatusCode::OK
        );
    }

    /// Se supprimer soi-même est refusé : c'est le clic qu'on regrette.
    #[tokio::test]
    async fn on_ne_se_supprime_pas_soi_meme() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        // Ce compte a de quoi supprimer des comptes (sinon la couche de droit
        // l'arrête avant d'arriver au handler, et le test ne prouverait rien).
        let res = post_json(
            &app,
            "/users",
            serde_json::json!({
                "username": "second",
                "email": "second@exemple.fr",
                "password": "motdepasse",
                "permissions": ["users.write"],
            }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let second = connecte(&app, "second", "motdepasse").await;

        let uuid = body_json(get_with(&app, "/auth/me", Some(&second)).await).await["user"]["uuid"]
            .as_str()
            .unwrap()
            .to_string();
        let res = delete_with(&app, &format!("/users/{uuid}"), Some(&second)).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "cannot_delete_self");
    }

    /// Les rôles livrés sont figés — mais on les clone pour partir de quelque
    /// chose.
    #[tokio::test]
    async fn les_roles_livres_sont_figes_mais_clonables() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;

        for res in [
            put_json(
                &app,
                &format!("/roles/{lecteur}"),
                serde_json::json!({ "name": "autre" }),
                Some(&admin),
            )
            .await,
            delete_with(&app, &format!("/roles/{lecteur}"), Some(&admin)).await,
        ] {
            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
            assert_eq!(body_text(res).await, "role_frozen");
        }

        // Clonage : le clone reprend les droits, puis vit sa vie.
        let res = post_json(
            &app,
            "/roles",
            serde_json::json!({ "name": "imprimeur", "from": lecteur, "permissions": ["catalog.read", "model.upload"] }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let clone = body_json(res).await["uuid"].as_str().unwrap().to_string();

        let v = body_json(get_with(&app, "/roles", Some(&admin)).await).await;
        let imprimeur = v["roles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|role| role["name"] == "imprimeur")
            .unwrap()
            .clone();
        assert_eq!(imprimeur["builtin"], false);
        assert_eq!(imprimeur["permissions"], serde_json::json!(["catalog.read", "model.upload"]));

        assert_eq!(
            put_json(
                &app,
                &format!("/roles/{clone}"),
                serde_json::json!({ "name": "imprimeur 3D" }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            delete_with(&app, &format!("/roles/{clone}"), Some(&admin)).await.status(),
            StatusCode::OK
        );
    }

    /// Un droit ou un rôle inconnu fait **échouer** l'écriture : accorder un
    /// droit que le serveur n'appliquera pas serait bien pire qu'un refus.
    #[tokio::test]
    async fn un_droit_ou_un_role_inconnu_est_refuse() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        let res = post_json(
            &app,
            "/roles",
            serde_json::json!({ "name": "bizarre", "permissions": ["model.purge"] }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "unknown_permission");

        let res = cree_compte(
            &app,
            &admin,
            "fantome",
            vec!["0199e3d0-0000-7000-8000-00000000ffff".to_string()],
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "unknown_role");

        // Un nom déjà pris est un **conflit**, pas une panne — et le code dit
        // lequel des deux, parce que l'interface n'affiche pas le même message.
        assert_eq!(
            cree_compte(&app, &admin, "doublon", vec![]).await.status(),
            StatusCode::CREATED
        );
        let res = post_json(
            &app,
            "/users",
            serde_json::json!({ "username": "doublon", "email": "autre@exemple.fr", "password": "motdepasse" }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(body_text(res).await, "username_taken");

        let res = post_json(
            &app,
            "/users",
            serde_json::json!({ "username": "autre", "email": "doublon@exemple.fr", "password": "motdepasse" }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(body_text(res).await, "email_taken");
    }

    /// Le rôle par défaut s'applique aux comptes créés sans rôle explicite.
    #[tokio::test]
    async fn le_role_par_defaut_s_applique_aux_nouveaux_comptes() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;

        assert_eq!(
            put_json(
                &app,
                "/roles/default",
                serde_json::json!({ "uuid": lecteur }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );

        assert_eq!(cree_compte(&app, &admin, "nouveau", vec![]).await.status(), StatusCode::CREATED);
        let cookie = connecte(&app, "nouveau", "motdepasse").await;
        let v = body_json(get_with(&app, "/auth/me", Some(&cookie)).await).await;
        assert_eq!(v["user"]["roles"][0], "lecteur");
        assert_eq!(get_with(&app, "/models", Some(&cookie)).await.status(), StatusCode::OK);
    }

    /// Un compte désactivé perd l'accès **immédiatement**, sans attendre que sa
    /// session expire.
    #[tokio::test]
    async fn desactiver_un_compte_coupe_l_acces_tout_de_suite() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        cree_compte(&app, &admin, "temporaire", vec![lecteur]).await;

        let cookie = connecte(&app, "temporaire", "motdepasse").await;
        assert_eq!(
            get_with(&app, "/models", Some(&cookie)).await.status(),
            StatusCode::OK
        );

        let uuid = body_json(get_with(&app, "/users", Some(&admin)).await).await["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|user| user["username"] == "temporaire")
            .unwrap()["uuid"]
            .as_str()
            .unwrap()
            .to_string();

        assert_eq!(
            put_json(
                &app,
                &format!("/users/{uuid}"),
                serde_json::json!({ "disabled": true }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );

        assert_eq!(
            get_with(&app, "/models", Some(&cookie)).await.status(),
            StatusCode::UNAUTHORIZED
        );
        // Et il ne peut plus se reconnecter.
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "temporaire", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// Chacun change son propre mot de passe ; les anciennes sessions tombent.
    #[tokio::test]
    async fn chacun_change_son_propre_mot_de_passe() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        // Mauvais mot de passe actuel.
        let res = post_json(
            &app,
            "/auth/password",
            serde_json::json!({ "current": "faux", "new": "nouveau-mot-de-passe" }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // Trop court.
        let res = post_json(
            &app,
            "/auth/password",
            serde_json::json!({ "current": "motdepasse", "new": "court" }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "password_too_short");

        // Accepté : la session est renouvelée (nouveau cookie) et reste valable.
        let res = post_json(
            &app,
            "/auth/password",
            serde_json::json!({ "current": "motdepasse", "new": "nouveau-mot-de-passe" }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = cookie_de(&res);
        assert_ne!(cookie, admin, "la session doit être renouvelée");
        assert_eq!(
            get_with(&app, "/models", Some(&cookie)).await.status(),
            StatusCode::OK
        );

        // L'ancien mot de passe ne vaut plus rien, le nouveau ouvre une session.
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "nouveau-mot-de-passe" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// Sans authentification, le catalogue des droits et les comptes n'existent
    /// pas : le reste du serveur se comporte comme avant.
    #[tokio::test]
    async fn sans_authentification_les_routes_de_comptes_restent_ouvertes() {
        let app = test_app(".");
        assert_eq!(
            get_with(&app, "/users", None).await.status(),
            StatusCode::OK,
            "sans comptes, il n'y a rien à cacher"
        );
        assert_eq!(
            get_with(&app, "/permissions", None).await.status(),
            StatusCode::OK
        );
    }

    /// La déconnexion ferme la session **et** les tickets d'accès déjà émis
    /// pour le compte : rien ne doit rester ouvert derrière soi.
    #[tokio::test]
    async fn la_deconnexion_ferme_la_session_et_ses_tickets() {
        let (app, _dir) = app_avec_comptes(|_| {});

        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "remi", "password": "motdepasse" }),
            None,
        )
        .await;
        let cookie = cookie_de(&res);
        let res = post_json(
            &app,
            "/auth/ws-ticket",
            serde_json::json!({}),
            Some(&cookie),
        )
        .await;
        let ticket = body_json(res).await["ticket"].as_str().unwrap().to_string();

        // Le compte existe toujours, mais la déconnexion a écarté ses tickets.
        post_json(&app, "/auth/logout", serde_json::json!({}), Some(&cookie)).await;
        let res = get_with(&app, &format!("/models?ticket={ticket}"), None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // ── Jetons d'API (agents) ───────────────────────────────────────────────

    /// Requête authentifiée par un **jeton** plutôt que par un cookie.
    async fn bearer(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        token: &str,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"));
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        app.clone()
            .oneshot(
                request
                    .body(
                        body.map(|value| Body::from(value.to_string()))
                            .unwrap_or_else(Body::empty),
                    )
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Crée un jeton pour un compte et renvoie sa valeur en clair.
    async fn cree_jeton(app: &Router, cookie: &str, name: &str) -> String {
        let res = post_json(
            app,
            "/tokens",
            serde_json::json!({ "name": name }),
            Some(cookie),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        body_json(res).await["token"].as_str().unwrap().to_string()
    }

    /// Un jeton remplace le cookie : c'est ce qui ouvre le serveur aux agents,
    /// qui n'ont pas de navigateur.
    #[tokio::test]
    async fn un_jeton_ouvre_l_acces_sans_cookie() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let cookie = connecte(&app, "remi", "motdepasse").await;
        let token = cree_jeton(&app, &cookie, "portable").await;
        assert!(token.starts_with("e3d_"), "jeton : {token}");

        // Le jeton vaut à la place du cookie…
        assert_eq!(
            bearer(&app, "GET", "/models", None, &token).await.status(),
            StatusCode::OK
        );

        // …mais il n'est **jamais** réaffiché : la liste ne donne que son nom.
        let v = body_json(get_with(&app, "/tokens", Some(&cookie)).await).await;
        assert_eq!(v[0]["name"], "portable");
        assert!(v[0].get("token").is_none(), "json : {}", v[0]);
        assert!(
            v[0]["last_used_at"].is_number(),
            "l'usage qui vient d'avoir lieu doit être noté : {}",
            v[0]
        );

        // Révocation : le jeton ne vaut plus rien.
        let uuid = v[0]["uuid"].as_str().unwrap().to_string();
        assert_eq!(
            delete_with(&app, &format!("/tokens/{uuid}"), Some(&cookie))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            bearer(&app, "GET", "/models", None, &token).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    /// Un jeton **hérite** des droits du compte : il n'en donne jamais plus.
    #[tokio::test]
    async fn un_jeton_ne_donne_pas_plus_de_droits_que_le_compte() {
        let (app, dir) = app_avec_comptes(|_| {});
        std::fs::write(dir.path().join("models/a.txt"), "x").unwrap();

        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        cree_compte(&app, &admin, "agent", vec![lecteur]).await;

        let cookie = connecte(&app, "agent", "motdepasse").await;
        let token = cree_jeton(&app, &cookie, "ci").await;

        assert_eq!(
            bearer(&app, "GET", "/models", None, &token).await.status(),
            StatusCode::OK
        );
        // Ce que le compte ne peut pas faire, son jeton ne le peut pas non plus.
        let res = bearer(
            &app,
            "POST",
            "/delete",
            Some(serde_json::json!({ "path": "a.txt" })),
            &token,
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(dir.path().join("models/a.txt").exists());

        // Un jeton inventé non plus.
        assert_eq!(
            bearer(&app, "GET", "/models", None, "e3d_0000").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    /// Les jetons sont **personnels** : personne ne voit ni ne révoque ceux des
    /// autres — pas même un administrateur (il désactive le compte, ce qui les
    /// neutralise tous d'un coup).
    #[tokio::test]
    async fn les_jetons_sont_personnels() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        cree_compte(&app, &admin, "bob", vec![lecteur]).await;
        let bob = connecte(&app, "bob", "motdepasse").await;

        let res = post_json(
            &app,
            "/tokens",
            serde_json::json!({ "name": "de bob" }),
            Some(&bob),
        )
        .await;
        let uuid = body_json(res).await["uuid"].as_str().unwrap().to_string();

        let v = body_json(get_with(&app, "/tokens", Some(&admin)).await).await;
        assert!(v.as_array().unwrap().is_empty(), "jetons de l'admin : {v}");
        assert_eq!(
            delete_with(&app, &format!("/tokens/{uuid}"), Some(&admin))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    /// Désactiver un compte arrête aussi ses agents, sans qu'on ait à retrouver
    /// chacun de ses jetons.
    #[tokio::test]
    async fn desactiver_un_compte_arrete_ses_jetons() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        cree_compte(&app, &admin, "agent", vec![lecteur]).await;

        let cookie = connecte(&app, "agent", "motdepasse").await;
        let token = cree_jeton(&app, &cookie, "ci").await;
        assert_eq!(
            bearer(&app, "GET", "/models", None, &token).await.status(),
            StatusCode::OK
        );

        let uuid = body_json(get_with(&app, "/auth/me", Some(&cookie)).await).await["user"]["uuid"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            put_json(
                &app,
                &format!("/users/{uuid}"),
                serde_json::json!({ "disabled": true }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );

        assert_eq!(
            bearer(&app, "GET", "/models", None, &token).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    // ── SSO (OIDC) ──────────────────────────────────────────────────────────

    /// Application avec comptes actifs, administrateur créé, et une
    /// configuration SSO donnée (les réglages d'environnement sont injectés par
    /// l'état, comme en production).
    fn app_sso(config: Config, env: auth::oidc::Env) -> (Router, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        let comptes = auth::Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
        let seed = auth::AdminSeed {
            username: "remi".into(),
            email: "remi@exemple.fr".into(),
            password: "motdepasse".into(),
        };
        auth::bootstrap_admin(&comptes, Some(seed)).unwrap();
        let (ws, _) = broadcast::channel::<String>(16);
        let app = routes(
            AppState::new(models, ws, config)
                .with_auth(comptes)
                .with_oidc(env),
        );
        (app, dir)
    }

    /// Configuration SSO complète (émetteur, client, secret).
    fn config_sso() -> Config {
        Config {
            oidc: config::Oidc {
                enabled: true,
                issuer: Some("https://sso.exemple.fr/realms/moi".into()),
                client_id: Some("easy3d".into()),
                client_secret: Some("secret".into()),
                ..config::Oidc::default()
            },
            ..Config::default()
        }
    }

    #[tokio::test]
    async fn le_sso_est_une_route_publique_qui_dit_quand_il_est_eteint() {
        let (app, _dir) = app_avec_comptes(|_| {});

        // Deux routes **publiques** : on n'est pas encore authentifié quand on
        // les appelle. Sans SSO configuré, le départ refuse clairement…
        let res = get_with(&app, "/auth/oidc/start", None).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "oidc_disabled");

        // …et le retour dépose l'utilisateur sur la page de connexion, avec un
        // code traduisible, plutôt que sur une page blanche.
        let res = get_with(&app, "/auth/oidc/callback?code=x&state=y", None).await;
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(
            res.headers().get("location").unwrap(),
            "/login?error=invalid_state"
        );

        // Même éteint, `/auth/me` reste une réponse normale : l'interface ne
        // montre simplement pas de bouton.
        let v = body_json(get_with(&app, "/auth/me", None).await).await;
        assert!(v["oidc"].is_null());
    }

    #[tokio::test]
    async fn auth_me_annonce_le_fournisseur_quand_le_sso_est_allume() {
        let (app, _dir) = app_sso(config_sso(), auth::oidc::Env::empty());

        // Le bouton de la page de connexion n'a besoin que du nom du
        // fournisseur — et le secret client ne sort **jamais**.
        let v = body_json(get_with(&app, "/auth/me", None).await).await;
        assert_eq!(v["oidc"]["label"], "sso.exemple.fr");
        assert!(v["oidc"].get("client_secret").is_none(), "{v}");

        // La configuration exposée masque le secret, comme la clé d'API.
        let cookie = connecte(&app, "remi", "motdepasse").await;
        let v = body_json(get_with(&app, "/config", Some(&cookie)).await).await;
        assert_eq!(v["config"]["oidc"]["client_secret"], config::KEY_PLACEHOLDER);
        assert_eq!(v["oidc"]["active"], true);
        assert_eq!(v["oidc"]["locked"].as_array().unwrap().len(), 0);
        assert!(v["oidc"]["problem"].is_null(), "{v}");
    }

    #[tokio::test]
    async fn un_sso_a_moitie_configure_le_dit_au_lieu_d_echouer_en_silence() {
        let config = Config {
            oidc: config::Oidc {
                enabled: true,
                issuer: Some("https://sso.exemple.fr".into()),
                ..config::Oidc::default()
            },
            ..Config::default()
        };
        let (app, _dir) = app_sso(config, auth::oidc::Env::empty());

        // L'interface peut dire *ce qui manque* au lieu d'afficher un
        // interrupteur sans effet.
        let cookie = connecte(&app, "remi", "motdepasse").await;
        let v = body_json(get_with(&app, "/config", Some(&cookie)).await).await;
        assert_eq!(v["oidc"]["active"], false);
        assert_eq!(v["oidc"]["problem"], "client_id", "{v}");
        // Et aucun bouton n'est proposé sur la page de connexion.
        let v = body_json(get_with(&app, "/auth/me", None).await).await;
        assert!(v["oidc"].is_null());
    }

    #[tokio::test]
    async fn les_reglages_sso_de_l_environnement_sont_annonces_comme_figes() {
        let (app, _dir) = app_sso(
            Config::default(),
            auth::oidc::Env {
                issuer: Some("https://env.exemple.fr".into()),
                client_id: Some("easy3d".into()),
                client_secret: Some("secret".into()),
                ..auth::oidc::Env::empty()
            },
        );

        // L'environnement allume le SSO même si le fichier ne dit rien : les
        // champs correspondants sont annoncés comme figés, donc l'interface les
        // grise au lieu de laisser croire qu'on peut les régler ici.
        let cookie = connecte(&app, "remi", "motdepasse").await;
        let v = body_json(get_with(&app, "/config", Some(&cookie)).await).await;
        assert_eq!(v["oidc"]["active"], true);
        assert!(v["oidc"]["problem"].is_null(), "{v}");
        let locked: Vec<&str> = v["oidc"]["locked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert!(locked.contains(&"enabled"), "{locked:?}");
        assert!(locked.contains(&"issuer"), "{locked:?}");
        assert!(locked.contains(&"client_secret"), "{locked:?}");
        assert!(!locked.contains(&"provisioning"), "{locked:?}");

        let v = body_json(get_with(&app, "/auth/me", None).await).await;
        assert_eq!(v["oidc"]["label"], "env.exemple.fr");
    }

    // ── Journal d'audit ─────────────────────────────────────────────────────

    /// Événements du journal, du plus récent au plus ancien.
    async fn journal(app: &Router, cookie: &str) -> Vec<serde_json::Value> {
        body_json(get_with(app, "/journal", Some(cookie)).await)
            .await
            .as_array()
            .unwrap()
            .clone()
    }

    /// Le premier événement de ce type, ou une panne explicite.
    fn evenement<'a>(events: &'a [serde_json::Value], kind: &str) -> &'a serde_json::Value {
        events
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap_or_else(|| panic!("aucun événement « {kind} » dans {events:#?}"))
    }

    /// L'identifiant d'un compte, par son nom (`/users`).
    async fn compte_uuid(app: &Router, cookie: &str, username: &str) -> String {
        let v = body_json(get_with(app, "/users", Some(cookie)).await).await;
        v["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|user| user["username"] == username)
            .unwrap_or_else(|| panic!("compte « {username} » absent"))["uuid"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn le_journal_retient_les_connexions_et_les_refus() {
        let (app, _dir) = app_avec_comptes(|_| {});

        // Refus : l'identifiant **essayé** est noté, même s'il n'existe pas — et
        // l'événement n'a pas d'acteur, puisque personne n'est entré.
        let res = post_json(
            &app,
            "/auth/login",
            serde_json::json!({ "login": "inconnu", "password": "peu-importe" }),
            None,
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let admin = connecte(&app, "remi", "motdepasse").await;
        let events = journal(&app, &admin).await;

        let refus = evenement(&events, "login_failed");
        assert_eq!(refus["subject"], "inconnu");
        assert_eq!(refus["detail"], "invalid_credentials");
        assert!(refus["actor"].is_null(), "{refus}");

        let ok = evenement(&events, "login_ok");
        assert_eq!(ok["actor"], "remi");
        assert_eq!(ok["subject"], "remi");
        assert_eq!(ok["detail"], "password");

        // Déconnexion : elle laisse sa trace, puis on rouvre une session pour
        // pouvoir relire le journal.
        assert_eq!(
            post_json(&app, "/auth/logout", serde_json::json!({}), Some(&admin))
                .await
                .status(),
            StatusCode::OK
        );
        let admin = connecte(&app, "remi", "motdepasse").await;
        assert!(evenement(&journal(&app, &admin).await, "logout")["actor"] == "remi");
    }

    #[tokio::test]
    async fn le_journal_demande_le_droit_de_voir_les_comptes() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;
        let lecteur = role_uuid(&app, &admin, "lecteur").await;
        assert_eq!(
            cree_compte(&app, &admin, "lecteur", vec![lecteur]).await.status(),
            StatusCode::CREATED
        );

        // Un lecteur du catalogue n'a rien à faire dans un journal de sécurité.
        let cookie = connecte(&app, "lecteur", "motdepasse").await;
        let res = get_with(&app, "/journal", Some(&cookie)).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(body_text(res).await, "forbidden");

        // Et sans session, rien du tout.
        assert_eq!(
            get_with(&app, "/journal", None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn le_journal_retient_les_jetons_et_les_changements_de_droits() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        assert_eq!(
            post_json(
                &app,
                "/roles",
                serde_json::json!({ "name": "imprimeur", "permissions": ["catalog.read"] }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::CREATED
        );
        let role = role_uuid(&app, &admin, "imprimeur").await;
        assert_eq!(
            cree_compte(&app, &admin, "imprimeur", vec![role.clone()]).await.status(),
            StatusCode::CREATED
        );
        let compte = compte_uuid(&app, &admin, "imprimeur").await;
        assert_eq!(
            put_json(
                &app,
                &format!("/users/{compte}"),
                serde_json::json!({ "disabled": true }),
                Some(&admin),
            )
            .await
            .status(),
            StatusCode::OK
        );

        let jeton = cree_jeton(&app, &admin, "ci").await;
        let uuid = body_json(get_with(&app, "/tokens", Some(&admin)).await).await[0]["uuid"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            delete_with(&app, &format!("/tokens/{uuid}"), Some(&admin))
                .await
                .status(),
            StatusCode::OK
        );

        let events = journal(&app, &admin).await;
        assert_eq!(evenement(&events, "role_created")["subject"], "imprimeur");
        assert_eq!(evenement(&events, "account_created")["subject"], "imprimeur");

        // Ce qui a changé est écrit : « compte modifié » tout court
        // n'apprendrait rien.
        let maj = evenement(&events, "account_updated");
        assert_eq!(maj["subject"], "imprimeur");
        assert_eq!(maj["detail"], "disabled");

        assert_eq!(evenement(&events, "token_created")["subject"], "ci");
        assert_eq!(
            evenement(&events, "token_revoked")["subject"].as_str(),
            Some(uuid.as_str())
        );

        // ⚠️ Le contrôle qui compte : **aucun** événement ne contient la valeur
        // d'un jeton. Le journal est un témoin, pas un second endroit où un
        // secret dort.
        let brut = serde_json::to_string(&events).unwrap();
        assert!(!brut.contains("e3d_"), "{brut}");
        let _ = jeton;
    }

    #[tokio::test]
    async fn un_nom_de_jeton_est_borne_a_quatre_vingts_caracteres() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        let res = post_json(
            &app,
            "/tokens",
            serde_json::json!({ "name": "a".repeat(81) }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(body_text(res).await, "name_too_long");

        // 80 exactement : accepté (la borne est inclusive), et compté en
        // **caractères** — 80 accents passent aussi.
        for nom in ["a".repeat(80), "é".repeat(80)] {
            let res = post_json(
                &app,
                "/tokens",
                serde_json::json!({ "name": nom }),
                Some(&admin),
            )
            .await;
            assert_eq!(res.status(), StatusCode::CREATED, "nom de 80 caractères");
        }
    }

    #[tokio::test]
    async fn un_jeton_peut_avoir_une_duree_de_vie() {
        let (app, _dir) = app_avec_comptes(|_| {});
        let admin = connecte(&app, "remi", "motdepasse").await;

        let res = post_json(
            &app,
            "/tokens",
            serde_json::json!({ "name": "ci", "expires_in_days": 30 }),
            Some(&admin),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let v = body_json(res).await;
        let fin = v["expires_at"].as_i64().expect("date d'expiration");
        let attendu = auth::db::now() + 30 * 86_400;
        assert!(
            (fin - attendu).abs() < 60,
            "expiration à {fin}, attendue vers {attendu}"
        );

        // Sans durée (ou à zéro) : pas de date, donc pas d'expiration.
        for corps in [
            serde_json::json!({ "name": "eternel" }),
            serde_json::json!({ "name": "eternel-zero", "expires_in_days": 0 }),
        ] {
            let v = body_json(post_json(&app, "/tokens", corps, Some(&admin)).await).await;
            assert!(v["expires_at"].is_null(), "{v}");
        }

        // Le journal note la durée **demandée** (0 = sans expiration) : on
        // cherche l'événement du jeton de 30 jours, pas le dernier écrit.
        let events = journal(&app, &admin).await;
        let creation = events
            .iter()
            .find(|e| e["kind"] == "token_created" && e["subject"] == "ci")
            .unwrap_or_else(|| panic!("création du jeton « ci » absente : {events:#?}"));
        assert_eq!(creation["detail"], "30");
    }
}
