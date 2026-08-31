<script setup lang="ts">
import type { DisplayMode, FileInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

const props = withDefaults(
  defineProps<{ file: FileInfo; displayMode?: DisplayMode }>(),
  { displayMode: '3d' },
)

const now = useNow()
const name = computed(() => basename(props.file.path))
const type = computed(() => ext(props.file.path) || '?')
const when = computed(() => timeAgo(toDate(props.file.modified), now.value))
const isModel = computed(() => ['stl', 'obj'].includes(type.value))
const isGcode = computed(() => ['gcode', 'gco'].includes(type.value))
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
        <ModelThumbnail v-else-if="isModel" :rel="file.rel" />
        <div v-else :class="['model-card__ph', isGcode ? 'ph--gcode' : 'ph--other']">
          <span class="ph-badge">{{ isGcode ? '🖨 GCODE' : type }}</span>
        </div>
      </div>

      <div class="model-card__body">
        <span class="model-card__name" :title="file.path">{{ name }}</span>
        <span class="model-card__meta">{{ type.toUpperCase() }} · {{ when }}</span>
      </div>
    </NuxtLink>
  </article>
</template>
