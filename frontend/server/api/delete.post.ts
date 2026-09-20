// Proxy d'écriture vers le backend Rust : POST /delete {path}
//
// Supprime un fichier **ou un dossier** — dans ce cas avec tout son contenu,
// comme le ferait un explorateur de fichiers (l'interface prévient avant, en
// annonçant le nombre de fichiers). Le backend enlève aussi ce qui accompagnait
// l'élément : sa note (`/.easy3d-notes`) et ses aperçus (`/.easy3d-thumbs`),
// puis rediffuse le catalogue sur le WebSocket — l'interface se met donc à jour
// sans rechargement, y compris là où le watcher ne verrait rien de l'opération
// (partage de fichiers virtualisé, sous Docker Desktop).
//
// Les refus du backend (« introuvable », « chemin réservé »…) sont écrits pour
// l'utilisateur : ils remontent tels quels.
export default defineEventHandler(async (event) => {
  const body = await readBody<{ path?: string }>(event)
  const path = body?.path

  if (typeof path !== 'string' || !path) {
    throw createError({ statusCode: 400, message: 'Paramètre path requis' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    const res = await fetch(`${base}/delete`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...backendHeaders(event) },
      body: JSON.stringify({ path }),
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
