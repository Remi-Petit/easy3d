<script setup lang="ts">
import type { FileInfo, FolderInfo } from '~/composables/useModels'

const { data, error, live } = useModels()
const { t } = useI18n()

// Filtre global : barre rendue par le layout `default`, état partagé.
const { query, types, searching, hasTypeFilter, filtering, matches, sortFiles } = useFilter()

/** Fichier enrichi du nom du dossier d'origine (absent si le fichier est à la racine). */
type SearchFile = FileInfo & { folder?: string }

/** Vue par défaut (aucune recherche) : tous les dossiers. */
const allFolders = computed<FolderInfo[]>(() =>
  data.value ? Object.values(data.value.folders) : [],
)

/** Vue par défaut (aucune recherche) : fichiers à la racine. */
const rootFiles = computed<FileInfo[]>(() => data.value?.files ?? [])

/**
 * Tous les fichiers du catalogue : sert à proposer les formats à filtrer.
 * (La liste reste complète même pendant une recherche, sinon les puces
 * disparaîtraient au fur et à mesure de la frappe.)
 */
const allFiles = computed<FileInfo[]>(() => [
  ...rootFiles.value,
  ...allFolders.value.flatMap((folder) => folder.files),
])

useFilterTypes(() => allFiles.value)

/** Fichiers racine triés selon le mode de tri partagé. */
const sortedRootFiles = computed<FileInfo[]>(() => sortFiles(rootFiles.value))

/**
 * Vue filtrée : dossiers dont le nom correspond **et** qui contiennent au moins
 * un fichier du type sélectionné.
 */
const matchedFolders = computed<FolderInfo[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !filtering.value) return []
  return allFolders.value.filter((folder) => {
    if (q && !folder.name.toLowerCase().includes(q)) return false
    return folder.files.some((file) => matches(file))
  })
})

/** Vue filtrée : fichiers correspondants, à la racine ET dans les dossiers. */
const matchedFiles = computed<SearchFile[]>(() => {
  if (!data.value || !filtering.value) return []
  const nested: SearchFile[] = Object.entries(data.value.folders).flatMap(
    ([name, folder]) => folder.files.filter(matches).map((f) => ({ ...f, folder: name })),
  )
  return [...data.value.files.filter(matches), ...nested]
})

const resultCount = computed(() => matchedFolders.value.length + matchedFiles.value.length)

/** Message d'état vide, qui rappelle ce qui a été filtré. */
const emptyLabel = computed(() => {
  const parts: string[] = []
  if (searching.value) parts.push(`« ${query.value.trim()} »`)
  if (hasTypeFilter.value) parts.push(types.value.map((type) => type.toUpperCase()).join(', '))
  return parts.length ? t('catalog.noResultFor', { what: parts.join(' + ') }) : t('catalog.noResult')
})

/** Résultats de recherche (fichiers) triés. */
const sortedMatchedFiles = computed<SearchFile[]>(() => sortFiles(matchedFiles.value))

const totalCount = computed(() => data.value?.count ?? 0)
// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: t('catalog.subtitle'),
  count: totalCount.value,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <div v-if="error" class="error">{{ error }}</div>

  <template v-if="data">
    <!-- Filtre actif : une seule section « All » regroupant dossiers + fichiers. -->
    <template v-if="filtering">
      <p class="section-label">{{ $t('catalog.all', { count: resultCount }) }}</p>
      <div v-if="resultCount" class="file-grid">
        <FolderCard v-for="f in matchedFolders" :key="`folder:${f.name}`" :folder="f" :display-mode="displayMode" />
        <FileItem
          v-for="f in sortedMatchedFiles"
          :key="f.path"
          :file="f"
          :folder="f.folder"
          :display-mode="displayMode"
        />
      </div>
      <div v-else class="empty">{{ emptyLabel }}</div>
    </template>

    <!-- Vue par défaut : Dossiers + Racine. -->
    <template v-else>
      <p class="section-label">{{ $t('catalog.folders') }}</p>
      <div v-if="allFolders.length" class="file-grid">
        <FolderCard
          v-for="f in allFolders"
          :key="f.name"
          :folder="f"
          :rel="f.name"
          :display-mode="displayMode"
        />
      </div>
      <div v-else class="empty">{{ $t('catalog.noFolder') }}</div>

      <p class="section-label">{{ $t('catalog.root', { count: rootFiles.length }) }}</p>
      <div v-if="rootFiles.length" class="file-grid">
        <FileItem v-for="f in sortedRootFiles" :key="f.path" :file="f" :display-mode="displayMode" />
      </div>
      <div v-else class="empty">{{ $t('catalog.noRootFile') }}</div>
    </template>
  </template>

  <div v-else class="empty">{{ $t('common.loading') }}</div>
</template>
