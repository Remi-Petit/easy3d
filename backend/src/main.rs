use easy3d::{api, config, render, scanner, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::path::{Path, PathBuf};
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

    // Chemin du fichier de config, résolu une fois (surveillé ensuite à chaud).
    // Canonicalisé quand possible : les chemins d'événements du watcher sont
    // comparés à cette référence.
    let config_path = std::fs::canonicalize(config::Config::config_path())
        .unwrap_or_else(|_| config::Config::config_path());

    let resolved = resolve_models_root(&config);
    // Canonicalise la racine : évite les `..` (ex : `backend/../models`) et
    // fiabilise les tests de préfixe (`starts_with`) du watcher.
    let root = std::fs::canonicalize(&resolved).unwrap_or_else(|_| PathBuf::from(&resolved));

    // Dossier des aperçus générés par le backend (`<models_root>/.easy3d-thumbs`).
    let thumbs_root = root.join(render::THUMB_DIR);

    // Génère un aperçu PNG pour chaque modèle déjà présent, avant de démarrer.
    // (Garde un aperçu à jour ; coût une fois au lancement.)
    render::generate_all(&root, &thumbs_root);

    // Canal broadcast : diffuse la liste des modèles (JSON) à tous les clients WS.
    let (ws_tx, _) = broadcast::channel::<String>(16);

    let state = api::AppState::new(root.clone(), ws_tx, config);

    // API → tâche async sur le pool Tokio (multi-thread).
    let server = tokio::spawn({
        let state = state.clone();
        async move {
            if let Err(e) = api::serve(state).await {
                eprintln!("Erreur serveur : {e}");
            }
        }
    });

    // Watcher (bloquant) → thread dédié, car notify bloque le thread OS.
    // Il surveille les modèles **et** la config : à chaque changement détecté,
    // il rescanne (et recharge la config le cas échéant) puis diffuse via le
    // canal broadcast — le frontend se met à jour sans recharger la page.
    std::thread::spawn({
        let state = state.clone();
        move || {
            if let Err(e) = watch_dir(root, config_path, state) {
                eprintln!("Erreur watcher : {e}");
            }
        }
    });

    // Garde le runtime en vie tant que le serveur tourne.
    server.await?;

    Ok(())
}

/// Surveille le dossier des modèles **et** le fichier de configuration.
///
/// - Changement dans les modèles → aperçus mis à jour + nouvelle liste diffusée.
/// - Changement de la config → rechargée à chaud (mode d'affichage, dossier des
///   modèles). Si `models_root` change, la surveillance bascule sur le nouveau
///   dossier. Dans tous les cas, un snapshot complet est diffusé aux clients WS
///   pour rafraîchir le frontend sans recharger la page.
fn watch_dir(
    models_root: PathBuf,
    config_path: PathBuf,
    state: api::AppState,
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

    let mut current_root = models_root;
    debouncer.watch(&current_root, RecursiveMode::Recursive)?;

    // On surveille le **dossier** parent du fichier de config (mode non
    // récursif) plutôt que le fichier lui-même : cela reste fiable lorsque
    // l'éditeur remplace le fichier (`rename`), ce qui invaliderait une
    // surveillance directe.
    if let Some(dir) = config_path.parent() {
        debouncer.watch(dir, RecursiveMode::NonRecursive)?;
    }

    println!(
        "Je surveille {} et {}… (Ctrl+C pour arrêter)",
        current_root.display(),
        config_path.display()
    );

    for result in rx {
        let events = match result {
            Ok(events) => events,
            Err(errors) => {
                for error in errors {
                    eprintln!("Erreur : {error}");
                }
                continue;
            }
        };

        let mut models_changed = false;
        let mut config_changed = false;

        for event in events {
            // On ignore les événements d'accès (ouverture/lecture d'un fichier) :
            // ils ne traduisent pas un changement et provoqueraient des boucles
            // (chaque relecture du fichier de config en génère un).
            if !watcher::is_content_change(&event) {
                continue;
            }

            // Événement sur le fichier de configuration ?
            if event.paths.iter().any(|p| p == &config_path) {
                config_changed = true;
                continue;
            }
            // Événement hors du dossier des modèles → ignoré (ex : autres
            // fichiers du dossier `backend/`, artefacts de build…).
            if !event.paths.iter().any(|p| p.starts_with(&current_root)) {
                continue;
            }

            let thumbs_root = current_root.join(render::THUMB_DIR);
            // Événements provoqués par nos propres aperçus générés : ce dossier
            // caché n'apparaît pas dans le scan, il n'y a donc rien à diffuser.
            if event.paths.iter().all(|p| p.starts_with(&thumbs_root)) {
                continue;
            }

            // Met à jour l'aperçu généré pour chaque chemin concerné.
            for p in &event.paths {
                if p.exists() {
                    render::ensure_for_changed(&current_root, &thumbs_root, p);
                } else {
                    render::remove_for_path(&current_root, &thumbs_root, p);
                }
            }
            // On n'affiche que les vrais changements (ajout / modif / suppression).
            if let Some(msg) = watcher::describe_event(&event) {
                println!("{msg}");
                models_changed = true;
            }
        }

        // ── Rechargement à chaud de la configuration ──────────────────────
        if config_changed {
            let reloaded = config::Config::load();
            let changed = reloaded != *state.config.read().unwrap();

            if changed {
                let new_root = PathBuf::from(resolve_models_root(&reloaded));
                let new_root = std::fs::canonicalize(&new_root).unwrap_or(new_root);

                if new_root != current_root {
                    println!(
                        "Dossier des modèles : {} → {}",
                        current_root.display(),
                        new_root.display()
                    );
                    let _ = debouncer.unwatch(&current_root);
                    match debouncer.watch(&new_root, RecursiveMode::Recursive) {
                        Ok(()) => {
                            render::generate_all(&new_root, &new_root.join(render::THUMB_DIR));
                            current_root = new_root;
                        }
                        Err(e) => {
                            // Échec : on rétablit la surveillance de l'ancien dossier.
                            eprintln!("Impossible de surveiller {} : {e}", new_root.display());
                            let _ = debouncer.watch(&current_root, RecursiveMode::Recursive);
                        }
                    }
                }

                *state.config.write().unwrap() = reloaded.clone();
                *state.root.write().unwrap() = current_root.clone();

                let mode = match reloaded.display.mode {
                    config::DisplayMode::Image => "image",
                    config::DisplayMode::ThreeD => "3d",
                };
                println!("Configuration rechargée (mode d'affichage : {mode}).");
            }

            // Même si la config est identique, on rafraîchit le front.
            broadcast_snapshot(&state);
        } else if models_changed {
            broadcast_snapshot(&state);
        }
    }

    Ok(())
}

/// Rescanne l'état courant et diffuse la liste des modèles (+ config) aux WS.
fn broadcast_snapshot(state: &api::AppState) {
    let payload =
        api::ModelsResponse::from_scan(scanner::scan_models(&state.root()), &state.config());
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = state.ws.send(json);
    }
}