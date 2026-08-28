use notify::event::ModifyKind;
use notify::EventKind;
use notify_debouncer_full::DebouncedEvent;

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
