/**
 * Relais WebSocket : le navigateur s'adresse à Nitro, Nitro au backend Rust.
 *
 * Le temps réel d'easy3d (catalogue poussé par le watcher, documents CRDT des
 * notes) est servi par axum. Plutôt que d'exposer ce second port et d'apprendre
 * au navigateur où joindre le backend — ce qui obligeait à figer une adresse
 * (`ws://localhost:8090`) et désignait donc, dès qu'on ouvrait le site depuis
 * une autre machine, la machine du **visiteur** et non le serveur — Nitro relaie
 * la connexion lui-même, comme il le fait déjà pour `/api/*`.
 *
 * Conséquence : un seul port à publier, aucune adresse à configurer côté
 * navigateur, et le même code fonctionne en direct (`:3000`), derrière un
 * reverse proxy, ou avec l'API sur un autre hôte.
 */

import { useRuntimeConfig } from '#imports'

/** Le strict nécessaire d'un pair `crossws` pour ce relais. */
export interface BrowserPeer {
  /**
   * Requête d'ouverture (`Upgrade`) : sert à retrouver le chemin demandé et, avec
   * l'authentification, le cookie de session du navigateur.
   */
  request: { url: string; headers?: unknown }
  send: (data: unknown) => void
  close: (code?: number, reason?: string) => void
}

/** Message reçu du navigateur (partie utile de `crossws.Message`). */
export interface BrowserMessage {
  text: () => string
  uint8Array: () => Uint8Array
}

/** Connexion amont d'un pair, en cours ou établie. */
export interface Relay {
  /** Transmet un message du navigateur vers le backend. */
  send: (message: BrowserMessage) => void
  /** Ferme la connexion amont (le pair se ferme de son côté). */
  close: () => void
}

/**
 * URL WebSocket du backend pour un chemin donné (`/ws`, `/collab/<rel>`).
 *
 * Déduite de `hpccatApiBase` — la même configuration que celle utilisée par le
 * proxy HTTP, donc identique en conteneur unique (`127.0.0.1:8090`) comme en
 * backend séparé (`http://api:8090`).
 */
export function backendWsUrl(path: string): string {
  return `${backendHttpUrl().replace(/^http/, 'ws')}${path}`
}

/** URL HTTP du backend (même base que `backendWsUrl`). */
export function backendHttpUrl(): string {
  const base = String(useRuntimeConfig().hpccatApiBase || 'http://127.0.0.1:8090')
  return base.replace(/\/+$/, '')
}

/**
 * En-tête `cookie` de la requête d'ouverture d'un pair.
 *
 * Selon l'adaptateur (Node, Web, uWebSockets…), les en-têtes du pair sont ceux de
 * Node (`IncomingHttpHeaders`) ou des `Headers` standards : les deux formes sont
 * acceptées ici, faute de type commun à tous les adaptateurs.
 */
export function peerCookie(peer: BrowserPeer): string {
  const headers = peer.request.headers
  if (!headers) return ''

  const web = headers as Headers
  if (typeof web.get === 'function') return web.get('cookie') ?? ''

  const node = headers as Record<string, string | string[] | undefined>
  const value = node.cookie
  return Array.isArray(value) ? value.join('; ') : (value ?? '')
}

/** Réponse du backend à une demande de ticket. */
export interface TicketResult {
  /** `false` : le backend a refusé la session (401). */
  accepted: boolean
  /** `null` : aucun ticket nécessaire (authentification éteinte) ou backend plus ancien. */
  ticket: string | null
}

/**
 * Demande au backend le droit d'ouvrir une connexion WebSocket.
 *
 * C'est le **seul** moyen pour le relais de s'authentifier : `new WebSocket(url)`
 * n'accepte aucun en-tête, donc la connexion amont ne peut porter le cookie du
 * navigateur — que Nitro, lui, reçoit très bien. Il demande donc un ticket court
 * au nom de cette session (voir `auth::ws_ticket` côté backend) et l'ajoute à
 * l'URL amont. Le ticket ne quitte jamais le serveur.
 */
