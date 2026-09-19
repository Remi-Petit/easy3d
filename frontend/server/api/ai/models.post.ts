// Proxy vers le backend Rust : POST /ai/models {provider, base_url, api_key}
//
// Interroge le fournisseur pour lister les modèles accessibles avec cette clé —
// c'est le bouton « Tester » de l'administration, qui remplit la liste de choix
// du modèle.
//
// Rien n'est enregistré : les valeurs viennent du formulaire, et la clé peut
// arriver sous la forme `***`, que le backend remplace par celle qu'il a.
export default defineEventHandler(async (event) => {
  const body = await readBody<{
    provider?: string | null
    base_url?: string | null
    api_key?: string | null
  }>(event)

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/ai/models`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        provider: body?.provider ?? null,
        base_url: body?.base_url ?? null,
        api_key: body?.api_key ?? null,
      }),
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
