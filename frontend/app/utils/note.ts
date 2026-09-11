/**
 * Réconciliation d'une note entre la saisie locale et le serveur.
 *
 * Logique **pure**, testable sans composant : c'est la partie délicate de la
 * synchronisation temps réel (ne jamais écraser une saisie en cours).
 */

/** Résultat de la réconciliation. */
export interface NoteReconcile {
  /** Contenu local après réconciliation (inchangé en cas de conflit). */
  content: string
  /** Nouvelle référence « ce que porte le serveur ». */
  saved: string
  /** Un conflit a été détecté : la saisie locale a été préservée. */
  conflict: boolean
}

/**
 * Confronte une valeur venue du serveur à l'état local.
 *
 * - valeur identique à la référence → rien à faire ;
 * - modifications locales **non enregistrées** → on préserve la saisie et on
 *   signale le conflit (le reste à la charge de l'utilisateur) ;
 * - sinon → on suit le serveur.
 */
export function reconcileRemote(
  content: string,
  saved: string,
  incoming: string,
): NoteReconcile {
  if (incoming === saved) {
    return { content, saved, conflict: false }
  }
  if (content !== saved) {
    return { content, saved, conflict: true }
  }
  return { content: incoming, saved: incoming, conflict: false }
}
