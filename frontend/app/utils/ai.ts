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
  /**
   * Modèles annoncés par le fournisseur la dernière fois qu'il a été interrogé.
   * Conservés côté serveur : la liste déroulante est donc remplie dès l'ouverture
   * de l'administration, sans avoir à recliquer sur « Tester ».
   */
  models?: string[] | null
}

/** Réponse de `POST /ai/models` : les modèles proposés par le fournisseur. */
export interface AiModelsResponse {
  models: string[]
}

/**
 * Options de la liste déroulante du modèle.
 *
 * La liste vient de **deux** sources : ce que le fournisseur a annoncé et ce qui
 * est enregistré dans la configuration. On dédoublonne, sinon la même entrée
 * apparaît deux fois (« fake-small » sélectionné, puis « fake-small » en
 * doublon un peu plus bas).
 *
 * Le modèle retenu reste proposé même s'il n'est pas (encore) annoncé : au
 * chargement de la page, rien n'a été interrogé, et sans cette règle le champ
 * apparaîtrait vide — l'enregistrement suivant effacerait alors le modèle.
 */
export function modelOptions(
  discovered: string[],
  current: string | null | undefined,
): string[] {
  const options: string[] = []
  for (const name of discovered) {
    const value = name?.trim()
    if (value && !options.includes(value)) options.push(value)
  }

  const kept = current?.trim()
  if (kept && !options.includes(kept)) options.unshift(kept)
  return options
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
