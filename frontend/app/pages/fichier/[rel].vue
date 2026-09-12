<script setup lang="ts">
import type { FileInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'

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

const now = useNow()
const name = computed(() => basename(rel.value))
const type = computed(() => ext(rel.value) || '?')
const isModel = computed(() => ['stl', 'obj', '3mf'].includes(type.value))
const isGcode = computed(() => ['gcode', 'gco'].includes(type.value))
const when = computed(() => timeAgo(toDate(file.value?.modified ?? null), now.value))
/**
 * La page détail fait exception au réglage global : un modèle (STL / OBJ / 3MF)
 * ou un G-code y est **toujours** rendu en 3D, même si le catalogue est réglé
 * sur « aperçu image ». Le mode `image` ne concerne donc que les vignettes.
 */
const showViewer = computed(() => isModel.value || isGcode.value)

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: `${type.value.toUpperCase()} · ${when.value}`,
  count: 0,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <!-- Actions du détail : retour à gauche, téléchargement à droite. -->
  <div class="detail-bar">
    <BackLink />
    <a class="download" :href="fileDownloadUrl(rel)" :download="name">
      <svg
        viewBox="0 0 24 24"
        width="15"
        height="15"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M12 3v12" />
        <path d="m7 10 5 5 5-5" />
        <path d="M5 21h14" />
      </svg>
      Télécharger
      <span class="download__size">{{ type.toUpperCase() }}</span>
    </a>
  </div>

  <div v-if="error" class="error">{{ error }}</div>

  <!-- Modèle à gauche, informations (métadonnées + note) à droite. -->
  <div class="detail">
    <div class="viewer-card">
      <ModelViewer v-if="showViewer" :rel="rel" show-info :auto-rotate="false" />
      <div v-else class="file-detail-placeholder">
        <span class="ph-badge">{{ type.toUpperCase() }}</span>
        <p>Aperçu 3D non disponible pour ce type de fichier.</p>
      </div>
    </div>

    <aside class="detail__side">
      <dl class="meta-table">
        <div class="meta-row"><dt>Fichier</dt><dd>{{ name }}</dd></div>
        <div class="meta-row"><dt>Type</dt><dd>{{ type.toUpperCase() }}</dd></div>
        <div class="meta-row"><dt>Chemin</dt><dd :title="rel">{{ rel }}</dd></div>
        <div class="meta-row"><dt>Modifié</dt><dd>{{ when }}</dd></div>
      </dl>

      <!-- Note du fichier (Markdown) : affichage + édition assistée. -->
      <NotePanel v-if="file" :key="rel" :rel="rel" :note="file.note" />
    </aside>
  </div>
</template>
