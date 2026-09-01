// Proxy vers le backend Rust : GET /file?path=<rel>
// Relaie le contenu binaire d'un modèle (STL / OBJ / 3MF / GCODE) au navigateur.
export default defineEventHandler(async (event) => {
  const q = getQuery(event)
  const rel = q.path
  if (typeof rel !== 'string' || !rel) {
    throw createError({ statusCode: 400, statusMessage: 'Paramètre path manquant' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    // fetch natif Node : fiable pour les réponses binaires (ArrayBuffer).
    const res = await fetch(`${base}/file?path=${encodeURIComponent(rel)}`)
    if (!res.ok) {
      throw createError({
        statusCode: res.status,
        statusMessage: `Backend a répondu ${res.status}`,
      })
    }
    const ct = res.headers.get('content-type') || 'application/octet-stream'
    setResponseHeaders(event, { 'content-type': ct })
    return Buffer.from(await res.arrayBuffer())
  } catch (err: any) {
    throw createError({
      statusCode: err?.statusCode || 502,
      statusMessage: `Backend indisponible (${base}).`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
