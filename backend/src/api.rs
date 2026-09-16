use crate::collab;
use crate::config::Config;
use crate::formats;
use crate::notes;
use crate::scanner::{self, FileInfo, FolderInfo};
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
        .route("/upload", post(post_upload).layer(DefaultBodyLimit::max(UPLOAD_MAX_BYTES)))
        .route("/note", get(get_note))
        .route("/health", get(health))
        .route("/ws", get(ws_models))
        .route("/collab/{*rel}", get(ws_collab))
        .nest_service("/mcp", mcp)
        .with_state(state)
}

/// Point de santé : renvoie `ok` (200).
async fn health() -> &'static str {
    "ok"
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
            config: config.clone(),
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
}

impl ConfigResponse {
    fn of(config: Config) -> Self {
        // Canonicalisé quand c'est possible : le chemin affiché dans
        // l'interface ne doit pas contenir de `..` (ex : `backend/../models`).
        let path = config.resolve_models_root();
        let models_root = std::fs::canonicalize(&path).unwrap_or(path);

        Self {
            config,
            models_root: tidy_path(&models_root),
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
    if q.path.contains('\\')
        || q.path.ends_with('/')
        || q.path.split('/').any(|part| part.is_empty() || part.starts_with('.'))
    {
        return Err(refuse(format!("chemin réservé : « {} »", q.path)));
    }
    if body.is_empty() {
        return Err(refuse(format!("fichier vide : « {} »", q.path)));
    }
    if relative.is_dir() {
        return Err(refuse(format!("un dossier porte déjà ce nom : « {} »", q.path)));
    }

    // Les dossiers manquants sont créés : un envoi de dossier arrive fichier par
    // fichier, dans un ordre qui n'est pas garanti.
    if let Some(dir) = relative.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(internal_error)?;
    }

    let tmp = temp_path(&relative);
    tokio::fs::write(&tmp, &body).await.map_err(internal_error)?;
    // Sous Windows, `rename` refuse d'écraser : on efface la destination d'abord.
    // Le contenu complet est déjà dans le temporaire, la fenêtre est minuscule.
    if relative.exists() {
        tokio::fs::remove_file(&relative).await.map_err(internal_error)?;
    }
    if let Err(e) = tokio::fs::rename(&tmp, &relative).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(internal_error(e));
    }

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
pub fn broadcast_snapshot(state: &AppState) {
    let payload = ModelsResponse::from_scan(scanner::scan_models(&state.root()), &state.config());
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = state.ws.send(json);
    }
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
            "..%2Fsecret.stl",      // hors de la racine
            "%2Fabsolu.stl",        // chemin absolu
            "a%5Cb.stl",            // contre-oblique (séparateur Windows)
            "sous%2F",              // se termine par un séparateur
            ".easy3d-thumbs%2Fx.png", // dossier interne du backend
            "DemaAuto",             // un dossier porte déjà ce nom
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
            assert_eq!(res.status(), StatusCode::BAD_REQUEST, "devrait refuser {rel}");
        }
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
