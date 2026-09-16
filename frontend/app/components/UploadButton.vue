<script setup lang="ts">
/**
 * Ajout de fichiers au catalogue depuis l'interface.
 *
 * Deux sélections distinctes, parce que le navigateur ne sait pas les mélanger
 * dans un seul champ : `multiple` pour des fichiers, `webkitdirectory` pour un
 * dossier entier — chaque fichier arrive alors avec son `webkitRelativePath`,
 * donc son arborescence.
 *
 * L'envoi passe par `POST /api/upload` (voir `useUpload`) ; le backend écrit le
 * fichier, le watcher le détecte et rediffuse le catalogue — la liste se met
 * donc à jour toute seule.
 */
const props = withDefaults(defineProps<{ folder?: string | null }>(), { folder: null })

const { t } = useI18n()
const { state, total, done, current, failures, running, upload } = useUpload()

const filesInput = ref<HTMLInputElement | null>(null)
const folderInput = ref<HTMLInputElement | null>(null)

/** Dossier d'accueil : celui de la page courante, sinon la racine du catalogue. */
const target = computed(() => props.folder ?? '')

const items = computed(() => [
  {
    label: t('upload.files'),
    icon: 'i-lucide-file-plus',
    onSelect: () => filesInput.value?.click(),
  },
  {
    label: t('upload.folder'),
    icon: 'i-lucide-folder-plus',
    onSelect: () => folderInput.value?.click(),
  },
])

/** Infobulle du bouton : rappelle où les fichiers seront déposés. */
const title = computed(() =>
  target.value ? t('upload.into', { folder: target.value }) : t('upload.add'),
)

function onPicked(event: Event) {
  const input = event.target as HTMLInputElement
  const files = Array.from(input.files ?? [])
  // Sans cette remise à zéro, rechoisir **le même** fichier ne déclencherait
  // aucun `change` : le champ n'aurait pas changé de valeur.
  input.value = ''
  void upload(inputPicks(files), target.value)
}
</script>

<template>
  <!-- `UDropdownMenu` ne rend aucun élément : le bouton est un enfant direct de
       la barre d'outils, aligné sur le bouton de tri. -->
  <UDropdownMenu :items="items" :content="{ align: 'end' }">
    <button type="button" class="add" :disabled="running" :title="title">
      <UIcon name="i-lucide-plus" class="add__icon" />
      <span>{{ $t('upload.add') }}</span>
    </button>
  </UDropdownMenu>

  <!-- Sélections masquées : ce sont les entrées du menu qui les ouvrent. -->
  <input ref="filesInput" class="add__input" type="file" multiple @change="onPicked" />
  <input ref="folderInput" class="add__input" type="file" webkitdirectory @change="onPicked" />

  <!-- Avancement, puis bilan : sur sa propre ligne, sous la barre d'outils. -->
  <p v-if="running" class="add__status">
    {{ $t('upload.running', { done, total, name: current }) }}
  </p>
  <p v-else-if="state === 'done'" class="add__status add__status--ok">
    {{ $t('upload.done', { count: done }, done) }}
  </p>
  <!--
    Échecs : le détail plutôt que le seul compteur. Un « 2 échecs » sans les
    noms oblige à deviner lequel des fichiers pose problème et pourquoi.
  -->
  <div v-else-if="state === 'failed'" class="add__status add__status--err">
    <p class="add__count">
      {{ $t('upload.failed', { count: failures.length }, failures.length) }}
    </p>
    <ul class="add__failures">
      <li v-for="failure in failures.slice(0, 3)" :key="failure">{{ failure }}</li>
      <li v-if="failures.length > 3">…</li>
    </ul>
  </div>
  <p v-else-if="state === 'empty'" class="add__status add__status--err">
    {{ $t('upload.empty') }}
  </p>
</template>
