// Chargement du catalogue côté serveur : le HTML arrive **déjà rempli**, comme
// avec le SSR de Nuxt (même travail, même mesure).
import type { LayoutServerLoad } from './$types'
import { chargerCatalogue } from '$lib/api'
import { baseBackend } from '$lib/server/backend'

export const load: LayoutServerLoad = async ({ fetch, locals, request }) => {
  // Le cookie de session du visiteur est **transmis** au backend : sans lui,
  // `/models` répond 401 dès que les comptes sont activés (`EASY3D_AUTH`), et
  // la page ne rendrait que la coquille.
  const cookie = request.headers.get('cookie')
  const catalogue = await chargerCatalogue(
    (url, init) =>
      fetch(url, { ...init, headers: cookie ? { ...(init?.headers as object), cookie } : init?.headers }),
    baseBackend(),
  )
  return {
    langue: locals.langue,
    catalogue,
    // Version affichée en pied de sidebar (comme l'app Nuxt).
    version: '0.4.4',
    // Adresse du serveur MCP, montrée dans « Mon compte » côté Nuxt ; ici, la
    // même valeur publique, sans effet sur le test.
    mcp: baseBackend().replace(/^http/, 'ws'),
  }
}
