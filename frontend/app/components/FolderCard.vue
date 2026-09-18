<script setup lang="ts">
import type { DisplayMode, FileInfo, FolderInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

const props = withDefaults(
  defineProps<{
    folder: FolderInfo
    displayMode?: DisplayMode
    to?: string | null
    /**
     * Chemin relatif du dossier dans le catalogue (`Maison/Toit`). Les
     * sous-dossiers d'une page n'ont que leur nom dans `folder.name` : c'est
     * l'appelant qui sait où ils sont rangés.
     */
    rel?: string | null
  }>(),
  { displayMode: '3d', to: null, rel: null },
)

/** Chemin relatif : le nom du dossier, sauf quand on connaît mieux. */
const folderRel = computed(() => props.rel ?? props.folder.name)

/**
 * Destination : l'accueil pointe le dossier de premier niveau ; la page d'un
 * dossier réutilise la même carte pour ses sous-dossiers (chemin complet).
 */
const href = computed(() => props.to ?? folderHref(props.folder.name))

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
/**
 * L'aperçu du dossier est emprunté à l'un de ses fichiers : s'il ne charge pas
 * (fichier déplacé depuis le scan), on montre le repli au lieu d'une image
 * cassée.
 */
const imageFailed = ref(false)
watch(
  () => preview.value?.image,
  () => (imageFailed.value = false),
)

// ── Menu contextuel (clic droit) ────────────────────────────────────────────
const { t } = useI18n()
const menu = reactive({ open: false, x: 0, y: 0 })

function openMenu(event: MouseEvent) {
  menu.open = true
  menu.x = event.clientX
  menu.y = event.clientY
}

const menuItems = computed(() => [
  { key: 'rename', label: t('rename.title'), icon: 'i-lucide-pencil' },
  { key: 'delete', label: t('delete.title'), icon: 'i-lucide-trash-2', danger: true },
])

const { start: startRename } = useRename()
const { start: startDelete } = useDelete()

function onMenuSelect(key: string) {
  menu.open = false
  // `count` : ce que la suppression emporte, annoncé avant de confirmer.
  const cible = {
    rel: folderRel.value,
    label: props.folder.name,
    kind: 'folder' as const,
    count: props.folder.count,
  }

  if (key === 'rename') startRename(cible)
  if (key === 'delete') startDelete(cible)
}
</script>

<template>
  <article class="model-card model-card--file" @contextmenu.prevent="openMenu">
    <NuxtLink :to="href" class="model-card__link">
      <div class="model-card__preview">
        <img
          v-if="showImage && !imageFailed"
          :src="fileUrl(preview!.image!, preview!.image_version)"
          :alt="folder.name"
          class="model-card__img"
          loading="lazy"
          @error="imageFailed = true"
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

    <CardMenu
      :open="menu.open"
      :x="menu.x"
      :y="menu.y"
      :items="menuItems"
      :label="folder.name"
      @select="onMenuSelect"
      @close="menu.open = false"
    />
  </article>
</template>
