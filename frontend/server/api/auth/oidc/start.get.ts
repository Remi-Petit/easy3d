import { createError, proxyRequest } from 'h3'
import { useRuntimeConfig } from '#imports'
import { callerHeaders } from '../../../utils/backend'

/**
 * Départ du flux SSO : le backend répond un `302` vers le fournisseur.
 *
 * ⚠️ **La redirection ne doit pas être suivie.** `proxyRequest` (comme `fetch`)
 * suit les redirections par défaut : sans `redirect: 'manual'`, Nitro irait
 * lui-même chez le fournisseur, puis renverrait au navigateur la **page HTML** du
 * fournisseur — un écran de connexion inutilisable (ses cookies ne nous
 * appartiennent pas), ou une boucle. On recopie donc le `302` tel quel, et c'est
 * le navigateur qui va chez le fournisseur, avec l'adresse *de l'utilisateur*.
 */
export default defineEventHandler(async (event) => {
  const base: string = useRuntimeConfig(event).hpccatApiBase
  const url = new URL(`${base}/auth/oidc/start`)
  const query = getQuery(event)
  if (typeof query.redirect === 'string' && query.redirect) {
    url.searchParams.set('redirect', query.redirect)
  }

  try {
    return await proxyRequest(event, url.toString(), {
      headers: callerHeaders(event),
      fetchOptions: { redirect: 'manual' },
    })
  } catch (err: any) {
    throw createError({
      statusCode: 502,
      message: `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
