<script setup lang="ts">
import type { DisplayMode, FileInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

const props = withDefaults(
  defineProps<{ file: FileInfo; displayMode?: DisplayMode; folder?: string | null }>(),
  { displayMode: '3d', folder: null },
)

const now = useNow()
const { t } = useI18n()
const { viewerOf, showsViewer } = useFormats()
const name = computed(() => basename(props.file.path))
const type = computed(() => ext(props.file.path) || '?')
const when = useTimeAgo(() => props.file.modified)
/** Visionneuse du format de ce fichier : `mesh`, `gcode` ou `none` (inconnu). */
const viewer = computed(() => viewerOf(props.file.rel))
const isGcode = computed(() => viewer.value === 'gcode')
/**
 * Rendu 3D interactif : toujours pour un maillage ; pour un G-code seulement en
 * mode `3d` (en mode `image` on préfère l'aperçu statique du slicer).
 */
const show3d = computed(() => showsViewer(props.file.rel, props.displayMode))
/** Chemin relatif encodé -> URL `/fichier/[rel]`. */
const href = computed(() => `/fichier/${encodeURIComponent(props.file.rel)}`)

/** Aperçu statique image si le mode "image" est actif et qu'une image existe. */
const showImage = computed(() => props.displayMode === 'image' && !!props.file.image)
/**
 * Une vignette annoncée peut manquer (aucun aperçu exploitable dans le fichier,
 * aperçu supprimé depuis le scan) : on bascule sur le repli plutôt que de
 * laisser une image cassée et une erreur dans la console du navigateur.
 */
const imageFailed = ref(false)
watch(
  () => props.file.image,
  () => (imageFailed.value = false),
)

// ── Menu contextuel (clic droit) ────────────────────────────────────────────
// La position est celle du curseur : c'est la carte qui la retient, le menu ne
// fait que s'y poser (voir `CardMenu`).
//
// `can` vient de `useAuth()` — ce n'est pas une auto-import : sans cet appel, le
// gabarit référence une fonction qui n'existe pas.
const { can } = useAuth()
const menu = reactive({ open: false, x: 0, y: 0 })

function openMenu(event: MouseEvent) {
  // Rien à proposer : un menu vide vaut moins qu'aucun menu.
  if (!menuItems.value.length) return
  menu.open = true
  menu.x = event.clientX
  menu.y = event.clientY
}

/**
 * Actions du menu contextuel, limitées à ce que le compte peut faire.
 *
 * Proposer un renommage que le serveur refusera serait un piège : l'utilisateur
 * remplirait une fenêtre pour rien.
 */
const menuItems = computed(() => {
  const items = []
  if (can(PERM.modelRename)) {
    items.push({ key: 'rename', label: t('rename.title'), icon: 'i-lucide-pencil' })
  }
  if (can(PERM.modelDelete)) {
    items.push({ key: 'delete', label: t('delete.title'), icon: 'i-lucide-trash-2', danger: true })
  }
  return items
})

const { start: startRename } = useRename()
const { start: startDelete } = useDelete()

function onMenuSelect(key: string) {
  menu.open = false
  const cible = { rel: props.file.rel, label: name.value, kind: 'file' as const }

  if (key === 'rename') startRename(cible)
  if (key === 'delete') startDelete(cible)
}
</script>

<template>
  <article
    class="model-card"
    :class="{ 'model-card--file': viewer === 'none' }"
    @contextmenu.prevent="openMenu"
  >
    <NuxtLink :to="href" class="model-card__link">
      <div class="model-card__preview">
        <img
          v-if="showImage && !imageFailed"
          :src="fileUrl(file.image!, file.image_version)"
          :alt="name"
          class="model-card__img"
          loading="lazy"
          @error="imageFailed = true"
        />
        <ModelThumbnail v-else-if="show3d" :rel="file.rel" />
        <div v-else :class="['model-card__ph', isGcode ? 'ph--gcode' : 'ph--other']">
          <span class="ph-badge">{{ isGcode ? $t('file.gcodeBadge') : type }}</span>
        </div>
        <span v-if="file.note" class="note-badge" :title="$t('file.hasNote')">📝</span>
      </div>

      <div class="model-card__body">
        <span class="model-card__name" :title="file.path">{{ name }}</span>
        <span class="model-card__meta">
          {{
            folder
              ? $t('file.metaInFolder', { type: type.toUpperCase(), folder, when })
              : $t('file.meta', { type: type.toUpperCase(), when })
          }}
        </span>
      </div>
    </NuxtLink>

    <CardMenu
      :open="menu.open"
      :x="menu.x"
      :y="menu.y"
      :items="menuItems"
      :label="name"
      @select="onMenuSelect"
      @close="menu.open = false"
    />
  </article>
</template>
