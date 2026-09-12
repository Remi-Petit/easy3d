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
import type { StaticTextDefaultValue } from 'md-editor-v3'
import 'md-editor-v3/lib/style.css'

// Libellés de l'éditeur : ils viennent des mêmes fichiers que le reste de l'app
// (sous `notes.editor`, donc inclus dans le test de couverture des langues) —
// mais lus en **texte brut** puis analysés.
//
// Pourquoi pas un import direct : le module i18n compile les messages de
// `i18n/locales/` au build, si bien qu'un `import` classique renvoie des arbres
// compilés (`{ type, body, loc… }`) et non des chaînes. md-editor, qui attend
// des chaînes, afficherait « [object Object] » partout.
//
// Et une ligne par langue, **pas** un `import.meta.glob` : le glob y récupère
// ces mêmes messages compilés, quel que soit le suffixe demandé (`?raw` compris).
import deRaw from '~~/i18n/locales/de.json?raw'
import enRaw from '~~/i18n/locales/en.json?raw'
import esRaw from '~~/i18n/locales/es.json?raw'
import frRaw from '~~/i18n/locales/fr.json?raw'

/** Extrait le bloc `notes.editor` d'un fichier de langue brut. */
function editorText(raw: string): StaticTextDefaultValue {
  return JSON.parse(raw).notes.editor as StaticTextDefaultValue
}

const props = defineProps<{
  /** Chemin relatif du fichier, ou nom du dossier. */
  rel: string
  /** Note telle que vue par le scan : affichée tant que le document n'est pas synchronisé. */
  note?: string | null
}>()

/**
 * Libellés lus par md-editor (la bibliothèque n'embarque que `zh-CN` et
 * `en-US`). Tous les champs sont optionnels : on ne traduit que la barre d'outils.
 */
config({
  editorConfig: {
    /**
     * md-editor-v3 n'embarque que `zh-CN` et `en-US` : les langues de l'app
     * sont donc déclarées ici, une entrée par fichier de `i18n/locales/`.
     *
     * Elles doivent l'être **d'un coup** : `config()` est un réglage global,
     * appelé une seule fois à l'import, alors que md-editor choisit son libellé
     * à l'affichage selon le `language` courant.
     */
    languageUserDefined: {
      fr: editorText(frRaw),
      en: editorText(enRaw),
      de: editorText(deRaw),
      es: editorText(esRaw),
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
/** Langue de l'éditeur : elle suit celle de l'app (voir `config()` plus haut). */
const { locale } = useI18n()
const mdLanguage = computed(() => locale.value)
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
      <h2 class="note__title">{{ $t('notes.title') }}</h2>
      <div class="note__actions">
        <span
          v-if="editing"
          class="note__status"
          :class="connected ? 'note__status--saved' : 'note__status--error'"
        >
          {{ connected ? (peers > 0 ? $t('notes.peers', { count: peers + 1 }) : $t('notes.connected')) : $t('notes.offline') }}
        </span>
        <button
          type="button"
          class="note__btn"
          :class="{ 'note__btn--primary': editing }"
          @click="toggleEdit"
        >
          {{ editing ? $t('notes.finish') : displayed ? $t('notes.edit') : $t('notes.add') }}
        </button>
      </div>
    </div>

    <MdEditor
      v-if="editing"
      :id="editorId"
      v-model="content"
      :language="mdLanguage"
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
      :placeholder="$t('notes.placeholder')"
    />
    <template v-else>
      <MdPreview
        v-if="displayed"
        class="note__preview"
        :model-value="displayed"
        :language="mdLanguage"
        theme="dark"
        preview-theme="github"
        :no-highlight="true"
        :no-mermaid="true"
        :no-katex="true"
      />
      <p v-else class="note__empty">{{ $t('notes.empty') }}</p>
    </template>
  </section>
</template>
