use easy3d::{api, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::sync::mpsc::channel;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Charge le fichier `.env` s'il existe (ex. PORT=6000).
    dotenvy::dotenv().ok();

    let path = "./models";

    // Démarre l'API HTTP dans un thread séparé (le watcher reste bloquant ici).
    let state = api::AppState { root: path.to_string() };
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("runtime tokio");
        if let Err(e) = rt.block_on(api::serve(state)) {
            eprintln!("Erreur serveur : {e}");
        }
    });

    watch_dir(path)?;

    Ok(())
}

/// Surveille le dossier et affiche les changements (ajout / modif / suppression).
fn watch_dir(path: &str) -> notify::Result<()> {
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
                for event in events {
                    // On n'affiche que les vrais changements (ajout / modif / suppression).
                    if let Some(msg) = watcher::describe_event(&event) {
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