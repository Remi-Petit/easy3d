// Proxy d'écriture vers le backend Rust : POST /rename {path, name}
//
// Un renommage ne change que le **dernier** segment : l'élément reste à sa
// place, sous son nouveau nom. Le backend fait suivre la note (`/.easy3d-notes`)
// et l'aperçu (`/.easy3d-thumbs`), puis rediffuse le catalogue sur le WebSocket
// — l'interface se met donc à jour sans rechargement, y compris là où le watcher
// ne verrait rien de l'opération (partage de fichiers virtualisé, sous Docker
// Desktop).
//
// Les refus du backend (« « Toit » existe déjà », « nom invalide »…) sont écrits
// pour l'utilisateur : ils remontent tels quels.
export default defineEventHandler(async (event) => {
  const body = await readBody<{ path?: string; name?: string }>(event)
  const path = body?.path
  const name = body?.name

  if (typeof path !== 'string' || !path || typeof name !== 'string' || !name) {
    throw createError({ statusCode: 400, message: 'Paramètres path et name requis' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/rename`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path, name }),
    })

    if (!res.ok) {
      const detail = (await res.text().catch(() => '')).trim()
      throw createError({
        statusCode: res.status,
        message: detail || `Backend a répondu ${res.status}`,
      })
    }

    return await res.json().catch(() => ({}))
  } catch (err: any) {
    // Une erreur déjà construite (refus du backend) passe telle quelle.
    if (err?.statusCode) throw err

    throw createError({
      statusCode: 502,
      message: `Backend indisponible (${base}).`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
