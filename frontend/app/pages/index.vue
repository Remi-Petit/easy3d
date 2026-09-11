<script setup lang="ts">
import type { FileInfo, FolderInfo } from '~/composables/useModels'

const { data, error, live } = useModels()

const query = ref('')

/** Une recherche est active dès que le champ contient du texte. */
const searching = computed(() => query.value.trim().length > 0)

/** Mode de tri par date : ordre d'origine, du plus ancien au plus récent, ou l'inverse. */
type SortMode = 'none' | 'date-asc' | 'date-desc'
const sortMode = ref<SortMode>('none')

/** Ordre de rotation au clic : normal -> récent → ancien -> ancien → récent -> normal. */
const SORT_CYCLE: SortMode[] = ['none', 'date-desc', 'date-asc']

/** Passe au mode de tri suivant. */
function cycleSort() {
  const i = SORT_CYCLE.indexOf(sortMode.value)
  sortMode.value = SORT_CYCLE[(i + 1) % SORT_CYCLE.length] ?? 'none'
}

/** Description du mode courant, utilisée pour l'infobulle et l'accessibilité. */
const sortLabel = computed(
  () =>
    ({
      none: 'date (ordre normal)',
      'date-desc': 'date : récent → ancien',
      'date-asc': 'date : ancien → récent',
    })[sortMode.value],
)

/** Flèche : ↕ = normal, ↓ = récent → ancien, ↑ = ancien → récent. */
const sortIcon = computed(
  () => ({ none: '↕', 'date-desc': '↓', 'date-asc': '↑' })[sortMode.value],
)

/** Trie une liste de fichiers par date de modification (les dates absentes vont en fin). */
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

/** Fichier enrichi du nom du dossier d'origine (absent si le fichier est à la racine). */
type SearchFile = FileInfo & { folder?: string }

/** Vue par défaut (aucune recherche) : tous les dossiers. */
const allFolders = computed<FolderInfo[]>(() =>
  data.value ? Object.values(data.value.folders) : [],
)

/** Vue par défaut (aucune recherche) : fichiers à la racine. */
const rootFiles = computed<FileInfo[]>(() => data.value?.files ?? [])

/** Fichiers racine triés selon `sortMode`. */
const sortedRootFiles = computed<FileInfo[]>(() => sortFiles(rootFiles.value))

/** Recherche : dossiers dont le nom correspond. */
const matchedFolders = computed<FolderInfo[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !q) return []
  return allFolders.value.filter((f) => f.name.toLowerCase().includes(q))
})

/** Recherche : fichiers dont le nom correspond, à la racine ET dans les dossiers. */
const matchedFiles = computed<SearchFile[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !q) return []
  const match = (f: FileInfo) => basename(f.path).toLowerCase().includes(q)
  const nested: SearchFile[] = Object.entries(data.value.folders).flatMap(
    ([name, folder]) => folder.files.filter(match).map((f) => ({ ...f, folder: name })),
  )
  return [...data.value.files.filter(match), ...nested]
})

const resultCount = computed(() => matchedFolders.value.length + matchedFiles.value.length)

/** Résultats de recherche (fichiers) triés selon `sortMode`. */
const sortedMatchedFiles = computed<SearchFile[]>(() => sortFiles(matchedFiles.value))

const totalCount = computed(() => data.value?.count ?? 0)
// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: 'catalogue de modèles · STL / 3MF / GCODE',
  count: totalCount.value,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <div class="toolbar">
    <div class="search">
      <span class="icon">🔍</span>
      <input v-model="query" type="text" placeholder="Filtrer par nom de fichier ou de dossier…" />
    </div>
    <button
      type="button"
      class="sort"
      :class="`sort--${sortMode}`"
      :title="`Trier par ${sortLabel} (cliquer pour changer)`"
      :aria-label="`Trier par ${sortLabel}`"
      @click="cycleSort"
    >
      <span class="sort__icon">{{ sortIcon }}</span>
      <span class="sort__label">Date</span>
    </button>
  </div>

  <div v-if="error" class="error">{{ error }}</div>

  <template v-if="data">
    <!-- Recherche active : une seule section « All » regroupant dossiers + fichiers. -->
    <template v-if="searching">
      <p class="section-label">All ({{ resultCount }})</p>
      <div v-if="resultCount" class="file-grid">
        <FolderCard v-for="f in matchedFolders" :key="`folder:${f.name}`" :folder="f" />
        <FileItem
          v-for="f in sortedMatchedFiles"
          :key="f.path"
          :file="f"
          :folder="f.folder"
          :display-mode="displayMode"
        />
      </div>
      <div v-else class="empty">Aucun résultat pour « {{ query.trim() }} ».</div>
    </template>

    <!-- Vue par défaut : Dossiers + Racine. -->
    <template v-else>
      <p class="section-label">Dossiers</p>
      <div v-if="allFolders.length" class="file-grid">
        <FolderCard v-for="f in allFolders" :key="f.name" :folder="f" />
      </div>
      <div v-else class="empty">Aucun dossier trouvé.</div>

      <p class="section-label">Racine ({{ rootFiles.length }})</p>
      <article class="card" v-if="rootFiles.length">
        <div class="file-grid">
          <FileItem v-for="f in sortedRootFiles" :key="f.path" :file="f" :display-mode="displayMode" />
        </div>
      </article>
      <div v-else class="empty">Aucun fichier à la racine.</div>
    </template>
  </template>

  <div v-else class="empty">Chargement…</div>
</template>
