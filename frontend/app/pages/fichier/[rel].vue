<script setup lang="ts">
import type { FileInfo } from '~/composables/useModels'

const route = useRoute()
const { data, error, live } = useModels()

// Chemin relatif encodé (ex : `DemaAuto%2Fboitier.stl`) -> décodé.
const rel = computed(() => decodeURIComponent(String(route.params.rel)))

// Métadonnées depuis le scan (racine + tous les dossiers).
const file = computed<FileInfo | null>(() => {
  if (!data.value) return null
  return (
    data.value.files.find((f) => f.rel === rel.value) ??
    Object.values(data.value.folders)
      .flatMap((folder) => folder.files)
      .find((f) => f.rel === rel.value) ??
    null
  )
})

const name = computed(() => basename(rel.value))
const type = computed(() => ext(rel.value) || '?')
const isModel = computed(() => ['stl', 'obj'].includes(type.value))
const when = computed(() => timeAgo(toDate(file.value?.modified ?? null)))
</script>

<template>
  <div class="container detail">
    <NuxtLink to="/" class="back">← Retour</NuxtLink>

    <header class="header">
      <div class="brand">
        <div class="logo">{{ isModel ? '🧊' : '📄' }}</div>
        <div>
          <h1>{{ name }}</h1>
          <small>{{ type.toUpperCase() }} · {{ when }}</small>
        </div>
      </div>
      <div v-if="live" class="stats">
        <span class="pill"><span class="dot live" /> temps réel</span>
      </div>
    </header>

    <div v-if="error" class="error">{{ error }}</div>

    <div class="viewer-card">
      <ModelViewer v-if="isModel" :rel="rel" show-info :auto-rotate="false" />
      <div v-else class="file-detail-placeholder">
        <span class="ph-badge">{{ type.toUpperCase() }}</span>
        <p>Aperçu 3D non disponible pour ce type de fichier.</p>
      </div>
    </div>

    <dl class="meta-table">
      <div class="meta-row"><dt>Fichier</dt><dd>{{ name }}</dd></div>
      <div class="meta-row"><dt>Type</dt><dd>{{ type.toUpperCase() }}</dd></div>
      <div class="meta-row"><dt>Chemin</dt><dd :title="rel">{{ rel }}</dd></div>
      <div class="meta-row"><dt>Modifié</dt><dd>{{ when }}</dd></div>
    </dl>
  </div>
</template>
