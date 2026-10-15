<script setup lang="ts">
import type { FileInfo } from '~/composables/useModels'
import { useNow } from '~/composables/useNow'
import { formatBytes, formatDuration, hasPrintFacts, type PrintInfo } from '~/utils/print'

const route = useRoute()
const { data, error, live } = useModels()
const { t, n, locale } = useI18n()

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
const { viewerOf } = useFormats()
/** Visionneuse du format : `mesh`, `gcode` ou `none` (format sans rendu). */
const viewer = computed(() => viewerOf(rel.value))
const when = useTimeAgo(() => file.value?.modified ?? null)
/**
 * La page détail fait exception au réglage global : tout fichier dont le format
 * a une visionneuse (maillage STL / OBJ / 3MF, G-code) y est **toujours** rendu
 * en 3D, même si le catalogue est réglé sur « aperçu image ». Le mode `image`
 * ne concerne donc que les vignettes.
 */
const showViewer = computed(() => viewer.value !== 'none')

// ── Informations d'impression ─────────────────────────────────────────────
/**
 * Ce que le slicer annonce en tranchant le fichier : durée, matière, poids…
 *
 * Demandées **à part** du catalogue, parce que ça se lit dans le fichier
 * (plusieurs mégaoctets pour un G-code) et que seul cet écran en a besoin. Un
 * fichier qui n'annonce rien — un STL, un 3MF exporté sans tranchage — répond
 * `{}` : la carte n'apparaît alors pas du tout.
 */
const printInfo = ref<PrintInfo | null>(null)

async function chargerPrintInfo() {
  try {
    const info = await $fetch<PrintInfo>('/api/print-info', { query: { path: rel.value } })
    printInfo.value = hasPrintFacts(info) ? info : null
  } catch {
    // Une information d'impression manquante ne casse pas la page : les
    // propriétés du fichier, elles, sont déjà là.
    printInfo.value = null
  }
}

onMounted(() => void chargerPrintInfo())
// La page est réutilisée d'un fichier à l'autre (même route, autre paramètre) :
// sans ce `watch`, on garderait les valeurs du fichier précédent.
watch(rel, () => {
  printInfo.value = null
  void chargerPrintInfo()
})

/**
 * Encombrement lisible, dans la langue de l'interface.
 *
 * Le backend donne `[X, Y, Z]` dans le repère **du fichier** (Z vers le haut,
 * comme un G-code) ; la visionneuse, elle, affiche « largeur × hauteur ×
 * profondeur » — elle tourne les fichiers Z-up en Y-up. On présente donc dans ce
 * même ordre, sans quoi les deux nombres de la page se liraient à l'envers l'un
 * de l'autre.
 */
const encombrement = computed(() => {
  const [x, y, z] = printInfo.value?.size_mm ?? []
  if (x === undefined || y === undefined || z === undefined) return ''
  return [x, z, y].map((valeur) => n(valeur)).join(' × ')
})

// En-tête global (rendu par le layout `default`).
usePageHeader(() => ({
  subtitle: t('file.subtitle', { type: type.value.toUpperCase(), when: when.value }),
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
      {{ $t('common.download') }}
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
        <p>{{ $t('file.noPreview') }}</p>
      </div>
    </div>

    <aside class="detail__side">
      <dl class="meta-table">
        <div class="meta-row"><dt>{{ $t('file.metaFile') }}</dt><dd>{{ name }}</dd></div>
        <div class="meta-row"><dt>{{ $t('file.metaType') }}</dt><dd>{{ type.toUpperCase() }}</dd></div>
        <div class="meta-row"><dt>{{ $t('file.metaPath') }}</dt><dd :title="rel">{{ rel }}</dd></div>
        <div class="meta-row"><dt>{{ $t('file.metaModified') }}</dt><dd>{{ when }}</dd></div>
      </dl>

      <!--
        Ce que le slicer annonce sur le fichier, sous les propriétés du fichier :
        durée, matière, poids, hauteur de couche… Le poids de filament n'est
        affiché que si le slicer l'écrit (QIDISlicer ne le fait pas) — calculer
        une estimation à partir des longueurs d'extrusion demanderait la densité
        de la matière, et se tromperait sans le dire.
      -->
      <section v-if="printInfo" class="print">
        <h2 class="print__title">{{ $t('print.title') }}</h2>
        <dl class="meta-table">
          <div v-if="printInfo.duration_seconds" class="meta-row">
            <dt>{{ $t('print.duration') }}</dt>
            <dd>{{ formatDuration(printInfo.duration_seconds) }}</dd>
          </div>
          <div v-if="printInfo.material" class="meta-row">
            <dt>{{ $t('print.material') }}</dt>
            <dd>{{ printInfo.material }}</dd>
          </div>
          <div v-if="printInfo.filament_grams" class="meta-row">
            <dt>{{ $t('print.weight') }}</dt>
            <dd>{{ n(printInfo.filament_grams) }} g</dd>
          </div>
          <div v-if="printInfo.layer_height" class="meta-row">
            <dt>{{ $t('print.layerHeight') }}</dt>
            <dd>{{ printInfo.layer_height }} mm</dd>
          </div>
          <div v-if="printInfo.first_layer_height" class="meta-row">
            <dt>{{ $t('print.firstLayer') }}</dt>
            <dd>{{ printInfo.first_layer_height }} mm</dd>
          </div>
          <div v-if="printInfo.nozzle" class="meta-row">
            <dt>{{ $t('print.nozzle') }}</dt>
            <dd>{{ printInfo.nozzle }} mm</dd>
          </div>
          <div v-if="printInfo.size_mm" class="meta-row">
            <dt>{{ $t('print.size') }}</dt>
            <dd>{{ encombrement }} mm</dd>
          </div>
          <div v-if="printInfo.printer" class="meta-row">
            <dt>{{ $t('print.printer') }}</dt>
            <dd :title="printInfo.printer">{{ printInfo.printer }}</dd>
          </div>
          <div v-if="printInfo.fill_density" class="meta-row">
            <dt>{{ $t('print.fill') }}</dt>
            <dd>{{ printInfo.fill_density }}</dd>
          </div>
          <div v-if="printInfo.support !== undefined" class="meta-row">
            <dt>{{ $t('print.support') }}</dt>
            <dd>{{ printInfo.support ? $t('common.yes') : $t('common.no') }}</dd>
          </div>
          <div v-if="printInfo.slicer" class="meta-row">
            <dt>{{ $t('print.slicer') }}</dt>
            <dd :title="printInfo.slicer">{{ printInfo.slicer }}</dd>
          </div>
          <div v-if="printInfo.bytes" class="meta-row">
            <dt>{{ $t('print.fileSize') }}</dt>
            <dd>{{ formatBytes(printInfo.bytes, locale) }}</dd>
          </div>
        </dl>
      </section>

      <!-- Note du fichier (Markdown) : affichage + édition assistée. -->
      <NotePanel v-if="file" :key="rel" :rel="rel" :note="file.note" />
    </aside>
  </div>
</template>
