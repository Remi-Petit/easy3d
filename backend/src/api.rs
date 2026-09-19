use crate::ai;
use crate::collab;
use crate::config::{self, Config};
use crate::formats;
use crate::notes;
use crate::scanner::{self, FileInfo, FolderInfo};
use crate::thumbnail;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

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
    /// Dernière liste diffusée aux clients WS (JSON).
    ///
    /// Référence du **re-scan périodique** ([`refresh_if_changed`]) : sans elle,
    /// chaque tic rediffuserait un contenu identique et le frontend, qui
    /// remplace son état à chaque message, se re-rendrait pour rien.
    pub last_snapshot: Arc<RwLock<String>>,
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
            last_snapshot: Arc::new(RwLock::new(String::new())),
        }
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
    // Serveur MCP monté sur `/mcp` (transport Streamable HTTP, voir `crate::mcp`).
    // Il partage l'état : les outils lisent le catalogue courant et écrivent la
    // configuration et les notes par les mêmes chemins que l'interface.
    let mcp: StreamableHttpService<crate::mcp::Easy3dMcp, LocalSessionManager> =
        StreamableHttpService::new(
            {
                let state = state.clone();
                move || Ok(crate::mcp::Easy3dMcp::new(state.clone()))
            },
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );

    Router::new()
        .route("/models", get(list_models))
        .route("/config", get(get_config).put(put_config))
        .route("/file", get(get_file))
        .route(
            "/upload",
            post(post_upload).layer(DefaultBodyLimit::max(UPLOAD_MAX_BYTES)),
        )
        .route("/rename", post(post_rename))
        .route("/delete", post(post_delete))
        .route("/note", get(get_note))
        .route("/health", get(health))
        .route("/ai/providers", get(ai_providers))
        .route("/ai/search", post(ai_search))
        .route("/ai/test", post(ai_test))
        .route("/ws", get(ws_models))
        .route("/collab/{*rel}", get(ws_collab))
        .nest_service("/mcp", mcp)
        .with_state(state)
}

/// Point de santé : renvoie `ok` (200).
async fn health() -> &'static str {
    "ok"
}

/// Fournisseurs de modèles connus, pour l'écran d'administration.
///
/// Les identifiants, libellés et valeurs par défaut viennent du backend : le
/// frontend n'a aucune liste de fournisseurs codée en dur.
async fn ai_providers() -> Json<Vec<ai::ProviderInfo>> {
    Json(ai::describe())
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

/// Réponse du test de configuration.
#[derive(Serialize)]
pub struct AiTestResponse {
    /// Ce que le modèle a répondu (en principe « OK »).
    pub message: String,
}

/// Vérifie que le fournisseur configuré répond (bouton « Tester »).
async fn ai_test(
    State(state): State<AppState>,
) -> Result<Json<AiTestResponse>, (StatusCode, String)> {
    ai::test(&state)
        .await
        .map(|message| Json(AiTestResponse { message }))
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
    /// Configuration applicative, exposée au frontend.
    pub config: Config,
}

impl ModelsResponse {
    /// Construit une réponse à partir d'un scan.
    pub fn from_scan(scan: scanner::ModelsScan, config: &Config) -> Self {
        let count = scan.total();
        ModelsResponse {
            folders: scan.folders,
            files: scan.files,
            count,
            formats: formats::describe(),
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
    fn of(config: Config) -> Self {
        // Canonicalisé quand c'est possible : le chemin affiché dans
        // l'interface ne doit pas contenir de `..` (ex : `backend/../models`).
        let path = config.resolve_models_root();
        let models_root = std::fs::canonicalize(&path).unwrap_or(path);

        // Recommandation pour **ce** dossier : c'est le système de fichiers qui
        // décide, pas l'OS. Dans un conteneur Docker Desktop, le backend tourne
        // sous Linux alors que le dossier vient de Windows.
        let (recommended, filesystem) = config::watch_poll_recommendation(&models_root);
        let effective =
            config::watch_poll_interval(&config, &models_root).map_or(0, |d| d.as_secs());

        Self {
            // Configuration telle qu'elle peut être montrée : la clé d'API y est
            // remplacée par un marqueur (voir [`config::KEY_PLACEHOLDER`]).
            config: config.redacted(),
            models_root: tidy_path(&models_root),
            watch: WatchInfo {
                recommended,
                effective,
                filesystem,
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
    Json(ConfigResponse::of(state.config()))
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
    config.ai.api_key = state.config().ai.merge_key(config.ai.api_key.as_deref());
    // Pas de clé orpheline : sans fournisseur, elle n'aurait aucun usage.
    if !config.ai.is_configured() {
        config.ai.api_key = None;
    }

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
        return Ok(ConfigResponse::of(config));
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

    Ok(ConfigResponse::of(config))
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

    /// Pas de clé orpheline : sans fournisseur, elle n'a aucune raison d'être
    /// conservée dans le fichier.
    #[tokio::test]
    async fn changer_de_fournisseur_sans_cle_efface_la_cle() {
        let dir = tempfile::tempdir().unwrap();
        let state = ai_state(dir.path(), "sk-secret");
        let app = routes(state.clone());

        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/config")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "models_root": ".",
                            "ai": { "provider": null, "api_key": "sk-orph"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert!(state.config().ai.api_key.is_none());
        assert!(!state.config().ai.is_configured());
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

    /// Le fournisseur est configuré mais ne répond pas : l'erreur d'appel
    /// remonte telle quelle (ici, un port fermé).
    #[tokio::test]
    async fn le_test_de_configuration_remonte_l_erreur_du_fournisseur() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.ai = config::Ai {
            provider: Some("openai".to_string()),
            base_url: Some("http://127.0.0.1:9/v1".to_string()),
            api_key: Some("sk-secret".to_string()),
            ..Default::default()
        };
        let (ws, _) = broadcast::channel::<String>(16);
        let app = routes(AppState::new(".", ws, config).with_config_path(dir.path().join("c.yml")));

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ai/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let message = String::from_utf8_lossy(&body);
        assert!(message.contains("impossible"), "{message}");
        assert!(message.contains("127.0.0.1:9"), "{message}");
        // La clé ne doit jamais apparaître dans un message d'erreur.
        assert!(!message.contains("sk-secret"), "{message}");
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
}
