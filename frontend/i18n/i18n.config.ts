import { pluralizationRules } from './plural'

/**
 * Options passées à `createI18n`.
 *
 * `@nuxtjs/i18n` cherche ce fichier dans le dossier `i18n/` et fusionne ce qu'il
 * exporte dans les options du moteur : c'est donc **le** bon endroit pour ce que
 * le module ne règle pas lui-même. Ici, seulement les pluriels des langues qui
 * en ont plus de deux (voir `plural.ts`) — le reste (langues, messages, cookie,
 * détection) vient de `nuxt.config.ts` et de `locales.json`.
 *
 * Le nom compte : vue-i18n 11 lit `pluralRules` sur le composeur, et c'est sous
 * ce nom que le fichier doit l'exposer (l'ancien `pluralizationRules` de la
 * v9 n'est plus repris par le chemin de `createI18n`).
 */
export default defineI18nConfig(() => ({ pluralRules: pluralizationRules }))
