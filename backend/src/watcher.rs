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
/// Retourne `None` pour les événements à ignorer (accès, autres).
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
        EventKind::Modify(_) => Some(format!("Modification : {path}")),
        // On ignore les accès et les événements non pertinents.
        _ => None,
    }
}