export async function ticketFor(cookie: string): Promise<TicketResult> {
  try {
    const response = await fetch(`${backendHttpUrl()}/auth/ws-ticket`, {
      method: 'POST',
      headers: cookie ? { cookie } : undefined,
    })

    // Session refusée : inutile d'ouvrir une connexion amont anonyme, le backend
    // la fermerait de toute façon. Le pair est fermé avec un code qui dit que
    // c'est l'authentification qui bloque, et non le réseau.
    if (response.status === 401) return { accepted: false, ticket: null }

    // Backend plus ancien (route absente) ou en erreur : on se connecte comme
    // avant l'existence des comptes.
    if (!response.ok) return { accepted: true, ticket: null }

    const body = (await response.json()) as { ticket?: string | null }
    return { accepted: true, ticket: body.ticket ?? null }
  } catch {
    // Backend injoignable : on laisse la tentative de connexion se faire, elle
    // échouera proprement (et le client basculera sur son repli).
    return { accepted: true, ticket: null }
  }
}

/** Ajoute un paramètre à une URL (le ticket, en pratique). */
export function withParam(url: string, name: string, value: string): string {
  return `${url}${url.includes('?') ? '&' : '?'}${name}=${encodeURIComponent(value)}`
}

/** Nombre maximal de messages mis en attente avant l'ouverture amont. */
const MAX_PENDING = 512

/**
 * Ouvre la connexion amont et retransmet les deux sens.
 *
 * `binary` décrit le protocole de la route : le catalogue circule en **texte**
 * (JSON lu par `JSON.parse` côté navigateur) et les documents CRDT en
 * **binaire**. Les deux sens gardent le type d'origine — convertir l'un en
 * l'autre casserait le client, qui ne saurait plus décoder.
 *
 * `cookie` est celui du navigateur (transmis par le pair) : il sert à obtenir un
 * ticket, la connexion amont ne pouvant porter aucun en-tête.
 */
export function relayToBackend(peer: BrowserPeer, url: string, binary = false, cookie = ''): Relay {
  // Mis en attente tant que la connexion amont n'est pas ouverte : le protocole
  // des CRDT est bavard dès la première trame, et ces messages seraient perdus.
  const pending: (string | Uint8Array)[] = []
  let upstream: WebSocket | null = null
  let ready = false
  let closed = false

  function open(cible: string) {
    const socket = new WebSocket(cible)
    // Sans cela, les trames binaires arriveraient en `Blob` (asynchrone) au lieu
    // d'un `ArrayBuffer` exploitable tel quel.
    socket.binaryType = 'arraybuffer'
    upstream = socket

    socket.addEventListener('open', () => {
      ready = true
      for (const frame of pending) socket.send(frame)
      pending.length = 0
    })

    socket.addEventListener('message', (event) => {
      const data = event.data
      peer.send(binary && data instanceof ArrayBuffer ? new Uint8Array(data) : data)
    })

    socket.addEventListener('close', (event) => {
      // 1005/1006/1015 sont réservés et refusés par les clients WebSocket : on les
      // présente comme une erreur serveur plutôt que de faire échouer la fermeture.
      const code = event.code >= 1000 && ![1005, 1006, 1015].includes(event.code) ? event.code : 1011
      peer.close(code, event.reason)
    })

    socket.addEventListener('error', () => {
      // Backend injoignable : on ferme franchement, sinon le navigateur attendrait
      // indéfiniment au lieu de basculer sur son repli (polling, reconnexion).
      peer.close(1011, 'backend injoignable')
    })
  }

  // La demande de ticket est asynchrone : le relais, lui, existe tout de suite,
  // donc les premières trames du navigateur sont conservées au lieu d'être
  // perdues pendant l'aller-retour.
  void (async () => {
    const { accepted, ticket } = await ticketFor(cookie)
    if (closed) return
    if (!accepted) {
      peer.close(1008, 'authentification requise')
      return
    }
    open(ticket ? withParam(url, 'ticket', ticket) : url)
  })()

  return {
    send(message) {
      const frame = binary ? message.uint8Array() : message.text()
      if (ready && upstream) {
        upstream.send(frame)
      } else if (pending.length < MAX_PENDING) {
        pending.push(frame)
      } else {
        // Au-delà, mieux vaut rompre que laisser croire que tout est transmis :
        // un document CRDT amputé est pire qu'un document absent.
        peer.close(1011, 'backend trop lent')
      }
    },
    close() {
      closed = true
      upstream?.close()
    },
  }
}
