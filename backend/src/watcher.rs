use crate::{notes, thumbnail};
use notify::EventKind;
use notify::event::{ModifyKind, RenameMode};
use notify_debouncer_full::DebouncedEvent;
use std::path::{Path, PathBuf};

/// `true` si l'événement traduit un **vrai changement** : création,
/// modification, renommage ou suppression.
///
/// On exclut volontairement les événements d'**accès** (`Access`) : `notify`
/// les émet à chaque ouverture de fichier (masque inotify `IN_OPEN`). Les
/// traiter comme des changements créerait une boucle sans fin — relire un
/// fichier (par ex. `config.yml` après l'avoir détecté modifié) en produirait
/// un nouveau à chaque fois.
pub fn is_content_change(event: &DebouncedEvent) -> bool {
    match &event.kind {
        EventKind::Access(_) | EventKind::Other => false,
        // Métadonnées seules (droits, propriétaire…) : pas un changement de contenu.
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        _ => true,
    }
}

/// `true` si un lot contient un **vrai** changement visant ce fichier précis.
///
/// Sert aux fichiers qui vivent à côté de `config.yml` (les adresses connues des
/// fournisseurs d'IA) : ils sont surveillés par le même dossier, mais ne
/// déclenchent pas la même relecture.
pub fn touches_file(events: &[DebouncedEvent], path: &Path) -> bool {
    events
        .iter()
        .any(|e| is_content_change(e) && e.paths.iter().any(|p| p == path))
}

/// Traduit un événement en message lisible : ajout / modification / suppression.
///
/// Retourne `None` pour les événements à ignorer (accès, métadonnées seules,
/// autres) — cohérent avec [`is_content_change`].
pub fn describe_event(event: &DebouncedEvent) -> Option<String> {
    let path = event
        .paths
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    match &event.kind {
        EventKind::Create(_) => Some(format!("Ajout : {path}")),
        EventKind::Remove(_) => Some(format!("Suppression : {path}")),
        EventKind::Modify(ModifyKind::Name(_)) => Some(format!("Renommage : {path}")),
        // Métadonnées seules (droits, horodatage…) : pas un changement de contenu.
        EventKind::Modify(ModifyKind::Metadata(_)) => None,
        EventKind::Modify(_) => Some(format!("Modification : {path}")),
        // On ignore les accès et les événements non pertinents.
        _ => None,
    }
}

/// Chemins (source, destination) d'un événement de déplacement / renommage.
///
/// `notify` documente que `RenameMode::Both` fournit les deux chemins **dans cet
/// ordre** (from, to). Attention : tous les déplacements ne sont pas signalés
/// ainsi — voir [`reconcile_moved_files`].
pub fn moved_paths(event: &DebouncedEvent) -> Option<(PathBuf, PathBuf)> {
    if !matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both))
    ) {
        return None;
    }
    let mut paths = event.paths.iter();
    let from = paths.next()?.clone();
    let to = paths.next()?.clone();
    Some((from, to))
}

/// Ce qu'un lot d'événements implique pour l'application.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BatchEffect {
    /// Un vrai changement dans les modèles → rediffuser la liste.
    pub models_changed: bool,
    /// Le fichier de configuration a changé → le recharger.
    pub config_changed: bool,
    /// Messages lisibles à afficher (ajout / modification / suppression).
    pub messages: Vec<String>,
}

