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
      <FileItem v-if="folder.files[0]" :key="folder.files[0].path" :file="folder.files[0]" />
      <p v-if="!folder.count" class="empty">dossier vide</p>
    </div>
    <p v-if="folder.count > 1" class="card-more">+ {{ folder.count - 1 }} autres fichiers</p>
  </article>
</template>
