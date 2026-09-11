import type { FileInfo } from '~/composables/useModels'
// Import explicite : l'auto-import Nuxt n'existe pas sous Vitest.
import { basename } from '~/utils/format'

/**
 * Logique **pure** du filtre global (recherche + tri par date).
 *
 * Extraite de `useFilter` pour être testable sans contexte Nuxt : le composable
 * ne fait que l'envelopper dans des refs réactifs partagés.
 */

/** Mode de tri par date : ordre d'origine, du plus ancien au plus récent, ou l'inverse. */
export type SortMode = 'none' | 'date-asc' | 'date-desc'

/** Ordre de rotation au clic : normal -> récent → ancien -> ancien → récent -> normal. */
export const SORT_CYCLE: readonly SortMode[] = ['none', 'date-desc', 'date-asc']

/** Description du mode, utilisée pour l'infobulle et l'accessibilité. */
export const SORT_LABELS: Record<SortMode, string> = {
  none: 'date (ordre normal)',
  'date-desc': 'date : récent → ancien',
  'date-asc': 'date : ancien → récent',
}

/** Flèche affichée : ↕ = normal, ↓ = récent → ancien, ↑ = ancien → récent. */
export const SORT_ICONS: Record<SortMode, string> = {
  none: '↕',
  'date-desc': '↓',
  'date-asc': '↑',
}

/** Mode de tri suivant, en boucle. */
export function nextSortMode(mode: SortMode): SortMode {
  const i = SORT_CYCLE.indexOf(mode)
  return SORT_CYCLE[(i + 1) % SORT_CYCLE.length] ?? 'none'
}

/**
 * `true` si le fichier correspond à la recherche (nom de fichier).
 * Une requête vide (ou blanche) accepte tout.
 */
export function matchesQuery(file: FileInfo, query: string): boolean {
  const q = query.trim().toLowerCase()
  return !q || basename(file.path).toLowerCase().includes(q)
}

/**
 * Trie une liste de fichiers par date de modification.
 *
 * - `none` → renvoie la liste telle quelle (ordre du backend) ;
 * - les fichiers **sans date** sont toujours renvoyés en fin de liste ;
 * - la liste d'entrée n'est pas modifiée.
 */
export function sortFilesByDate<T extends FileInfo>(files: T[], mode: SortMode): T[] {
  if (mode === 'none') return files
  const dir = mode === 'date-asc' ? 1 : -1
  return [...files].sort((a, b) => {
    if (a.modified == null && b.modified == null) return 0
    if (a.modified == null) return 1
    if (b.modified == null) return -1
    return (a.modified - b.modified) * dir
  })
}
