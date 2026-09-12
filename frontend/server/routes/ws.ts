import { defineWebSocketHandler } from 'h3'
import { backendWsUrl, relayToBackend, type Relay } from '../utils/wsRelay'

/**
 * WS « catalogue » (`/ws`) : le backend pousse un `ModelsResponse` à chaque
 * changement détecté par le watcher, le navigateur n'a donc rien à demander.
 *
 * Le client vise l'origine qui sert l'interface ; Nitro relaie vers le backend
 * (voir `server/utils/wsRelay.ts`). Une connexion amont par pair.
 */
const relays = new Map<string, Relay>()

function drop(peer: { id: string }) {
  relays.get(peer.id)?.close()
  relays.delete(peer.id)
}

export default defineWebSocketHandler({
  open(peer) {
    relays.set(peer.id, relayToBackend(peer, backendWsUrl('/ws')))
  },
  message(peer, message) {
    // Protocole à sens unique : on transmet quand même, un relais qui trie les
    // messages n'est plus un relais.
    relays.get(peer.id)?.send(message)
  },
  close(peer) {
    drop(peer)
  },
  error(peer) {
    drop(peer)
  },
})
