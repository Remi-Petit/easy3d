<script setup lang="ts">
import type { FileInfo, FolderInfo } from '~/composables/useModels'

const { data, error, live } = useModels()

// Filtre global : barre rendue par le layout `default`, état partagé.
const { query, searching, matches, sortFiles } = useFilter()

/** Fichier enrichi du nom du dossier d'origine (absent si le fichier est à la racine). */
type SearchFile = FileInfo & { folder?: string }

/** Vue par défaut (aucune recherche) : tous les dossiers. */
const allFolders = computed<FolderInfo[]>(() =>
  data.value ? Object.values(data.value.folders) : [],
)

/** Vue par défaut (aucune recherche) : fichiers à la racine. */
const rootFiles = computed<FileInfo[]>(() => data.value?.files ?? [])

/** Fichiers racine triés selon le mode de tri partagé. */
const sortedRootFiles = computed<FileInfo[]>(() => sortFiles(rootFiles.value))

/** Recherche : dossiers dont le nom correspond. */
const matchedFolders = computed<FolderInfo[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !q) return []
  return allFolders.value.filter((f) => f.name.toLowerCase().includes(q))
})

/** Recherche : fichiers dont le nom correspond, à la racine ET dans les dossiers. */
const matchedFiles = computed<SearchFile[]>(() => {
  if (!data.value || !searching.value) return []
  const nested: SearchFile[] = Object.entries(data.value.folders).flatMap(
    ([name, folder]) => folder.files.filter(matches).map((f) => ({ ...f, folder: name })),
  )
  return [...data.value.files.filter(matches), ...nested]
})

const resultCount = computed(() => matchedFolders.value.length + matchedFiles.value.length)

/** Résultats de recherche (fichiers) triés. */
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
