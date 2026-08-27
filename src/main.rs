use notify::{Watcher, RecursiveMode, RecommendedWatcher, Event};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

fn main() -> notify::Result<()> {
    let path = "./test";

    let (tx, rx) = channel();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    )?;

    watcher.watch(Path::new(path), RecursiveMode::Recursive)?;

    println!("Je surveille {path}... (Ctrl+C pour arrêter)");

    // Le state de dédoublonnage : ici, DANS la fonction
    let mut last: Option<(notify::EventKind, std::path::PathBuf, Instant)> = None;

    for event in rx {
        if matches!(event.kind, notify::EventKind::Access(_)) {
            continue;
        }

        let path_changed = event.paths.first().cloned().unwrap_or_default();

        if let Some((kind, p, t)) = &last {
            if *kind == event.kind
                && *p == path_changed
                && t.elapsed() < Duration::from_millis(100)
            {
                // doublon récent → on met juste à jour l'horodatage et on ignore
                last = Some((event.kind.clone(), path_changed, Instant::now()));
                continue;
            }
        }

        last = Some((event.kind.clone(), path_changed, Instant::now()));
        println!("Changement détecté : {:?}", event);
    }

    Ok(())
}