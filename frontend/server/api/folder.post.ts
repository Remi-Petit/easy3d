// Proxy d'écriture vers le backend Rust : POST /folder {parent, name}
//
// Crée un **dossier vide** dans le catalogue — le pendant du « Nouveau dossier »
// d'un explorateur de fichiers. `parent` est le dossier d'accueil (`''` pour la
// racine) ; `name` est le seul nom du dossier, sans séparateur.
//
// Le backend refuse un nom déjà pris (409), un nom invalide ou caché (400) et un
// parent introuvable (404) ; ces messages sont écrits pour l'utilisateur et
// remontent tels quels. Comme les autres écritures, il rediffuse ensuite la
// liste complète sur le WebSocket : le nouveau dossier apparaît sans
// rechargement, même là où le watcher ne verrait rien (partage de fichiers
// virtualisé, sous Docker Desktop).
export default defineEventHandler(async (event) => {
  const body = await readBody<{ parent?: string; name?: string }>(event)
  const parent = typeof body?.parent === 'string' ? body.parent : ''
  const name = body?.name

  if (typeof name !== 'string' || !name.trim()) {
    throw createError({ statusCode: 400, message: 'Paramètre name requis' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/folder`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...backendHeaders(event) },
      body: JSON.stringify({ parent, name: name.trim() }),
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
