import { createError, getMethod, getRequestHeader, readRawBody, type H3Event } from 'h3'
import { useRuntimeConfig } from '#imports'

/**
 * Appel au backend **pour le compte du navigateur** : mêmes en-têtes, même corps,
 * mêmes codes d'état.
 *
 * Sert aux routes d'administration (comptes, rôles) dont le corps et la réponse
 * JSON se recopient tels quels : une seule fonction plutôt que dix proxys qui
 * répètent la recopie du cookie et la traduction des erreurs.
 */
export async function backendCall(event: H3Event, path: string) {
  const base: string = useRuntimeConfig(event).hpccatApiBase
  const method = getMethod(event)
  const withBody = method === 'POST' || method === 'PUT' || method === 'PATCH'

  try {
    // `$fetch.raw` (et non `$fetch`) : il faut le **code d'état** du backend pour
    // le rendre tel quel — une création répond 201, et l'interface s'en sert.
    const response = await $fetch.raw(`${base}${path}`, {
      method: method as 'GET' | 'POST' | 'PUT' | 'DELETE',
      body: withBody ? ((await readRawBody(event)) ?? undefined) : undefined,
      headers: {
        ...backendHeaders(event),
        ...(withBody
          ? { 'content-type': getRequestHeader(event, 'content-type') || 'application/json' }
          : {}),
      },
    })

    setResponseStatus(event, response.status)
    return response._data
  } catch (err: any) {
    throw backendError(base, err)
  }
}

/**
 * En-têtes à transmettre au backend : le **cookie de session**.
 *
 * Ces routes-ci appellent le backend avec `fetch`/`$fetch` **depuis le serveur**,
 * donc sans les en-têtes du navigateur : Nitro n'est pas un simple tube. Depuis
 * que les comptes existent, le cookie doit être recopié à la main — sinon le
 * backend voit un anonyme (401) alors que le navigateur, lui, est bien connecté.
 *
 * Les routes d'authentification n'en ont pas besoin : elles passent par
 * `proxyRequest` (voir `server/utils/authProxy.ts`), qui recopie tout.
 */
export function backendHeaders(event: H3Event): Record<string, string> {
  const cookie = getRequestHeader(event, 'cookie')
  return cookie ? { cookie } : {}
}

/**
 * Erreur d'un appel au backend.
 *
 * Quand le backend a **répondu**, son code est conservé : un 401 veut dire « pas
 * de session » (l'interface doit renvoyer vers la page de connexion), un 400 un
 * refus métier… Seule l'absence de réponse est une indisponibilité. Sans cette
 * distinction, une session expirée s'afficherait « backend indisponible » — et
 * on chercherait la panne au mauvais endroit.
 */
export function backendError(base: string, err: any) {
  const status: number | undefined = err?.response?.status

  if (typeof status === 'number') {
    const detail = typeof err?.data === 'string' ? err.data.trim() : ''
    return createError({
      statusCode: status,
      // `message` plutôt que `statusMessage` : h3 réserve ce dernier aux messages
      // courts et le nettoiera dans une prochaine version (même convention que
      // `config.put.ts`).
      message: detail || `Backend a répondu ${status}`,
    })
  }

  return createError({
    statusCode: 502,
    message: `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
    data: { cause: err?.message ?? String(err) },
  })
}
