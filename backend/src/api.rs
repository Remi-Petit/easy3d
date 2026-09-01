use crate::config::Config;
use crate::scanner::{self, FileInfo, FolderInfo};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use tokio::sync::broadcast;

/// État partagé par les handlers HTTP et WebSocket.
#[derive(Clone)]
pub struct AppState {
    pub root: String,
    /// Canal broadcast : diffuse la liste des modèles (JSON) à tous les WS.
    pub ws: broadcast::Sender<String>,
    pub config: Config,
}

/// Démarre le serveur HTTP et bloque jusqu'à son arrêt.
pub async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/models", get(list_models))
        .route("/file", get(get_file))
        .route("/health", get(health))
        .route("/ws", get(ws_models))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8090".to_string());
    let addr = format!("127.0.0.1:{port}").parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("API HTTP : http://{addr}/models");
    axum::serve(listener, app).await?;
    Ok(())
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
            config: config.clone(),
        }
    }
}

/// Renvoie le contenu structuré : fichiers racine + dossiers + total.
async fn list_models(State(state): State<AppState>) -> Json<ModelsResponse> {
    Json(ModelsResponse::from_scan(
        scanner::scan_models(Path::new(&state.root)),
        &state.config,
    ))
}

/// WebSocket : connexion persistante qui pousse la liste des modèles à chaque
/// changement détecté par le watcher (via le canal broadcast).
async fn ws_models(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Snapshot complet de la liste des modèles, sérialisé en JSON.
fn snapshot(root: &Path, config: &Config) -> String {
    serde_json::to_string(&ModelsResponse::from_scan(
        scanner::scan_models(root),
        config,
    ))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Boucle d'une connexion WS : snapshot initial, puis diffusion en continu.
///
/// Si le client est en retard (`Lagged`), on renvoie un snapshot complet plutôt
/// que des MAJ partielles perdues. Fermeture / erreur → on sort de la boucle.
async fn handle_ws(socket: WebSocket, state: AppState) {
    let root = PathBuf::from(&state.root);
    let (mut sink, mut stream) = socket.split();

    if sink
        .send(Message::text(snapshot(&root, &state.config)))
        .await
        .is_err()
    {
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
                        snapshot(&root, &state.config)
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

/// Query param de `/file` : chemin relatif au dossier des modèles.
#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

/// Sert le contenu binaire d'un fichier du répertoire modèles.
///
/// `path` est relatif à `state.root` (ex : `DemaAuto/boitier.stl`).
/// Sécurisé contre la traversée de dossier (`..`, absolu).
async fn get_file(State(state): State<AppState>, Query(q): Query<FileQuery>) -> Response {
    let Some(relative) = safe_join(Path::new(&state.root), &q.path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    match tokio::fs::read(&relative).await {
        Ok(bytes) => {
            let ct = content_type(&relative);
            ([(header::CONTENT_TYPE, ct)], bytes).into_response()
        }
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

/// Type MIME approximatif selon l'extension.
fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("stl") => "model/stl",
        Some("obj") => "model/obj",
        Some("3mf") => "model/3mf",
        Some("gcode") | Some("gco") => "text/plain",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
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
        Router::new()
            .route("/models", get(list_models))
            .route("/file", get(get_file))
            .route("/health", get(health))
            .route("/ws", get(ws_models))
            .with_state(AppState {
                root: root.to_string(),
                ws,
                config: crate::config::Config::default(),
            })
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
    }

    #[tokio::test]
    async fn route_inconnue_renvoie_404() {
        let app = test_app(".");
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
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
        let ct = res
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "model/stl");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"solid x");
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

    /// Intégration WebSocket : connexion réelle → snapshot initial, puis
    /// réception d'une MAJ diffusée sur le canal broadcast.
    #[tokio::test]
    async fn ws_pousse_snapshot_puis_diffusion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b.txt"), "y").unwrap();

        let (ws_tx, _) = broadcast::channel::<String>(16);
        let state = AppState {
            root: dir.path().to_string_lossy().into_owned(),
            ws: ws_tx.clone(),
            config: crate::config::Config::default(),
        };
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
}

