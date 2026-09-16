<script setup lang="ts">
/**
 * Ajout de fichiers au catalogue depuis l'interface.
 *
 * Le bouton ouvre une fenêtre de **dépôt** : on y glisse des fichiers ou un
 * dossier entier (l'arborescence est reconstruite à partir des chemins relatifs).
 * Le chemin « parcourir » reste là en secours — un glisser-déposer est
 * impossible au clavier — mais il n'est plus le chemin principal.
 *
 * L'envoi passe par `POST /api/upload` (voir `useUpload`) ; le backend écrit le
 * fichier, le watcher le détecte et rediffuse le catalogue — la liste se met
 * donc à jour toute seule.
 */
const props = withDefaults(defineProps<{ folder?: string | null }>(), { folder: null })

const { t } = useI18n()
const { running, total, done, current, upload } = useUpload()

const open = ref(false)
/** Un fichier survole la zone de dépôt de la fenêtre. */
const over = ref(false)

const filesInput = ref<HTMLInputElement | null>(null)
const folderInput = ref<HTMLInputElement | null>(null)

/** Dossier d'accueil : celui de la page courante, sinon la racine du catalogue. */
const target = computed(() => props.folder ?? '')

/** Destination, telle qu'annoncée dans la fenêtre et l'infobulle du bouton. */
const where = computed(() =>
  target.value ? t('upload.into', { folder: target.value }) : t('upload.atRoot'),
)

/**
 * Pendant que la fenêtre est ouverte, son voile couvre la zone de dépôt du
 * panneau : sans ce garde-fou, un fichier lâché à côté de la zone serait ouvert
 * par le navigateur — c'est le comportement par défaut d'un dépôt non traité.
 */
function blockNativeDrop(event: DragEvent) {
  event.preventDefault()
}

watch(open, (isOpen) => {
  if (isOpen) {
    document.addEventListener('dragover', blockNativeDrop)
    document.addEventListener('drop', blockNativeDrop)
  } else {
    document.removeEventListener('dragover', blockNativeDrop)
    document.removeEventListener('drop', blockNativeDrop)
  }
})
onBeforeUnmount(() => {
  document.removeEventListener('dragover', blockNativeDrop)
  document.removeEventListener('drop', blockNativeDrop)
})

/** Dépôt dans la zone de la fenêtre. */
async function onDrop(event: DragEvent) {
  over.value = false
  const data = event.dataTransfer
  if (!data) return
  // `dropPicks` lit les entrées avant son premier `await` : le `DataTransfer`
  // n'est valide que le temps de l'événement.
  const picks = await dropPicks(data)
  open.value = false
  void upload(picks, target.value)
}

function onPicked(event: Event) {
  const input = event.target as HTMLInputElement
  const files = Array.from(input.files ?? [])
  // Sans cette remise à zéro, rechoisir **le même** fichier ne déclencherait
  // aucun `change` : le champ n'aurait pas changé de valeur.
  input.value = ''
  open.value = false
  void upload(inputPicks(files), target.value)
}
</script>

<template>
  <button type="button" class="add" :disabled="running" :title="where" @click="open = true">
    <UIcon name="i-lucide-plus" class="add__icon" />
    <span>{{ $t('upload.add') }}</span>
  </button>

  <UModal v-model:open="open" :title="$t('upload.add')" :description="where">
    <template #body>
      <!-- Zone de dépôt : fichiers **et** dossiers. Les enfants sont en
           `pointer-events: none` (voir le CSS), sinon `dragleave` se déclenche
           au passage d'un texte à l'autre et la zone clignote. -->
      <div
        class="dropzone"
        :class="{ 'dropzone--over': over }"
        @dragenter.prevent="over = true"
        @dragover.prevent="over = true"
        @dragleave="over = false"
        @drop.prevent="onDrop"
      >
        <UIcon name="i-lucide-upload" class="dropzone__icon" />
        <p class="dropzone__hint">{{ $t('upload.dropHere') }}</p>
        <p class="dropzone__detail">{{ $t('upload.accepts') }}</p>
      </div>

      <!-- Secours : un glisser-déposer ne se fait pas au clavier. -->
      <p class="dropzone__browse">
        {{ $t('upload.or') }}
        <button type="button" class="dropzone__link" @click="filesInput?.click()">
          {{ $t('upload.files') }}
        </button>
        <button type="button" class="dropzone__link" @click="folderInput?.click()">
          {{ $t('upload.folder') }}
        </button>
      </p>
    </template>
  </UModal>

  <!-- Sélections masquées, ouvertes par le secours ci-dessus. -->
  <input ref="filesInput" class="add__input" type="file" multiple @change="onPicked" />
  <input ref="folderInput" class="add__input" type="file" webkitdirectory @change="onPicked" />

  <!-- Avancement de l'envoi : le bilan part en notification (voir `useUpload`). -->
  <p v-if="running" class="add__status">
    {{ $t('upload.running', { done, total, name: current }) }}
  </p>
</template>
