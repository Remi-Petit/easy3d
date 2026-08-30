<script setup lang="ts">
import type { FolderInfo } from '~/composables/useModels'

const props = defineProps<{ folder: FolderInfo }>()
/** Nom du dossier encodé -> URL `/dossiers/[name]`. */
const href = computed(() => `/dossiers/${encodeURIComponent(props.folder.name)}`)
</script>

<template>
  <article class="model-card model-card--file">
    <NuxtLink :to="href" class="model-card__link">
      <div class="model-card__preview">
        <div class="model-card__ph ph--other">
          <span class="folder-icon">📁</span>
        </div>
        <span class="folder-count">{{ folder.count }}</span>
      </div>
      <div class="model-card__body">
        <span class="model-card__name" :title="folder.name">{{ folder.name }}</span>
        <span class="model-card__meta">dossier · {{ folder.count }} fichier{{ folder.count > 1 ? 's' : '' }}</span>
      </div>
    </NuxtLink>
  </article>
</template>
