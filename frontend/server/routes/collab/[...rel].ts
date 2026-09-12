import { defineWebSocketHandler } from 'h3'
import { backendWsUrl, relayToBackend, type Relay } from '../../utils/wsRelay'

/**
 * WS d'une note (`/collab/<rel>`) : document CRDT partagé, en binaire (Yjs).
 *
 * Le chemin demandé est repris tel quel — c'est le nom de la « room » côté
 * backend — et la connexion est relayée vers le backend (voir
 * `server/utils/wsRelay.ts`). Une connexion amont par pair.
 */
const relays = new Map<string, Relay>()

function drop(peer: { id: string }) {
  relays.get(peer.id)?.close()
  relays.delete(peer.id)
}

export default defineWebSocketHandler({
  open(peer) {
    // `peer.request` est la requête d'ouverture : son chemin porte la room.
    const path = new URL(peer.request.url, 'http://localhost').pathname
    relays.set(peer.id, relayToBackend(peer, backendWsUrl(path), true))
  },
  message(peer, message) {
    relays.get(peer.id)?.send(message)
  },
  close(peer) {
    drop(peer)
  },
  error(peer) {
    drop(peer)
  },
})
