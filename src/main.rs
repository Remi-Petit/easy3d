use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::sync::mpsc::channel;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "./models";

    let (tx, rx) = channel();

    // notify-debouncer-full regroupe la rafale d'événements et n'émet qu'un
    // résultat stable après le timeout. 100ms = quasi instantané, sans couper
    // une rafale en deux ni laisser passer les doublons.
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