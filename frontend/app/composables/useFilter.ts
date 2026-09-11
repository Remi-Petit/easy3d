import type { FileInfo } from '~/composables/useModels'

/**
 * Filtre global (recherche + tri par date).
 *
 * L'état est partagé entre la barre rendue par le layout `default` et les pages
 * qui listent des fichiers ; il est donc conservé d'une page à l'autre.
 *
 * La logique elle-même vit dans `~/utils/filter` (pure et testée) ; ce composable
 * ne fait que l'envelopper dans des refs partagés.
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
    sortMode.value = nextSortMode(sortMode.value)
  }

  /** `true` si le fichier correspond à la recherche courante (ou si elle est vide). */
  function matches(file: FileInfo): boolean {
    return matchesQuery(file, query.value)
  }

  /** Trie par date de modification selon le mode courant. */
  function sortFiles<T extends FileInfo>(files: T[]): T[] {
    return sortFilesByDate(files, sortMode.value)
  }

  return { query, sortMode, searching, sortLabel, sortIcon, cycleSort, matches, sortFiles }
}