/// Traite **un lot** d'événements du debouncer et applique ses effets de bord.
///
/// Le lot (et non l'événement) est la bonne unité : un même déplacement peut
/// produire plusieurs événements, et c'est à l'échelle du lot qu'on peut les
/// réapparier.
///
/// Effets de bord :
/// - la **note** d'un fichier déplacé suit son propriétaire ;
/// - les **aperçus** générés sont régénérés pour la cible, supprimés pour la
///   source ;
/// - les dossiers cachés `.easy3d-thumbs/` et `.easy3d-notes/` sont ignorés :
///   ce sont nos propres artefacts, les rediffuser bouclerait.
pub fn handle_batch(root: &Path, config_path: &Path, events: &[DebouncedEvent]) -> BatchEffect {
    let mut effect = BatchEffect::default();
    let mut removed: Vec<PathBuf> = Vec::new();
    let mut created: Vec<PathBuf> = Vec::new();

    for event in events {
        if !is_content_change(event) {
            continue;
        }

        // Événement sur le fichier de configuration ?
        if event.paths.iter().any(|p| p == config_path) {
            effect.config_changed = true;
            continue;
        }

        // Événement hors du dossier des modèles → ignoré (ex : autres fichiers
        // du dossier `backend/`, artefacts de build…).
        if !event.paths.iter().any(|p| p.starts_with(root)) {
            continue;
        }

        let thumbs_root = root.join(thumbnail::THUMB_DIR);
        let notes_root = root.join(notes::NOTES_DIR);
        if event
            .paths
            .iter()
            .all(|p| p.starts_with(&thumbs_root) || p.starts_with(&notes_root))
        {
            continue;
        }

        // Renommage signalé explicitement (même dossier) : la note suit.
        if let Some((from, to)) = moved_paths(event) {
            move_note(root, &from, &to);
        }

        for path in &event.paths {
            if path.starts_with(&thumbs_root) || path.starts_with(&notes_root) {
                continue;
            }
            match event.kind {
                EventKind::Remove(_) => removed.push(path.clone()),
                EventKind::Create(_) if path.is_file() => created.push(path.clone()),
                _ => {}
            }

            // Aperçus : régénéré pour la cible, supprimé pour la source.
            if path.exists() {
                thumbnail::ensure_for_changed(root, &thumbs_root, path);
            } else {
                thumbnail::remove_for_path(root, &thumbs_root, path);
            }
        }

        if let Some(msg) = describe_event(event) {
            effect.messages.push(msg);
            effect.models_changed = true;
        }
    }

    reconcile_moved_files(root, &removed, &created);

    effect
}

/// Réapparie les déplacements **entre dossiers**.
///
/// Selon la plateforme, déplacer un fichier vers un autre dossier n'est pas
/// signalé comme un renommage mais comme une **suppression suivie d'une
/// création** : `moved_paths` ne peut alors rien relier, et la note serait
/// perdue. Les deux événements arrivant dans le même lot, on réapparie ici par
/// **nom de fichier**, uniquement quand une note est réellement en jeu (et de
/// façon non ambiguë : un seul candidat).
fn reconcile_moved_files(root: &Path, removed: &[PathBuf], created: &[PathBuf]) {
    for from in removed {
        let Some(from_rel) = rel_of(root, from) else {
            continue;
        };
        // Rien à sauver si aucune note n'existe à l'ancien emplacement.
        if notes::read(root, &from_rel).is_none() {
            continue;
        }

        let Some(name) = from.file_name() else {
            continue;
        };
        let mut candidates = created
            .iter()
            .filter(|p| p.as_path() != from.as_path() && p.file_name() == Some(name));

        // Ambiguïté (aucun ou plusieurs candidats) : on ne devine pas.
        let (Some(to), None) = (candidates.next(), candidates.next()) else {
            continue;
        };
        move_note(root, from, to);
    }
}

/// Déplace la note d'un élément, en signalant les échecs sans interrompre.
fn move_note(root: &Path, from: &Path, to: &Path) {
    if let Err(e) = notes::move_for_path(root, from, to) {
        eprintln!(
            "⚠️  Note non déplacée ({} → {}) : {e}",
            from.display(),
            to.display()
        );
    }
}

