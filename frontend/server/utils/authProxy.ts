import { createError, proxyRequest, type H3Event } from 'h3'
import { useRuntimeConfig } from '#imports'

/**
 * Relais d'une route d'authentification vers le backend Rust.
 *
 * `proxyRequest` (et non `$fetch`, comme les autres proxys du projet) : c'est ce
 * qui fait circuler les **cookies dans les deux sens**. Le cookie de session doit
 * partir vers le backend, et surtout le `Set-Cookie` de la connexion doit
 * revenir intact au navigateur — un `$fetch` intermédiaire ne le laisserait pas
 * passer, et la connexion « réussirait » sans ouvrir de session.
 *
 * Le corps et le code d'état du backend passent tels quels : le frontend reçoit
 * les codes d'erreur (`invalid_credentials`, `rate_limited`…) et les traduit.
 */
export async function authProxy(event: H3Event, path: string) {
  const base: string = useRuntimeConfig(event).hpccatApiBase

  try {
    return await proxyRequest(event, `${base}${path}`)
  } catch (err: any) {
    throw createError({
      statusCode: 502,
      message: `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
      data: { cause: err?.message ?? String(err) },
    })
  }
}
