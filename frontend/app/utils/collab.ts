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
  hpccatApiBase?: string
}

/** Base WebSocket du backend, dérivée de la configuration. */
export function wsBase(pub?: WsConfig): string {
  const base =
    pub?.hpccatWsBase ||
    (pub?.hpccatApiBase ? String(pub.hpccatApiBase).replace(/^http/, 'ws') : '')
  return base.replace(/\/+$/, '')
}

/** Base de la route collaborative, sans slash final (`ws://…/collab`). */
export function collabBase(pub?: WsConfig): string {
  return `${wsBase(pub)}/collab`
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
export function collabUrl(pub: WsConfig | undefined, rel: string): string {
  return `${collabBase(pub)}/${collabRoom(rel)}`
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
