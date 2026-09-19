/**
 * Recherche assistée : types partagés avec le backend Rust (`ai/mod.rs`,
 * `ai/tools.rs`).
 *
 * Logique **pure**, testable sans serveur.
 */

/** Fournisseur de modèle, tel que l'administration le propose. */
export interface AiProvider {
  id: string
  label: string
  /** `false` pour un serveur local (Ollama) : aucune clé à saisir. */
  needs_key: boolean
  /** Adresse de l'API proposée par défaut (champ laissé vide). */
  base_url: string
  /** Modèle proposé par défaut. */
  model: string
}

/** Une proposition de la recherche assistée. */
export interface AiHit {
  /** Chemin relatif dans le catalogue (clé des pages fichier et dossier). */
  rel: string
  kind: 'file' | 'folder'
  /** Pourquoi cet élément correspond, rédigé par le modèle. */
  reason: string
}

/** Réponse de `POST /ai/search`. */
export interface AiOutcome {
  hits: AiHit[]
  /** Réponse libre du modèle, quand il n'a pas conclu par des résultats. */
  text?: string
  /** Nombre d'allers-retours effectués (diagnostic). */
  hops: number
}

/** Bloc de configuration de l'IA, tel qu'exposé par `/api/config`. */
export interface AiConfig {
  provider?: string | null
  base_url?: string | null
  model?: string | null
  /** Toujours masquée (`***`) côté interface : voir `config::KEY_PLACEHOLDER`. */
  api_key?: string | null
}

/** URL de la page d'un élément proposé. */
export function hitHref(hit: AiHit): string {
  return hit.kind === 'folder'
    ? `/dossiers/${encodeURIComponent(hit.rel)}`
    : `/fichier/${encodeURIComponent(hit.rel)}`
}

/**
 * `true` si un fournisseur est choisi.
 *
 * C'est **la seule** condition d'activation de la recherche assistée : le
 * backend refuse une clé absente avec un message explicite, et Ollama n'en
 * demande aucune.
 */
export function aiConfigured(ai: AiConfig | null | undefined): boolean {
  return !!ai?.provider?.trim()
}
