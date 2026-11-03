// Internationalisation : les **mêmes fichiers** de messages que l'application
// Nuxt (`frontend/i18n/locales/*.json`, référencés par `locales.json`).
//
// C'est délibéré : le test porte sur le cadre applicatif, pas sur les
// traductions. Une seule source, donc, et les deux implémentations affichent
// exactement les mêmes textes.
import { derived, writable } from 'svelte/store'
import localesConfig from '../../../frontend/i18n/locales.json'

/** Tous les fichiers de langue, résolus au build (aucune requête réseau). */
const fichiers = import.meta.glob('../../../frontend/i18n/locales/*.json', {
  eager: true,
  import: 'default',
}) as Record<string, Record<string, unknown>>

/** Messages par code de langue (`fr`, `en`, …). */
const parCode: Record<string, Record<string, unknown>> = {}
for (const [chemin, contenu] of Object.entries(fichiers)) {
  const code = chemin.split('/').pop()!.replace(/\.json$/, '')
  parCode[code] = contenu as Record<string, unknown>
}

export const langues = localesConfig.locales
export const langueParDefaut = localesConfig.defaultLocale

/** Langue courante (cookie `easy3d_lang`, comme l'app Nuxt). */
export const langue = writable<string>(langueParDefaut)

function lire(objet: Record<string, unknown>, cle: string): unknown {
  return cle.split('.').reduce<unknown>((noeud, part) => {
    if (noeud && typeof noeud === 'object') return (noeud as Record<string, unknown>)[part]
    return undefined
  }, objet)
}

/**
 * Traduit une clé. Gère les deux formes utilisées par les fichiers :
 *
 * - `{nom}` : interpolation ;
 * - `singulier | pluriel` : choix d'après `count` (la règle des fichiers de
 *   messages, identique à vue-i18n pour les langues gérées ici).
 */
export function traduire(
  code: string,
  cle: string,
  params: Record<string, unknown> = {},
): string {
  const messages = parCode[code] ?? parCode[langueParDefaut] ?? {}
  const brut = lire(messages, cle) ?? lire(parCode[langueParDefaut] ?? {}, cle)
  if (typeof brut !== 'string') return cle

  let texte = brut
  if (texte.includes('|')) {
    const formes = texte.split('|').map((f) => f.trim())
    const n = Number(params.count ?? 0)
    texte = n > 1 ? (formes[1] ?? formes[0]) : formes[0]
  }
  return texte.replace(/\{(\w+)\}/g, (_, nom) => String(params[nom] ?? `{${nom}}`))
}

/** `t` réactif : se recalcule au changement de langue. */
export const t = derived(langue, (code) => {
  return (cle: string, params: Record<string, unknown> = {}) => traduire(code, cle, params)
})

/** Applique la langue au document (attribut `lang`, comme l'app Nuxt). */
export function appliquerLangue(code: string) {
  if (typeof document !== 'undefined') document.documentElement.lang = code
}
