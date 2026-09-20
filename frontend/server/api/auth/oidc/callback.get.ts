import { createError, proxyRequest, type H3Event, getQuery } from 'h3'
import { useRuntimeConfig } from '#imports'

/**
 * Retour du fournisseur d'identité : le backend échange le code, rattache le
 * compte et répond un `302` vers l'interface, **avec le `Set-Cookie` de
 * session**.
 *
 * Même piège que pour le départ : la redirection est recopiée, pas suivie
 * (`redirect: 'manual'`). Sans ça, Nitro suivrait le `302` vers `/` ou
 * `/dossiers/…`, renverrait le HTML de cette page à la place de la redirection —
 * le navigateur ne verrait jamais le `Set-Cookie` et l'utilisateur resterait
 * déconnecté après avoir pourtant saisi son mot de passe chez le fournisseur.
 */
export default defineEventHandler(async (event: H3Event) => {
  const base: string = useRuntimeConfig(event).hpccatApiBase
  const url = new URL(`${base}/auth/oidc/callback`)
  // `code`, `state` et `error` sont recopiés tels quels : c'est le backend qui
  // les valide (l'`state` y est à usage unique, et lui seul peut en juger).
  for (const [cle, valeur] of Object.entries(getQuery(event))) {
    if (typeof valeur === 'string' && valeur) {
      url.searchParams.set(cle, valeur)
    }
  }

  try {
    return await proxyRequest(event, url.toString(), {
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
