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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app(root: &str) -> Router {
        Router::new()
            .route("/files", get(list_files))
            .route("/health", get(health))
            .with_state(AppState { root: root.to_string() })
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
    async fn files_renvoie_la_liste_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();

        let app = test_app(dir.path().to_str().unwrap());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/files")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0]["path"].as_str().unwrap().ends_with("a.txt"));
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
}

