/**
 * Adresse du service d'édition collaborative.
 *
 * Chaque note est un document CRDT identifié par le **chemin relatif** de
 * l'élément dans le catalogue — la même clé que celle des notes Markdown côté
 * backend (`/collab/{rel}`). Le nom de « room » envoyé par `y-websocket` est
 * donc ce chemin, encodé pour l'URL.
 */

/** Configuration publique du frontend (`runtimeConfig.public`). */
export interface WsConfig {
  hpccatWsBase?: string
}

/** Origine de la page, telle qu'il faut pour en déduire une URL WebSocket. */
export interface WsOrigin {
  protocol: string
  host: string
}

/**
 * Base WebSocket du temps réel.
 *
 * Priorité à `NUXT_PUBLIC_HPCCAT_WS_BASE` si elle est posée ; sinon **l'origine
 * de la page**. Nitro relaie `/ws` (catalogue) et `/collab/*` (notes) vers le
 * backend, donc le navigateur n'a aucune adresse de backend à connaître : le
 * site fonctionne tel qu'il est servi, proxy ou pas.
 *
 * L'ancien défaut, `ws://localhost:8090`, désignait la machine du **visiteur** :
 * ouverte depuis un autre poste, l'interface se connectait donc au conteneur
 * local de celui qui regardait, et non au serveur.
 */
export function wsBase(pub?: WsConfig, here?: WsOrigin): string {
  const configured = (pub?.hpccatWsBase || '').replace(/\/+$/, '')
  if (configured) return configured
  const origin = here ?? (typeof location === 'undefined' ? undefined : location)
  if (!origin) return ''
  return `${origin.protocol === 'https:' ? 'wss:' : 'ws:'}//${origin.host}`
}

/** Base de la route collaborative, sans slash final (`ws://…/collab`). */
export function collabBase(pub?: WsConfig, here?: WsOrigin): string {
  return `${wsBase(pub, here)}/collab`
}

/**
 * Nom de « room » d'une note, encodé pour tenir dans une URL.
 *
 * Les segments restent séparés par `/` — le backend les attend tels quels —
 * mais tout ce qui perturberait l'URL est échappé : espace, `#`, `?`, `%`.
 */
export function collabRoom(rel: string): string {
  return rel
    .split('/')
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0)
    .map(encodeURIComponent)
    .join('/')
}

/** URL WebSocket complète du document d'une note. */
export function collabUrl(pub: WsConfig | undefined, rel: string, here?: WsOrigin): string {
  return `${collabBase(pub, here)}/${collabRoom(rel)}`
}

/**
 * Identifiant DOM de l'éditeur d'une note.
 *
 * md-editor-v3 s'en sert pour construire des sélecteurs CSS
 * (`#<id> .cm-scroller`) : tout ce qui n'est pas valide dans un identifiant
 * — points, slashes, espaces — doit être remplacé, sinon les sélecteurs ne
 * correspondent à rien.
 */
export function editorIdFor(rel: string): string {
  const sur = rel.replace(/[^A-Za-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '')
  return `note-${sur || 'courante'}`
}
