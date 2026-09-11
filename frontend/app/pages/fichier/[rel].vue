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
// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')
/** Aperçu statique image si le mode "image" est actif et qu'une image existe. */
const showDetailImage = computed(() => displayMode.value === 'image' && !!file.value?.image)
/**
 * Viewer 3D : toujours pour un modèle ; pour un G-code seulement en mode `3d`
 * (en mode `image` on préfère l'aperçu statique extrait du fichier).
 */
const showViewer = computed(
  () => isModel.value || (isGcode.value && displayMode.value === '3d'),
)

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: `${type.value.toUpperCase()} · ${when.value}`,
  count: 0,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <BackLink />

  <div v-if="error" class="error">{{ error }}</div>

  <div class="viewer-card">
    <img
      v-if="showDetailImage"
      :src="fileUrl(file.image!)"
      :alt="name"
      class="model-card__img"
    />
    <ModelViewer v-else-if="showViewer" :rel="rel" show-info :auto-rotate="false" />
    <div v-else class="file-detail-placeholder">
      <span class="ph-badge">{{ type.toUpperCase() }}</span>
      <p>
        {{
          isGcode
            ? 'Aucun aperçu trouvé dans ce G-code.'
            : 'Aperçu 3D non disponible pour ce type de fichier.'
        }}
      </p>
    </div>
  </div>

  <dl class="meta-table">
    <div class="meta-row"><dt>Fichier</dt><dd>{{ name }}</dd></div>
    <div class="meta-row"><dt>Type</dt><dd>{{ type.toUpperCase() }}</dd></div>
    <div class="meta-row"><dt>Chemin</dt><dd :title="rel">{{ rel }}</dd></div>
    <div class="meta-row"><dt>Modifié</dt><dd>{{ when }}</dd></div>
  </dl>

  <!-- Note du fichier (Markdown) : affichage + édition assistée. -->
  <NotePanel v-if="file" :key="rel" :rel="rel" :note="file.note" />
</template>
