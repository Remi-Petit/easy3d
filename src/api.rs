use crate::scanner::{self, FileInfo};
use axum::{extract::State, routing::get, Json, Router};
use std::net::SocketAddr;
use std::path::Path;

/// État partagé par les handlers HTTP.
#[derive(Clone)]
pub struct AppState {
    pub root: String,
}

/// Démarre le serveur HTTP et bloque jusqu'à son arrêt.
pub async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/files", get(list_files))
        .route("/health", get(health))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8090".to_string());
    let addr = format!("127.0.0.1:{port}").parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    println!("API HTTP : http://{addr}/files");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Point de santé : renvoie `ok` (200).
async fn health() -> &'static str {
    "ok"
}

/// Renvoie la liste JSON des fichiers du dossier surveillé.
async fn list_files(State(state): State<AppState>) -> Json<Vec<FileInfo>> {
    Json(scanner::scan_files(Path::new(&state.root)))
}
