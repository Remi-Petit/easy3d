<script setup lang="ts">
/**
 * Texte Markdown, **rendu** en lecture seule.
 *
 * La réponse de l'assistant arrive en Markdown — titres, listes, gras, extraits
 * de code — et l'afficher tel quel laissait voir les `**` et les `-`. On
 * réutilise donc la bibliothèque qui rend déjà les notes (`md-editor-v3`, voir
 * `NotePanel.client.vue`) : même moteur, même thème, et rien de plus à
 * installer.
 *
 * Composant `.client.vue` : la bibliothèque manipule le DOM, elle ne doit pas
 * être rendue côté serveur.
 */
import { MdPreview } from 'md-editor-v3'
import 'md-editor-v3/lib/style.css'

defineProps<{
  /** Le Markdown à rendre, tel qu'il arrive du modèle (jamais retouché). */
  text: string
}>()

/** Langue de la bibliothèque : celle de l'interface (elle n'embarque que deux
 *  langues, et la page lui en déclare quatre — voir `NotePanel`). */
const { locale } = useI18n()
</script>

<template>
  <MdPreview
    class="md-text"
    :model-value="text"
    :language="locale"
    theme="dark"
    preview-theme="github"
    :no-highlight="true"
    :no-mermaid="true"
    :no-katex="true"
  />
</template>
