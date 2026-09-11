<script setup lang="ts">
import type { DisplayMode, FileInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

const props = withDefaults(
  defineProps<{ file: FileInfo; displayMode?: DisplayMode; folder?: string | null }>(),
  { displayMode: '3d', folder: null },
)

const now = useNow()
const name = computed(() => basename(props.file.path))
const type = computed(() => ext(props.file.path) || '?')
const when = computed(() => timeAgo(toDate(props.file.modified), now.value))
const isModel = computed(() => ['stl', 'obj', '3mf'].includes(type.value))
const isGcode = computed(() => ['gcode', 'gco'].includes(type.value))
/**
 * Rendu 3D interactif : toujours pour un modèle ; pour un G-code seulement en
 * mode `3d` (en mode `image` on préfère l'aperçu statique du slicer).
 */
const show3d = computed(
  () => isModel.value || (isGcode.value && props.displayMode === '3d'),
)
/** Chemin relatif encodé -> URL `/fichier/[rel]`. */
const href = computed(() => `/fichier/${encodeURIComponent(props.file.rel)}`)

/** Aperçu statique image si le mode "image" est actif et qu'une image existe. */
const showImage = computed(() => props.displayMode === 'image' && !!props.file.image)
</script>

<template>
  <article class="model-card" :class="{ 'model-card--file': !isModel && !isGcode }">
    <NuxtLink :to="href" class="model-card__link">
      <div class="model-card__preview">
        <img
          v-if="showImage"
          :src="fileUrl(file.image!)"
          :alt="name"
          class="model-card__img"
          loading="lazy"
        />
        <ModelThumbnail v-else-if="show3d" :rel="file.rel" />
        <div v-else :class="['model-card__ph', isGcode ? 'ph--gcode' : 'ph--other']">
          <span class="ph-badge">{{ isGcode ? '🖨 GCODE' : type }}</span>
        </div>
        <span v-if="file.note" class="note-badge" title="Ce fichier a une note">📝</span>
      </div>

      <div class="model-card__body">
        <span class="model-card__name" :title="file.path">{{ name }}</span>
        <span class="model-card__meta">
          {{ type.toUpperCase() }}<template v-if="folder"> · 📁 {{ folder }}</template> · {{ when }}
        </span>
      </div>
    </NuxtLink>
  </article>
</template>
