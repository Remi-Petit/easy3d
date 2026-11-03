// Serveur de production : le handler SvelteKit **plus** le relais WebSocket.
//
// SvelteKit ne sait pas prendre en charge une montée en WebSocket dans une
// route : il faut donc un serveur HTTP devant, comme Nitro le fait pour l'app
// Nuxt (`frontend/server/routes/ws.ts` et `collab/[...rel].ts`). Même logique,
// même découpage : `/ws` (catalogue, texte) et `/collab/*` (notes, binaire)
// sont relayés vers le backend Rust.
import { handler } from './build/handler.js'
import { createServer } from 'node:http'
import { WebSocket, WebSocketServer } from 'ws'

const PORT = Number(process.env.PORT || 3000)
const BACKEND = (
  process.env.NUXT_HPCCAT_API_BASE ||
  process.env.EASY3D_API_BASE ||
  `http://127.0.0.1:${process.env.NUXT_HPCCAT_API_PORT || '8090'}`
).replace(/\/+$/, '')

const BACKEND_WS = BACKEND.replace(/^http/, 'ws')

const serveur = createServer(handler)
const wss = new WebSocketServer({ noServer: true })

serveur.on('upgrade', (requete, socket, tete) => {
  const url = new URL(requete.url ?? '/', 'http://localhost')
  if (url.pathname !== '/ws' && !url.pathname.startsWith('/collab/')) {
    socket.destroy()
    return
  }
  wss.handleUpgrade(requete, socket, tete, (client) => {
    // Une connexion amont par pair : les messages sont retransmis tels quels
    // (texte ou binaire), sans les interpréter.
    const amont = new WebSocket(`${BACKEND_WS}${url.pathname}${url.search}`)

    // Tampon avant `open` : un client peut envoyer son premier message
    // (sync-step1 d'un document Yjs) avant que l'amont soit prêt.
    const tampon = []
    const envoyer = (donnee, binaire) => {
      if (amont.readyState === WebSocket.OPEN) amont.send(donnee, { binary: binaire })
      else tampon.push([donnee, binaire])
    }

    amont.on('open', () => {
      for (const [donnee, binaire] of tampon.splice(0)) amont.send(donnee, { binary: binaire })
    })
    amont.on('message', (donnee, binaire) => {
      if (client.readyState === WebSocket.OPEN) client.send(donnee, { binary: binaire })
    })

    client.on('message', (donnee, binaire) => envoyer(donnee, binaire))

    const fermer = (code) => {
      // 1005/1006 ne peuvent pas être renvoyés tels quels : on retombe sur 1011.
      const sur = code === 1005 || code === 1006 ? 1011 : code
      if (client.readyState === WebSocket.OPEN) client.close(sur)
      if (amont.readyState === WebSocket.OPEN) amont.close(sur)
    }
    amont.on('close', (code) => fermer(code))
    client.on('close', (code) => fermer(code))
    amont.on('error', () => fermer(1011))
    client.on('error', () => fermer(1011))
  })
})

serveur.listen(PORT, () => {
  console.log(`easy3d (SvelteKit) : http://0.0.0.0:${PORT} — relais WS vers ${BACKEND_WS}`)
})
