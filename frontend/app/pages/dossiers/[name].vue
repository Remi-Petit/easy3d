<script setup lang="ts">
import type { FolderInfo } from '~/composables/useModels'

const route = useRoute()
const { data, error, live } = useModels()
const { t } = useI18n()

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
  const count = total.value
  const shown = files.value.length
  return {
    subtitle: filtering.value
      ? t('folder.subtitleFiltered', { shown, count }, count)
      : t('folder.subtitle', count, { count }),
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
      {{ filtering ? $t('folder.noMatch') : $t('folder.empty') }}
    </p>
  </div>
  <div v-else class="empty">{{ $t('common.loading') }}</div>
</template>
