import type { FileInfo } from '~/composables/useModels'

/**
 * Filtre global (recherche, types de fichiers, tri par date).
 *
 * L'état est partagé entre la barre rendue par le layout `default` et les pages
 * qui listent des fichiers ; il est donc conservé d'une page à l'autre.
 *
 * La logique elle-même vit dans `~/utils/filter` (pure et testée) ; ce composable
 * ne fait que l'envelopper dans des refs partagés.
 */
export function useFilter() {
  const { t } = useI18n()
  const query = useState('filter:query', () => '')
  const sortMode = useState<SortMode>('filter:sort', () => 'none')
  /** Types sélectionnés (vide = aucun filtre de type). */
  const types = useState<string[]>('filter:types', () => [])
  /** Types proposés dans la vue courante, avec leur nombre. */
  const availableTypes = useState<TypeCount[]>('filter:availableTypes', () => [])

  /** Une recherche est active dès que le champ contient du texte. */
  const searching = computed(() => query.value.trim().length > 0)
  /** Au moins un type est sélectionné. */
  const hasTypeFilter = computed(() => types.value.length > 0)
  /** Un filtre quelconque est actif (texte ou type). */
  const filtering = computed(() => searching.value || hasTypeFilter.value)

  const sortLabel = computed(() => t(SORT_LABEL_KEYS[sortMode.value]))
  const sortIcon = computed(() => SORT_ICONS[sortMode.value])

  /** Passe au mode de tri suivant. */
  function cycleSort() {
    sortMode.value = nextSortMode(sortMode.value)
  }

  /** Ajoute ou retire un type de la sélection. */
  function toggle(type: string) {
    types.value = toggleType(types.value, type)
  }

  /** Retire le filtre de type (la recherche garde son texte). */
  function clearTypes() {
    types.value = []
  }

  /** `true` si le fichier passe la recherche **et** le filtre de type. */
  function matches(file: FileInfo): boolean {
    return matchesQuery(file, query.value) && matchesTypes(file, types.value)
  }

  /** Trie par date de modification selon le mode courant. */
  function sortFiles<T extends FileInfo>(files: T[]): T[] {
    return sortFilesByDate(files, sortMode.value)
  }

  return {
    query,
    sortMode,
    types,
    availableTypes,
    searching,
    hasTypeFilter,
    filtering,
    sortLabel,
    sortIcon,
    cycleSort,
    toggle,
    clearTypes,
    matches,
    sortFiles,
  }
}

/**
 * Déclare les formats présents dans la vue courante (appelé par les pages).
 *
 * « Vue courante » : tout le catalogue sur l'accueil, les fichiers du dossier
 * sur une page dossier. Comme pour l'en-tête, le remplissage est différé à
 * `onMounted` — le HTML serveur et le premier rendu client partagent ainsi
 * l'état initial, ce qui évite tout « hydration mismatch ».
 */
export function useFilterTypes(read: () => FileInfo[]) {
  const { types, availableTypes } = useFilter()

  onMounted(() => {
    watchEffect(() => {
      availableTypes.value = collectTypes(read())
    })
    // Un format sélectionné qui n'existe plus ici est retiré de la sélection.
    watch(availableTypes, (list) => {
      types.value = pruneTypes(
        types.value,
        list.map((entry) => entry.type),
      )
    })
  })
}
