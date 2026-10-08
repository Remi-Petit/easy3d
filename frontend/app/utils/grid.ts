import type { SortMode } from '~/utils/filter'

/**
 * Logique **pure** de la grille de données (voir `components/DataGrid.vue`) :
 * le tri, la fenêtre de lignes rendues, la valeur de tri d'une colonne.
 *
 * Extraite du composant pour être testable sans contexte Nuxt — même découpage
 * que `filter.ts` (le catalogue) et `journal.ts` (le journal d'audit).
 */

/**
 * Mode de tri de la grille.
 *
 * Ce sont les valeurs de `filter.ts` : la grille réutilise le **cycle** de
 * rotation (`SORT_CYCLE`) et les **flèches** (`SORT_ICONS`) du catalogue et du
 * journal, pour que ↕ ↓ ↑ veuillent dire la même chose partout. Le mot « date »
 * dans `date-asc` est historique : ne lire que « asc », « desc » et « none ».
 */
export type GridSortMode = SortMode

/** Tri en cours : la colonne, et son sens (`none` = ordre reçu, non trié). */
export interface GridSort {
  id: string
  mode: GridSortMode
}

/** Une colonne, telle que la grille la reçoit. */
export interface GridColumn<T = Record<string, unknown>> {
  /** Identifiant stable : clé de tri, et nom du gabarit de cellule (`#cell-<id>`). */
  id: string
  /** Libellé d'en-tête, **déjà traduit** (la grille ne traduit pas les données). */
  header: string
  /** Largeur en pixels, ajustable à la souris. */
  width: number
  /** `true` si un clic sur l'en-tête trie la colonne. */
  sortable?: boolean
  /** Alignement du contenu : les nombres se lisent à droite. */
  align?: 'left' | 'right'
  /** Valeur de tri ; par défaut, la propriété `id` de la ligne. */
  sortValue?: (row: T) => string | number
}

/**
 * Clés de traduction du sens du tri.
 *
 * Elles ne sont pas dans `filter.ts` : là-bas, les libellés parlent de la
 * **date** (« récent → ancien »), qui n'a aucun sens pour une colonne de noms.
 * Le cycle et les flèches, eux, restent partagés.
 */
export const GRID_SORT_LABEL_KEYS: Record<GridSortMode, string> = {
  none: 'grid.sortNone',
  'date-desc': 'grid.sortDesc',
  'date-asc': 'grid.sortAsc',
}

/**
 * Valeur d'une colonne pour une ligne.
 *
 * La clé de tri n'est pas forcément la clé d'affichage — un libellé calculé, un
 * état, des rôles déjà mis en mots : la colonne peut dire **comment** se trier.
 */
export function sortValueOf<T>(column: GridColumn<T>, row: T): string | number {
  if (column.sortValue) return column.sortValue(row)
  const value = (row as Record<string, unknown>)[column.id]
  if (typeof value === 'number') return value
  return value == null ? '' : String(value)
}

/**
 * Trie une copie des lignes selon la colonne et le sens demandés.
 *
 * `null` (ou un mode `none`) rend les lignes **telles quelles** : l'ordre reçu
 * est celui du backend, et c'est un ordre qui veut dire quelque chose (les
 * comptes arrivent triés, les événements du journal sont datés).
 *
 * Les nombres se comparent en nombre, le reste en texte — insensible à la casse
 * et aux accents (`sensitivity: 'base'`), ce qu'on attend d'une liste de noms
 * (« Élodie » se range avec « elodie », pas après « Zoé »).
 */
export function sortRows<T>(rows: T[], columns: GridColumn<T>[], sort: GridSort | null): T[] {
  if (!sort || sort.mode === 'none') return rows
  const column = columns.find((candidate) => candidate.id === sort.id)
  // Colonne inconnue (elle a disparu entre deux rendus) : mieux vaut l'ordre
  // reçu qu'un tri silencieusement faux.
  if (!column) return rows

  const sens = sort.mode === 'date-asc' ? 1 : -1
  return [...rows].sort((a, b) => {
    const va = sortValueOf(column, a)
    const vb = sortValueOf(column, b)
    if (typeof va === 'number' && typeof vb === 'number') return (va - vb) * sens
    return String(va).localeCompare(String(vb), undefined, { numeric: true, sensitivity: 'base' }) * sens
  })
}

/** Fenêtre de lignes à rendre (virtualisation) : bornes et hauteurs des cales. */
export interface GridWindow {
  /** Premier index rendu. */
  start: number
  /** Dernier index rendu, **exclu**. */
  end: number
  /** Hauteur de la cale du haut, en pixels. */
  paddingTop: number
  /** Hauteur de la cale du bas, en pixels. */
  paddingBottom: number
}

/**
 * Calcule les lignes à rendre pour une position de défilement donnée.
 *
 * `overscan` rend quelques lignes de plus de chaque côté : sans elles, une ligne
 * apparaît une fraction de seconde après être entrée dans la vue (le temps que
 * l'événement de défilement soit traité), et on la voit se poser.
 *
 * Les entrées sont bornées (`count`, `scrollTop`, `viewportHeight`) plutôt que
 * supposées saines : un conteneur pas encore mesuré (`viewportHeight = 0`) ou un
 * défilement négatif (rebond élastique) ne doivent pas produire d'index
 * négatifs.
 */
export function visibleRange(
  count: number,
  rowHeight: number,
  scrollTop: number,
  viewportHeight: number,
  overscan = 4,
): GridWindow {
  const hauteur = Math.max(1, rowHeight)
  const haut = Math.max(0, scrollTop)
  const start = Math.max(0, Math.floor(haut / hauteur) - overscan)
  const end = Math.min(
    Math.max(0, count),
    Math.ceil((haut + Math.max(0, viewportHeight)) / hauteur) + overscan,
  )
  const borne = Math.max(start, end)
  return {
    start,
    end: borne,
    paddingTop: start * hauteur,
    paddingBottom: Math.max(0, Math.max(0, count) - borne) * hauteur,
  }
}