/// Chemin relatif (séparateurs `/`) de `path` par rapport à `root`.
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode,
    };
    use notify::{Event, EventKind};
    use std::path::PathBuf;
    use std::time::Instant;

    /// Construit un `DebouncedEvent` avec un chemin, comme le fait le debouncer.
    fn event(kind: EventKind, path: &str) -> DebouncedEvent {
        let mut event = Event::new(kind);
        event.paths.push(PathBuf::from(path));
        DebouncedEvent::new(event, Instant::now())
    }

    #[test]
    fn ignore_les_acces() {
        // Ouvrir/fermer un fichier (inotify IN_OPEN) n'est pas un changement :
        // les traiter comme tel créerait une boucle (relire → nouvel événement).
        for kind in [
            EventKind::Access(AccessKind::Any),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Close(AccessMode::Any)),
        ] {
            let ev = event(kind, "config.yml");
            assert!(!is_content_change(&ev), "accepté à tort : {ev:?}");
            assert_eq!(describe_event(&ev), None);
        }
    }

    #[test]
    fn ignore_les_metadonnees_et_les_evenements_autres() {
        let ev = event(
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            "a.stl",
        );
        assert!(!is_content_change(&ev));
        assert_eq!(describe_event(&ev), None);

        assert!(!is_content_change(&event(EventKind::Other, "a.stl")));
    }

    #[test]
    fn accepte_creation_suppression_et_modification() {
        for kind in [
            EventKind::Create(CreateKind::File),
            EventKind::Remove(RemoveKind::File),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
        ] {
            assert!(is_content_change(&event(kind, "a.stl")), "refusé à tort");
        }
    }

    #[test]
    fn decrit_les_changements_en_francais() {
        let cases = [
            (EventKind::Create(CreateKind::File), "Ajout : models/a.stl"),
            (
                EventKind::Remove(RemoveKind::File),
                "Suppression : models/a.stl",
            ),
            (
                EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                "Renommage : models/a.stl",
            ),
            (
                EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                "Modification : models/a.stl",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                describe_event(&event(kind, "models/a.stl")).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn chemin_absent_donne_un_message_vide() {
        // Certains événements n'ont pas de chemin : on ne doit pas paniquer.
        let ev = DebouncedEvent::new(
            Event::new(EventKind::Create(CreateKind::Any)),
            Instant::now(),
        );
        assert_eq!(describe_event(&ev).as_deref(), Some("Ajout : "));
    }

    /// Événement de déplacement avec les deux chemins (ordre `notify` : from, to).
    fn moved_event(from: &std::path::Path, to: &std::path::Path) -> DebouncedEvent {
        let mut event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)));
        event.paths.push(from.to_path_buf());
        event.paths.push(to.to_path_buf());
        DebouncedEvent::new(event, Instant::now())
    }

    #[test]
    fn moved_paths_lit_un_deplacement_complet() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("A/x.stl");
        let to = dir.path().join("B/x.stl");

        let (got_from, got_to) = moved_paths(&moved_event(&from, &to)).unwrap();
        assert_eq!(got_from, from);
        assert_eq!(got_to, to);
    }

    #[test]
    fn moved_paths_ignore_les_autres_evenements() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.stl");

        // Création, suppression, modification de contenu : pas un déplacement.
        for kind in [
            EventKind::Create(CreateKind::File),
            EventKind::Remove(RemoveKind::File),
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
        ] {
            assert!(moved_paths(&event(kind, path.to_str().unwrap())).is_none());
        }

        // Un renommage partiel (From seul) ne permet pas de relier les chemins.
        assert!(
            moved_paths(&event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                path.to_str().unwrap()
            ))
            .is_none()
        );

        // `Both` avec un seul chemin : incomplet, on s'abstient.
        let mut incomplete = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)));
        incomplete.paths.push(path);
        assert!(moved_paths(&DebouncedEvent::new(incomplete, Instant::now())).is_none());
    }

    /// Prépare une racine avec les dossiers `A` et `B`.
    fn models_tree() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("A")).unwrap();
        std::fs::create_dir_all(root.join("B")).unwrap();
        (dir, root)
    }

    fn note_of(root: &Path, rel: &str) -> Option<String> {
        crate::notes::read(root, rel)
    }

    /// Renommage signalé explicitement par l'OS (même dossier).
    #[test]
    fn un_renommage_explicite_emmene_la_note() {
        let (_dir, root) = models_tree();
        std::fs::write(root.join("A/y.stl"), "x").unwrap();
        crate::notes::write(&root, "A/x.stl", "# note à suivre").unwrap();

        let batch = [moved_event(&root.join("A/x.stl"), &root.join("A/y.stl"))];
        let effect = handle_batch(&root, &root.join("config.yml"), &batch);

        assert!(effect.models_changed);
        assert!(note_of(&root, "A/x.stl").is_none());
        assert_eq!(
            note_of(&root, "A/y.stl").as_deref(),
            Some("# note à suivre")
        );
    }

    /// Déplacement **entre dossiers** : l'OS le signale comme une suppression
    /// suivie d'une création (constaté sous Windows, voir `tests/note_move.rs`).
    /// Les deux événements du même lot sont réappariés par nom de fichier.
    #[test]
    fn un_deplacement_entre_dossiers_emmene_la_note() {
        let (_dir, root) = models_tree();
        std::fs::write(root.join("B/x.stl"), "x").unwrap();
        crate::notes::write(&root, "A/x.stl", "# note à suivre").unwrap();

        let batch = [
            event(
                EventKind::Remove(RemoveKind::File),
                root.join("A/x.stl").to_str().unwrap(),
            ),
            event(
                EventKind::Create(CreateKind::File),
                root.join("B/x.stl").to_str().unwrap(),
            ),
        ];
        handle_batch(&root, &root.join("config.yml"), &batch);

        assert!(note_of(&root, "A/x.stl").is_none());
        assert_eq!(
            note_of(&root, "B/x.stl").as_deref(),
            Some("# note à suivre")
        );
    }

    #[test]
    fn une_suppression_sans_note_ne_deplace_rien() {
        let (_dir, root) = models_tree();
        std::fs::write(root.join("B/x.stl"), "x").unwrap();

        let batch = [
            event(
                EventKind::Remove(RemoveKind::File),
                root.join("A/x.stl").to_str().unwrap(),
            ),
            event(
                EventKind::Create(CreateKind::File),
                root.join("B/x.stl").to_str().unwrap(),
            ),
        ];
        handle_batch(&root, &root.join("config.yml"), &batch);

        // Aucune note n'existait : rien n'est créé.
        assert!(!root.join(".easy3d-notes").exists());
    }

    #[test]
    fn un_deplacement_ambigue_ne_deplace_rien() {
        let (_dir, root) = models_tree();
        std::fs::create_dir_all(root.join("C")).unwrap();
        std::fs::write(root.join("B/x.stl"), "x").unwrap();
        std::fs::write(root.join("C/x.stl"), "x").unwrap();
        crate::notes::write(&root, "A/x.stl", "# note").unwrap();

        // Deux créations portent le même nom : on ne devine pas.
        let batch = [
            event(
                EventKind::Remove(RemoveKind::File),
                root.join("A/x.stl").to_str().unwrap(),
            ),
            event(
                EventKind::Create(CreateKind::File),
                root.join("B/x.stl").to_str().unwrap(),
            ),
            event(
                EventKind::Create(CreateKind::File),
                root.join("C/x.stl").to_str().unwrap(),
            ),
        ];
        handle_batch(&root, &root.join("config.yml"), &batch);

        assert_eq!(note_of(&root, "A/x.stl").as_deref(), Some("# note"));
        assert!(note_of(&root, "B/x.stl").is_none());
    }

    #[test]
    fn handle_batch_classe_les_evenements() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let config = root.join("config.yml");
        std::fs::write(&config, "display: {}\n").unwrap();

        // Config → rechargement.
        let effect = handle_batch(
            root,
            &config,
            &[event(
                EventKind::Modify(ModifyKind::Any),
                config.to_str().unwrap(),
            )],
        );
        assert!(effect.config_changed);
        assert!(!effect.models_changed);

        // Changement dans les modèles → message + rediffusion.
        let model = root.join("sub/a.stl");
        std::fs::write(&model, "x").unwrap();
        let effect = handle_batch(
            root,
            &config,
            &[event(
                EventKind::Create(CreateKind::File),
                model.to_str().unwrap(),
            )],
        );
        assert!(effect.models_changed);
        assert!(effect.messages[0].starts_with("Ajout : "));

        // Hors de la racine des modèles → ignoré.
        let outside = dir.path().parent().unwrap().join("hors.stl");
        let effect = handle_batch(
            root,
            &config,
            &[event(
                EventKind::Create(CreateKind::File),
                outside.to_str().unwrap(),
            )],
        );
        assert!(!effect.models_changed && !effect.config_changed);

        // Accès : pas un changement.
        let effect = handle_batch(
            root,
            &config,
            &[event(
                EventKind::Access(AccessKind::Read),
                model.to_str().unwrap(),
            )],
        );
        assert!(!effect.models_changed && !effect.config_changed);
    }

    #[test]
    fn handle_batch_ignore_nos_propres_artefacts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let config = root.join("config.yml");

        // Écrire une note ne doit pas déclencher de rediffusion (boucle).
        for dir_name in [crate::thumbnail::THUMB_DIR, crate::notes::NOTES_DIR] {
            let file = root.join(dir_name).join("sub/a.stl.md");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "x").unwrap();

            let effect = handle_batch(
                root,
                &config,
                &[event(
                    EventKind::Modify(ModifyKind::Any),
                    file.to_str().unwrap(),
                )],
            );
            assert_eq!(
                effect,
                BatchEffect::default(),
                "{dir_name} devrait être ignoré"
            );
        }
    }
}
