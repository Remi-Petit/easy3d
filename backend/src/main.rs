use easy3d::{api, config, scanner, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::sync::broadcast;

/// Chemin du dossier de modèles.
///
/// Priorité : `config.models_root`, sinon variable d'env `MODELS_ROOT`,
/// sinon les données à la racine du repo (`../models` depuis `backend/`).
fn resolve_models_root(config: &config::Config) -> String {
    if let Some(p) = config.models_root.clone() {
        let path = Path::new(&p);
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
        };
        return joined.to_string_lossy().into_owned();
    }
    std::env::var("MODELS_ROOT").unwrap_or_else(|_| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../models")
            .to_string_lossy()
            .into_owned()
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Charge la configuration YAML (mode d'affichage, dossier des modèles…).
    let config = config::Config::load();

    let path = resolve_models_root(&config);

    // Canal broadcast : diffuse la liste des modèles (JSON) à tous les clients WS.
    let (ws_tx, _) = broadcast::channel::<String>(16);

    let state = api::AppState {
        root: path.clone(),
        ws: ws_tx.clone(),
        config: config.clone(),
    };

    // API → tâche async sur le pool Tokio (multi-thread).
    let server = tokio::spawn(async move {
        if let Err(e) = api::serve(state).await {
            eprintln!("Erreur serveur : {e}");
        }
    });

    // Watcher (bloquant) → thread dédié, car notify bloque le thread OS.
    // À chaque changement détecté, il rescanne et diffuse via le canal broadcast.
    let path_watcher = path.to_string();
    let ws_watcher = ws_tx.clone();
    let cfg_watcher = config.clone();
    std::thread::spawn(move || {
        if let Err(e) = watch_dir(&path_watcher, cfg_watcher, ws_watcher) {
            eprintln!("Erreur watcher : {e}");
        }
    });

    // Garde le runtime en vie tant que le serveur tourne.
    server.await?;

    Ok(())
}

/// Surveille le dossier. À chaque changement (ajout / modif / suppression),
/// rescanne et diffuse la nouvelle liste des modèles sur le canal broadcast.
fn watch_dir(
    path: &str,
    config: config::Config,
    ws: broadcast::Sender<String>,
) -> notify::Result<()> {
    let (tx, rx) = channel();

    // notify-debouncer-full regroupe la rafale d'événements et n'émet qu'un
    // résultat stable après le timeout (50ms = quasi instantané).
    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )?;

    debouncer.watch(path, RecursiveMode::Recursive)?;
    println!("Je surveille {path}... (Ctrl+C pour arrêter)");

    for result in rx {
        match result {
            Ok(events) => {
                let mut changed = false;
                for event in events {
                    // On n'affiche que les vrais changements (ajout / modif / suppression).
                    if let Some(msg) = watcher::describe_event(&event) {
                        println!("{msg}");
                        changed = true;
                    }
                }
                // Vrai changement → rescanne + broadcast aux clients WS.
                if changed {
                    let scan = scanner::scan_models(Path::new(path));
                    let payload = api::ModelsResponse::from_scan(scan, &config);
                    if let Ok(json) = serde_json::to_string(&payload) {
                        let _ = ws.send(json);
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