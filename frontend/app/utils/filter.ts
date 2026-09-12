import type { FileInfo } from '~/composables/useModels'
// Import explicite : l'auto-import Nuxt n'existe pas sous Vitest.
import { basename, ext } from '~/utils/format'

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

/**
 * Clés de traduction du mode de tri, utilisées pour l'infobulle et
 * l'accessibilité (voir `useFilter`).
 */
export const SORT_LABEL_KEYS: Record<SortMode, string> = {
  none: 'filter.sort.none',
  'date-desc': 'filter.sort.desc',
  'date-asc': 'filter.sort.asc',
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

// ─────────────────────────────────────────────────────────────────────────
// Filtre par type de fichier
// ─────────────────────────────────────────────────────────────────────────

/** Un format présent, avec le nombre de fichiers concernés. */
export interface TypeCount {
  /** Extension en minuscules, sans le point (`stl`, `gcode`…). */
  type: string
  count: number
}

/**
 * Recense les formats présents, avec leur nombre, triés par nom.
 *
 * Les fichiers **sans extension** sont ignorés : il n'y aurait rien à proposer.
 */
export function collectTypes(files: FileInfo[]): TypeCount[] {
  const counts = new Map<string, number>()
  for (const file of files) {
    const type = ext(file.path)
    if (!type) continue
    counts.set(type, (counts.get(type) ?? 0) + 1)
  }
  return [...counts.entries()]
    .map(([type, count]) => ({ type, count }))
    .sort((a, b) => a.type.localeCompare(b.type))
}

/**
 * `true` si le fichier est d'un des types sélectionnés.
 * Une sélection vide accepte tout (aucun filtre de type).
 */
export function matchesTypes(file: FileInfo, types: string[]): boolean {
  return types.length === 0 || types.includes(ext(file.path))
}

/** Ajoute le type à la sélection, ou l'en retire s'il y était déjà. */
export function toggleType(types: string[], type: string): string[] {
  return types.includes(type) ? types.filter((t) => t !== type) : [...types, type]
}

/**
 * Ne conserve que les types encore disponibles.
 *
 * Après un changement de page, un format sélectionné peut avoir disparu : le
 * garder afficherait une liste vide sans qu'on puisse le désélectionner.
 * Renvoie la **même référence** si rien ne change (évite du travail réactif).
 */
export function pruneTypes(selected: string[], available: string[]): string[] {
  const kept = selected.filter((type) => available.includes(type))
  return kept.length === selected.length ? selected : kept
}
