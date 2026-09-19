// Proxy vers le backend Rust : POST /ai/search {query}
//
// La recherche prend plusieurs secondes (le modèle interroge le catalogue en
// plusieurs allers-retours), d'où un délai large côté proxy.
//
// Les messages d'erreur du backend sont écrits pour l'utilisateur (« clé API
// manquante », « appel de … impossible ») : ils remontent tels quels, c'est le
// panneau de résultats qui les affiche.
export default defineEventHandler(async (event) => {
  const body = await readBody<{ query?: string }>(event)
  const query = body?.query?.trim()

  if (!query) {
    throw createError({ statusCode: 400, message: 'Paramètre query requis' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/ai/search`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ query }),
    })

    if (!res.ok) {
      const detail = (await res.text().catch(() => '')).trim()
      throw createError({
        statusCode: res.status,
        message: detail || `Backend a répondu ${res.status}`,
      })
    }

    return await res.json()
  } catch (err: any) {
    if (err?.statusCode) throw err
    throw createError({
      statusCode: 502,
      message: `Backend indisponible (${base}).`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
