<script setup lang="ts">
import type { FolderInfo } from '~/composables/useModels'

const route = useRoute()
const { data, error, live } = useModels()

// Le nom est encodé dans l'URL ; on le décode pour retrouver la clé du dossier.
const name = computed(() => decodeURIComponent(String(route.params.name)))
const folder = computed<FolderInfo | null>(() => {
  if (!data.value) return null
  return data.value.folders[name.value] ?? null
})
const total = computed(() => folder.value?.count ?? 0)
// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')

// Filtre global : barre rendue par le layout `default`, état partagé.
const { filtering, matches, sortFiles } = useFilter()

/** Les formats proposés sont ceux du dossier courant. */
useFilterTypes(() => folder.value?.files ?? [])

/** Fichiers du dossier, filtrés (recherche + type) puis triés. */
const files = computed(() => sortFiles((folder.value?.files ?? []).filter(matches)))

// En-tête global (rendu par le layout `default`).
usePageHeader(() => {
  const suffix = total.value > 1 ? 's' : ''
  return {
    subtitle: filtering.value
      ? `${files.value.length} / ${total.value} fichier${suffix} · STL / 3MF / GCODE`
      : `${total.value} fichier${suffix} · STL / 3MF / GCODE`,
    count: 0,
    live: live.value,
    offline: !!error.value,
  }
})
</script>

<template>
  <BackLink />

  <div v-if="error" class="error">{{ error }}</div>

  <!-- Note du dossier (Markdown) : affichage + édition assistée. -->
  <NotePanel v-if="folder" :key="name" :rel="name" :note="folder.note" />

  <div v-if="folder" class="file-grid">
    <FileItem v-for="f in files" :key="f.path" :file="f" :display-mode="displayMode" />
    <p v-if="!files.length" class="empty">
      {{ filtering ? 'Aucun fichier ne correspond au filtre.' : 'Dossier vide' }}
    </p>
  </div>
  <div v-else class="empty">Chargement…</div>
</template>
