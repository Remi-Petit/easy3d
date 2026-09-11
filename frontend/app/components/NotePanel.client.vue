<script setup lang="ts">
// Note Markdown d'un dossier ou d'un fichier : affichage rendu + édition
// assistée (barre d'outils) avec enregistrement automatique côté backend.
//
// Composant `.client.vue` : la bibliothèque d'édition manipule le DOM, elle ne
// doit pas être rendue côté serveur.
import { MdEditor, MdPreview, config } from 'md-editor-v3'
import 'md-editor-v3/lib/style.css'

const props = defineProps<{
  /** Chemin relatif du fichier, ou nom du dossier. */
  rel: string
  /** Note initiale, telle que renvoyée par le scan. */
  note?: string | null
}>()

/**
 * Textes français (la bibliothèque n'embarque que `zh-CN` et `en-US`).
 * Tous les champs sont optionnels : on ne traduit que la barre d'outils.
 */
config({
  editorConfig: {
    languageUserDefined: {
      fr: {
        toolbarTips: {
          bold: 'Gras',
          underline: 'Souligné',
          italic: 'Italique',
          strikeThrough: 'Barré',
          title: 'Titre',
          sub: 'Indice',
          sup: 'Exposant',
          quote: 'Citation',
          unorderedList: 'Liste à puces',
          orderedList: 'Liste numérotée',
          task: 'Case à cocher',
          codeRow: 'Code en ligne',
          code: 'Bloc de code',
          link: 'Lien',
          image: 'Image',
          table: 'Tableau',
          revoke: 'Annuler',
          next: 'Rétablir',
          save: 'Enregistrer',
          prettier: 'Formater',
          pageFullscreen: 'Plein écran de la page',
          fullscreen: 'Plein écran',
          preview: 'Aperçu',
          previewOnly: 'Aperçu seul',
          htmlPreview: 'HTML',
          catalog: 'Sommaire',
          github: 'GitHub',
        },
        titleItem: {
          h1: 'Titre 1',
          h2: 'Titre 2',
          h3: 'Titre 3',
          h4: 'Titre 4',
          h5: 'Titre 5',
          h6: 'Titre 6',
        },
        linkModalTips: {
          linkTitle: 'Insérer un lien',
          imageTitle: 'Insérer une image',
          descLabel: 'Texte',
          descLabelPlaceHolder: 'Texte affiché…',
          urlLabel: 'Adresse',
          urlLabelPlaceHolder: 'https://…',
          buttonOK: 'Valider',
        },
        footer: {
          markdownTotal: 'caractères',
          scrollAuto: 'Défilement synchronisé',
        },
      },
    },
  },
})

/** Barre d'outils : de quoi mettre en forme sans écrire de Markdown. */
const TOOLBARS = [
  'bold',
  'italic',
  'strikeThrough',
  '-',
  'title',
  '-',
  'unorderedList',
  'orderedList',
  'task',
  '-',
  'quote',
  'code',
  'codeRow',
  '-',
  'link',
  'table',
  '-',
  'revoke',
  'next',
  '-',
  'preview',
] as const

const content = ref(props.note ?? '')
const editing = ref(false)
const status = ref<'idle' | 'saving' | 'saved' | 'error'>('idle')

const statusLabel = computed(
  () =>
    ({
      idle: '',
      saving: 'Enregistrement…',
      saved: 'Enregistré',
      error: 'Échec de l’enregistrement',
    })[status.value],
)

let timer: ReturnType<typeof setTimeout> | null = null

/** Enregistre la note (différé, pour ne pas écrire à chaque frappe). */
async function save(value: string) {
  timer = null
  status.value = 'saving'
  try {
    await $fetch('/api/note', {
      method: 'PUT',
      query: { path: props.rel },
      body: { content: value },
    })
    status.value = 'saved'
  } catch {
    status.value = 'error'
  }
}

watch(content, (value) => {
  // Rien à enregistrer si le contenu est identique à ce qui est déjà stocké.
  if (value === (props.note ?? '')) return
  if (timer) clearTimeout(timer)
  status.value = 'saving'
  timer = setTimeout(() => void save(value), 700)
})

// Quitte la page juste après une frappe : on force l'enregistrement en attente.
onBeforeUnmount(() => {
  if (timer) {
    clearTimeout(timer)
    void save(content.value)
  }
})
</script>

<template>
  <section class="note">
    <div class="note__head">
      <h2 class="note__title">📝 Note</h2>
      <div class="note__actions">
        <span v-if="editing && statusLabel" class="note__status" :class="`note__status--${status}`">
          {{ statusLabel }}
        </span>
        <button
          type="button"
          class="note__btn"
          :class="{ 'note__btn--primary': editing }"
          @click="editing = !editing"
        >
          {{ editing ? 'Terminer' : content ? 'Modifier' : 'Ajouter une note' }}
        </button>
      </div>
    </div>

    <MdEditor
      v-if="editing"
      v-model="content"
      language="fr"
      theme="dark"
      preview-theme="github"
      :toolbars="TOOLBARS"
      :style="{ height: '360px' }"
      :no-highlight="true"
      :no-mermaid="true"
      :no-katex="true"
      :no-prettier="true"
      :no-upload-img="true"
      :footers="['markdownTotal']"
      placeholder="Décrivez ce fichier ou ce dossier…"
    />
    <template v-else>
      <MdPreview
        v-if="content"
        class="note__preview"
        :model-value="content"
        language="fr"
        theme="dark"
        preview-theme="github"
        :no-highlight="true"
        :no-mermaid="true"
        :no-katex="true"
      />
      <p v-else class="note__empty">Aucune note pour l’instant.</p>
    </template>
  </section>
</template>
