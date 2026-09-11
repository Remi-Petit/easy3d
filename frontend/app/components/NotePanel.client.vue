<script setup lang="ts">
// Note Markdown d'un dossier ou d'un fichier : affichage rendu + édition en
// **temps réel** (document partagé Yjs).
//
// Le texte n'est plus enregistré par un `PUT` : il vit dans un document
// collaboratif dont le backend Rust tient la version autoritaire. Plusieurs
// sessions peuvent donc écrire en même temps, à des endroits différents, sans
// qu'aucune saisie n'en écrase une autre — et le texte affiché suit en direct.
//
// Composant `.client.vue` : la bibliothèque d'édition manipule le DOM, elle ne
// doit pas être rendue côté serveur.
import { MdEditor, MdPreview, config } from 'md-editor-v3'
import 'md-editor-v3/lib/style.css'

const props = defineProps<{
  /** Chemin relatif du fichier, ou nom du dossier. */
  rel: string
  /** Note telle que vue par le scan : affichée tant que le document n'est pas synchronisé. */
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
  // Extensions CodeMirror : c'est par là qu'on branche la synchronisation Yjs
  // et les curseurs des autres participants. Un seul appel à `config()`, donc
  // les deux réglages ne peuvent pas s'écraser l'un l'autre.
  codeMirrorExtensions: (extensions: unknown[], { editorId }: { editorId: string }) =>
    collabExtensions(editorId, extensions),
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

/** Identifiant de l'éditeur, unique dans la page. */
const editorId = editorIdFor(props.rel)

const { text, connected, synced, peers, beginEdit } = useCollabNote(props.rel)

const editing = ref(false)
/**
 * Contenu de l'éditeur. Il suit le document partagé — y compris les
 * modifications venues des autres participants, que CodeMirror applique — ce
 * qui garde l'aperçu de md-editor-v3 à jour.
 */
const content = ref('')

/**
 * Texte affiché : le document partagé dès qu'il est synchronisé, sinon la
 * valeur du scan, pour ne pas afficher « aucune note » le temps de la connexion.
 */
const displayed = computed(() => (synced.value ? text.value : props.note ?? ''))

function toggleEdit() {
  if (editing.value) {
    editing.value = false
    return
  }
  // Le contenu initial doit correspondre à l'état du document partagé, sinon
  // l'éditeur impose le sien et le CRDT se désynchronise.
  content.value = beginEdit(editorId)
  editing.value = true
}
</script>

<template>
  <section class="note">
    <div class="note__head">
      <h2 class="note__title">📝 Note</h2>
      <div class="note__actions">
        <span
          v-if="editing"
          class="note__status"
          :class="connected ? 'note__status--saved' : 'note__status--error'"
        >
          {{ connected ? (peers > 0 ? `👥 ${peers + 1} personnes` : 'Connecté') : 'Hors ligne' }}
        </span>
        <button
          type="button"
          class="note__btn"
          :class="{ 'note__btn--primary': editing }"
          @click="toggleEdit"
        >
          {{ editing ? 'Terminer' : displayed ? 'Modifier' : 'Ajouter une note' }}
        </button>
      </div>
    </div>

    <MdEditor
      v-if="editing"
      :id="editorId"
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
        v-if="displayed"
        class="note__preview"
        :model-value="displayed"
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
