//! Test d'intégration : **le watcher suit-il réellement un déplacement ?**
//!
//! Les tests unitaires de `watcher::handle_event` synthétisent les événements :
//! ils vérifient la logique, pas l'hypothèse sur laquelle elle repose — à savoir
//! que `notify` (via le debouncer) émet un `RenameMode::Both` avec les deux
//! chemins **dans l'ordre (from, to)** quand un fichier est déplacé.
//!
//! Si un backend ou une version changeait ce comportement, le correctif « la
//! note suit le fichier » cesserait silencieusement de fonctionner, et seuls les
//! tests ci-dessous le signaleraient. D'où ce test, qui utilise un vrai
//! système de fichiers et un vrai watcher.

use easy3d::{notes, watcher};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// Durée maximale d'attente des événements du système (large : dépende de l'OS).
const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn un_fichier_deplace_emmene_sa_note() {
    let dir = tempfile::tempdir().unwrap();
    // `\\?\` (Windows) : la racine doit être canonicalisée comme dans `main.rs`,
    // sinon les chemins rapportés par le watcher ne préfixent plus la racine.
    let root = std::fs::canonicalize(dir.path()).unwrap();

    std::fs::create_dir_all(root.join("A")).unwrap();
    std::fs::create_dir_all(root.join("B")).unwrap();
    std::fs::write(root.join("A/x.stl"), "x").unwrap();
    notes::write(&root, "A/x.stl", "# note à suivre").unwrap();

    // Même configuration que `main.rs` (debounce court, canal dédié).
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )
    .unwrap();
    debouncer.watch(&root, RecursiveMode::Recursive).unwrap();

    // Le déplacement lui-même (ce que fait l'utilisateur dans l'explorateur).
    std::fs::rename(root.join("A/x.stl"), root.join("B/x.stl")).unwrap();

    // On traite les événements exactement comme `main.rs`.
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline && notes::read(&root, "B/x.stl").is_none() {
        let Ok(Ok(events)) = rx.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        for event in &events {
            eprintln!("[diag] {:?} {:?}", event.kind, event.paths);
        }
        // Exactement comme `main.rs` : le lot est traité d'un bloc.
        watcher::handle_batch(&root, &root.join("config.yml"), &events);
    }

    assert!(
        notes::read(&root, "A/x.stl").is_none(),
        "la note est restée à l'ancien emplacement (A/x.stl)"
    );
    assert_eq!(
        notes::read(&root, "B/x.stl").as_deref(),
        Some("# note à suivre"),
        "la note n'a pas suivi le fichier : le watcher n'a pas signalé le déplacement \
         sous la forme attendue (RenameMode::Both, chemins [from, to])"
    );
}

#[test]
fn deplacer_un_dossier_emmene_les_notes_du_sous_arbre() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();

    std::fs::create_dir_all(root.join("A/sous")).unwrap();
    std::fs::write(root.join("A/x.stl"), "x").unwrap();
    std::fs::write(root.join("A/sous/y.stl"), "y").unwrap();
    notes::write(&root, "A", "# note du dossier").unwrap();
    notes::write(&root, "A/x.stl", "# note de x").unwrap();
    notes::write(&root, "A/sous/y.stl", "# note de y").unwrap();

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(50),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )
    .unwrap();
    debouncer.watch(&root, RecursiveMode::Recursive).unwrap();

    std::fs::rename(root.join("A"), root.join("B")).unwrap();

    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline && notes::read(&root, "B/sous/y.stl").is_none() {
        let Ok(Ok(events)) = rx.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        // Exactement comme `main.rs` : le lot est traité d'un bloc.
        watcher::handle_batch(&root, &root.join("config.yml"), &events);
    }

    assert_eq!(notes::read(&root, "B").as_deref(), Some("# note du dossier"));
    assert_eq!(notes::read(&root, "B/x.stl").as_deref(), Some("# note de x"));
    assert_eq!(notes::read(&root, "B/sous/y.stl").as_deref(), Some("# note de y"));
}
