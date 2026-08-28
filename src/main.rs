use axum::{extract::State, routing::get, Json, Router};
use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
struct AppState {
    root: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Charge le fichier `.env` s'il existe (ex. PORT=6000).
    dotenvy::dotenv().ok();

    let path = "./models";

    // Démarre l'API HTTP dans un thread séparé (le watcher reste bloquant ici).
    let state = AppState { root: path.to_string() };
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("runtime tokio");
        if let Err(e) = rt.block_on(serve(state)) {
            eprintln!("Erreur serveur : {e}");
        }
    });

    let (tx, rx) = channel();

    // notify-debouncer-full regroupe la rafale d'événements et n'émet qu'un
    // résultat stable après le timeout (50ms = quasi instantané).
    let mut debouncer = new_debouncer(Duration::from_millis(50), None, move |result: DebounceEventResult| {
        let _ = tx.send(result);
    })?;

    debouncer.watch(path, RecursiveMode::Recursive)?;

    println!("Je surveille {path}... (Ctrl+C pour arrêter)");

    for result in rx {
        match result {
            Ok(events) => {
                for event in events {
                    // On n'affiche que les vrais changements (ajout / modif / suppression).
                    if let Some(msg) = describe_event(&event) {
                        println!("{msg}");
                    }
                }
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("Erreur : {error}");
                }
            }
        }
    }

    Ok(())
}

async fn serve(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
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

async fn health() -> &'static str {
    "ok"
}

async fn list_files(State(state): State<AppState>) -> Json<Vec<FileInfo>> {
    Json(scan_files(Path::new(&state.root)))
}

#[derive(Serialize)]
struct FileInfo {
    path: String,
    created: Option<u64>,
    modified: Option<u64>,
}

/// Liste récursive des fichiers (hors dossiers) avec date de création/modification.
fn scan_files(root: &Path) -> Vec<FileInfo> {
    let mut out = Vec::new();
    scan_dir_recursive(root, &mut out);
    out
}

fn scan_dir_recursive(dir: &Path, out: &mut Vec<FileInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, out);
        } else if path.is_file() {
            if let Ok(meta) = entry.metadata() {
                out.push(FileInfo {
                    path: path.display().to_string(),
                    created: meta.created().ok().and_then(to_unix_secs),
                    modified: meta.modified().ok().and_then(to_unix_secs),
                });
            }
        }
    }
}

fn to_unix_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Traduit un événement en message lisible : ajout / modification / suppression.
/// Retourne `None` pour les événements à ignorer (accès, autres).
fn describe_event(event: &notify_debouncer_full::DebouncedEvent) -> Option<String> {
    let path = event
        .paths
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    match &event.kind {
        EventKind::Create(_) => Some(format!("Ajout : {path}")),
        EventKind::Remove(_) => Some(format!("Suppression : {path}")),
        EventKind::Modify(ModifyKind::Name(_)) => Some(format!("Renommage : {path}")),
        EventKind::Modify(_) => Some(format!("Modification : {path}")),
        // On ignore les accès et les événements non pertinents.
        _ => None,
    }
}