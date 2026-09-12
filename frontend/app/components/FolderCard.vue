<script setup lang="ts">
import type { DisplayMode, FileInfo, FolderInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

const props = withDefaults(
  defineProps<{ folder: FolderInfo; displayMode?: DisplayMode }>(),
  { displayMode: '3d' },
)

/** Nom du dossier encodé -> URL `/dossiers/[name]`. */
const href = computed(() => `/dossiers/${encodeURIComponent(props.folder.name)}`)

const now = useNow()
/** Dernier changement du dossier (ajout, suppression, renommage…). */
const when = useTimeAgo(() => props.folder.modified)

function isModel(rel: string) {
  return ['stl', 'obj', '3mf'].includes(ext(rel))
}

function isGcode(rel: string) {
  return ['gcode', 'gco'].includes(ext(rel))
}

/**
 * Aperçu du dossier : on emprunte l'image d'un fichier qu'il contient — celle
 * du premier fichier qui a un aperçu, sinon la vignette 3D du premier modèle,
 * sinon celle du premier G-code. `null` si rien n'est représentable.
 */
const preview = computed<FileInfo | null>(() => {
  const files = props.folder.files
  return (
    files.find((f) => f.image) ??
    files.find((f) => isModel(f.rel)) ??
    files.find((f) => isGcode(f.rel)) ??
    null
  )
})

/** Aperçu statique (mode `image`) ou vignette 3D (mode `3d`), comme les cartes
 * de fichier. */
const showImage = computed(() => props.displayMode === 'image' && !!preview.value?.image)
const show3d = computed(() => {
  const rel = preview.value?.rel
  if (!rel) return false
  return isModel(rel) || (isGcode(rel) && props.displayMode === '3d')
})
</script>

<template>
  <article class="model-card model-card--file">
    <NuxtLink :to="href" class="model-card__link">
      <div class="model-card__preview">
        <img
          v-if="showImage"
          :src="fileUrl(preview!.image!)"
          :alt="folder.name"
          class="model-card__img"
          loading="lazy"
        />
        <ModelThumbnail v-else-if="show3d" :rel="preview!.rel" />
        <div v-else class="model-card__ph ph--other">
          <span class="folder-icon">📁</span>
        </div>
        <span class="folder-count">{{ folder.count }}</span>
        <span v-if="folder.note" class="note-badge" :title="$t('folder.hasNote')">📝</span>
      </div>
      <div class="model-card__body">
        <span class="model-card__name" :title="folder.name">{{ folder.name }}</span>
        <span class="model-card__meta">
          {{ $t('folder.meta', { count: folder.count, when }, folder.count) }}
        </span>
      </div>
    </NuxtLink>
  </article>
</template>
