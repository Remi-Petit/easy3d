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
const { hasPreview, viewerOf, showsViewer } = useFormats()

/**
 * Aperçu du dossier : on emprunte l'image d'un fichier qu'il contient — celle
 * du premier fichier qui a un aperçu, sinon celle du premier maillage, sinon
 * celle du premier fichier dont le format est prévisualisable par le backend.
 * `null` si rien n'est représentable.
 */
const preview = computed<FileInfo | null>(() => {
  const files = props.folder.files
  return (
    files.find((f) => f.image) ??
    files.find((f) => viewerOf(f.rel) === 'mesh') ??
    files.find((f) => hasPreview(f.rel)) ??
    null
  )
})

/** Aperçu statique (mode `image`) ou vignette 3D (mode `3d`), comme les cartes
 * de fichier. */
const showImage = computed(() => props.displayMode === 'image' && !!preview.value?.image)
const show3d = computed(() =>
  preview.value ? showsViewer(preview.value.rel, props.displayMode) : false,
)
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
