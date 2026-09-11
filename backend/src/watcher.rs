use notify::event::ModifyKind;
use notify::EventKind;
use notify_debouncer_full::DebouncedEvent;

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
            (EventKind::Remove(RemoveKind::File), "Suppression : models/a.stl"),
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
        let ev = DebouncedEvent::new(Event::new(EventKind::Create(CreateKind::Any)), Instant::now());
        assert_eq!(describe_event(&ev).as_deref(), Some("Ajout : "));
    }
}
