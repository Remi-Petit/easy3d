// Relais générique vers le backend Rust.
//
// L'application Nuxt déclare **37 routes** (une par endpoint, avec validation
// et messages d'erreur) ; ici, un seul relais : c'est suffisant pour le test de
// consommation, et c'est aussi la partie où SvelteKit demande le moins de code.
// Les en-têtes de sécurité de l'interface restent posés par `hooks.server.ts`.
import type { RequestHandler } from './$types'
import { baseBackend } from '$lib/server/backend'

const RELAYE = new Set(['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'HEAD', 'OPTIONS'])

const passer: RequestHandler = async ({ request, params, url }) => {
  if (!RELAYE.has(request.method)) {
    return new Response('Méthode non relayée', { status: 405 })
  }

  const cible = `${baseBackend()}/${params.path ?? ''}${url.search}`

  // Le corps est transmis tel quel (JSON, formulaire, binaire d'un envoi) :
  // `duplex: 'half'` est exigé par Node pour un corps en flux.
  const entetes = new Headers()
  const type = request.headers.get('content-type')
  if (type) entetes.set('content-type', type)
  const cookie = request.headers.get('cookie')
  if (cookie) entetes.set('cookie', cookie)
  const longueur = request.headers.get('content-length')
  if (longueur) entetes.set('content-length', longueur)

  const corps =
    request.method === 'GET' || request.method === 'HEAD' ? undefined : request.body

  const reponse = await fetch(cible, {
    method: request.method,
    headers: entetes,
    body: corps,
    // @ts-expect-error — `duplex` existe à l'exécution (Node), pas dans le type DOM.
    duplex: 'half',
    redirect: 'manual',
  })

  // On recopie les en-têtes utiles (type, cache, cookies de session).
  const sortie = new Headers()
  for (const nom of ['content-type', 'cache-control', 'etag', 'last-modified']) {
    const valeur = reponse.headers.get(nom)
    if (valeur) sortie.set(nom, valeur)
  }
  for (const cookie of reponse.headers.getSetCookie?.() ?? []) {
    sortie.append('set-cookie', cookie)
  }

  return new Response(reponse.body, { status: reponse.status, headers: sortie })
}

export const GET = passer
export const POST = passer
export const PUT = passer
export const DELETE = passer
export const PATCH = passer
export const HEAD = passer
export const OPTIONS = passer
