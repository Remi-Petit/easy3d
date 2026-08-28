<script setup lang="ts">
import type { FolderInfo } from '~/composables/useModels'

defineProps<{ folder: FolderInfo }>()
</script>

<template>
  <article class="card">
    <div class="card-head">
      <span class="card-title">📁 {{ folder.name }}</span>
      <span class="badge">{{ folder.count }}</span>
    </div>
    <div class="file-grid">
      <FileItem v-for="f in folder.files" :key="f.path" :file="f" />
      <p v-if="!folder.count" class="empty">dossier vide</p>
    </div>
  </article>
</template>
