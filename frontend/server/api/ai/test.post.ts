// Proxy vers le backend Rust : POST /ai/test
//
// Un aller-retour minimal vers le fournisseur configuré : c'est le bouton
// « Tester » de l'administration, qui distingue « la clé est fausse » d'un
// « je croyais que c'était configuré ».
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/ai/test`, { method: 'POST' })

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
