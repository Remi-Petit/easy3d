import type { FileInfo } from '~/composables/useModels'

/** Mode de tri par date : ordre d'origine, du plus ancien au plus récent, ou l'inverse. */
export type SortMode = 'none' | 'date-asc' | 'date-desc'

/** Ordre de rotation au clic : normal -> récent → ancien -> ancien → récent -> normal. */
const SORT_CYCLE: SortMode[] = ['none', 'date-desc', 'date-asc']

/** Description du mode, utilisée pour l'infobulle et l'accessibilité. */
const SORT_LABELS: Record<SortMode, string> = {
  none: 'date (ordre normal)',
  'date-desc': 'date : récent → ancien',
  'date-asc': 'date : ancien → récent',
}

/** Flèche : ↕ = normal, ↓ = récent → ancien, ↑ = ancien → récent. */
const SORT_ICONS: Record<SortMode, string> = {
  none: '↕',
  'date-desc': '↓',
  'date-asc': '↑',
}

/**
 * Filtre global (recherche + tri par date).
 *
 * L'état est partagé entre la barre rendue par le layout `default` et les pages
 * qui listent des fichiers ; il est donc conservé d'une page à l'autre.
 */
export function useFilter() {
  const query = useState('filter:query', () => '')
  const sortMode = useState<SortMode>('filter:sort', () => 'none')

  /** Une recherche est active dès que le champ contient du texte. */
  const searching = computed(() => query.value.trim().length > 0)
  const sortLabel = computed(() => SORT_LABELS[sortMode.value])
  const sortIcon = computed(() => SORT_ICONS[sortMode.value])

  /** Passe au mode de tri suivant. */
  function cycleSort() {
    const i = SORT_CYCLE.indexOf(sortMode.value)
    sortMode.value = SORT_CYCLE[(i + 1) % SORT_CYCLE.length] ?? 'none'
  }

  /** `true` si le fichier correspond à la recherche courante (ou si elle est vide). */
  function matches(file: FileInfo): boolean {
    const q = query.value.trim().toLowerCase()
    return !q || basename(file.path).toLowerCase().includes(q)
  }

  /** Trie par date de modification (les dates absentes vont en fin de liste). */
  function sortFiles<T extends FileInfo>(files: T[]): T[] {
    if (sortMode.value === 'none') return files
    const dir = sortMode.value === 'date-asc' ? 1 : -1
    return [...files].sort((a, b) => {
      if (a.modified == null && b.modified == null) return 0
      if (a.modified == null) return 1
      if (b.modified == null) return -1
      return (a.modified - b.modified) * dir
    })
  }

  return { query, sortMode, searching, sortLabel, sortIcon, cycleSort, matches, sortFiles }
}
