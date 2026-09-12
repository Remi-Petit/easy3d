use easy3d::{api, config, formats, thumbnail, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Rappel des formats reconnus (registre `formats`) : utile pour vérifier
    // qu'un format ajouté est bien pris en compte au démarrage.
    let names: Vec<&str> = formats::all().iter().map(|f| f.name()).collect();
    println!("Formats reconnus : {}", names.join(", "));

    // Charge la configuration YAML (mode d'affichage, dossier des modèles…).
    let config = config::Config::load();

    // Chemin du fichier de config, résolu une fois (surveillé ensuite à chaud).
    // Canonicalisé quand possible : les chemins d'événements du watcher sont
    // comparés à cette référence.
    let config_path = std::fs::canonicalize(config::Config::config_path())
        .unwrap_or_else(|_| config::Config::config_path());

    let resolved = config.resolve_models_root();
    // Canonicalise la racine : évite les `..` (ex : `backend/../models`) et
    // fiabilise les tests de préfixe (`starts_with`) du watcher.
    let root = std::fs::canonicalize(&resolved).unwrap_or_else(|_| PathBuf::from(&resolved));

    // Dossier des aperçus générés par le backend (`<models_root>/.easy3d-thumbs`).
    let thumbs_root = root.join(thumbnail::THUMB_DIR);

    // Génère un aperçu PNG pour chaque modèle déjà présent, avant de démarrer.
    // (Garde un aperçu à jour ; coût une fois au lancement.)
    thumbnail::generate_all(&root, &thumbs_root);

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

        // L'interprétation des événements vit dans `watcher` (testable) : ici on
        // ne fait que router le résultat du lot.
        let effect = watcher::handle_batch(&current_root, &config_path, &events);
        for msg in &effect.messages {
            println!("{msg}");
        }
        config_changed |= effect.config_changed;
        models_changed |= effect.models_changed;

        // ── Rechargement à chaud de la configuration ──────────────────────
        if config_changed {
            let reloaded = config::Config::load();
            let changed = reloaded != *state.config.read().unwrap();

            if changed {
                let new_root = reloaded.resolve_models_root();
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
                            thumbnail::generate_all(
                                &new_root,
                                &new_root.join(thumbnail::THUMB_DIR),
                            );
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
            api::broadcast_snapshot(&state);
        } else if models_changed {
            api::broadcast_snapshot(&state);
        }
    }

    Ok(())
}
