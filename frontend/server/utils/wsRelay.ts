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
  /** Requête d'ouverture (`Upgrade`) : sert à retrouver le chemin demandé. */
  request: { url: string }
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
  const base = String(useRuntimeConfig().hpccatApiBase || 'http://127.0.0.1:8090')
  return `${base.replace(/^http/, 'ws').replace(/\/+$/, '')}${path}`
}

/**
 * Ouvre la connexion amont et retransmet les deux sens.
 *
 * `binary` décrit le protocole de la route : le catalogue circule en **texte**
 * (JSON lu par `JSON.parse` côté navigateur) et les documents CRDT en
 * **binaire**. Les deux sens gardent le type d'origine — convertir l'un en
 * l'autre casserait le client, qui ne saurait plus décoder.
 */
export function relayToBackend(peer: BrowserPeer, url: string, binary = false): Relay {
  const upstream = new WebSocket(url)
  // Sans cela, les trames binaires arriveraient en `Blob` (asynchrone) au lieu
  // d'un `ArrayBuffer` exploitable tel quel.
  upstream.binaryType = 'arraybuffer'

  // Le backend ne répond qu'une fois connecté : ce qui est écrit avant (les
  // protocoles bavards des CRDT envoient dès l'ouverture) est mis en attente.
  const pending: (string | Uint8Array)[] = []
  let ready = false

  upstream.addEventListener('open', () => {
    ready = true
    for (const frame of pending) upstream.send(frame)
    pending.length = 0
  })

  upstream.addEventListener('message', (event) => {
    const data = event.data
    peer.send(binary && data instanceof ArrayBuffer ? new Uint8Array(data) : data)
  })

  upstream.addEventListener('close', (event) => {
    // 1005/1006/1015 sont réservés et refusés par les clients WebSocket : on les
    // présente comme une erreur serveur plutôt que de faire échouer la fermeture.
    const code = event.code >= 1000 && ![1005, 1006, 1015].includes(event.code) ? event.code : 1011
    peer.close(code, event.reason)
  })

  upstream.addEventListener('error', () => {
    // Backend injoignable : on ferme franchement, sinon le navigateur attendrait
    // indéfiniment au lieu de basculer sur son repli (polling, reconnexion).
    peer.close(1011, 'backend injoignable')
  })

  return {
    send(message) {
      const frame = binary ? message.uint8Array() : message.text()
      if (ready) upstream.send(frame)
      else pending.push(frame)
    },
    close() {
      upstream.close()
    },
  }
}
