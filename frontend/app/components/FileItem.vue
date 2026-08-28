<script setup lang="ts">
import type { FileInfo } from '~/composables/useModels'

const props = defineProps<{ file: FileInfo }>()

const name = computed(() => basename(props.file.path))
const type = computed(() => ext(props.file.path) || '?')
const when = computed(() => timeAgo(toDate(props.file.modified)))
const isModel = computed(() => ['stl', 'obj'].includes(type.value))
const isGcode = computed(() => ['gcode', 'gco'].includes(type.value))
const showModal = ref(false)
</script>

<template>
  <article class="model-card" :class="{ 'model-card--file': !isModel && !isGcode }">
    <div class="model-card__preview" @click="isModel && (showModal = true)">
      <ModelThumbnail v-if="isModel" :rel="file.rel" />
      <div v-else :class="['model-card__ph', isGcode ? 'ph--gcode' : 'ph--other']">
        <span class="ph-badge">{{ isGcode ? '🖨 GCODE' : type }}</span>
      </div>
    </div>

    <div class="model-card__body">
      <span class="model-card__name" :title="file.path">{{ name }}</span>
      <span class="model-card__meta">{{ type.toUpperCase() }} · {{ when }}</span>
    </div>

    <ModelModal
      v-if="showModal"
      :rel="file.rel"
      :name="name"
      @close="showModal = false"
    />
  </article>
</template>
