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

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: `${total.value} fichier${total.value > 1 ? 's' : ''} · STL / 3MF / GCODE`,
  count: 0,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <div v-if="error" class="error">{{ error }}</div>

  <div v-if="folder" class="file-grid">
    <FileItem v-for="f in folder.files" :key="f.path" :file="f" :display-mode="displayMode" />
    <p v-if="!folder.files.length" class="empty">Dossier vide</p>
  </div>
  <div v-else class="empty">Chargement…</div>
</template>
