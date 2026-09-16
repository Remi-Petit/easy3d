// Proxy d'écriture vers le backend Rust : POST /upload?path=<rel>
//
// Le corps de la requête **est** le fichier : il est transmis en flux, sans
// `multipart` et sans être mis en mémoire côté Nitro — un G-code de plusieurs
// centaines de mégaoctets ne doit pas transiter par un buffer.
//
// Le backend valide le chemin, crée les dossiers manquants et écrit le fichier
// de façon atomique (temporaire caché + `rename`). Le watcher détecte ensuite le
// nouveau fichier et rediffuse le catalogue sur le WebSocket : l'interface se
// met à jour toute seule, sans rechargement.
export default defineEventHandler(async (event) => {
  const q = getQuery(event)
  const rel = q.path
  if (typeof rel !== 'string' || !rel) {
    throw createError({ statusCode: 400, statusMessage: 'Paramètre path manquant' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  // `duplex: 'half'` est exigé par fetch (undici) pour un corps en flux ; il
  // manque au type `RequestInit`, d'où l'assertion.
  const request = {
    method: 'POST',
    body: getRequestWebStream(event),
    duplex: 'half',
    headers: {
      'content-type': getRequestHeader(event, 'content-type') || 'application/octet-stream',
    },
  } as RequestInit

  try {
    const res = await fetch(`${base}/upload?path=${encodeURIComponent(rel)}`, request)

    if (!res.ok) {
      // Les refus du backend (« chemin réservé », « un dossier porte déjà ce
      // nom »…) sont destinés à l'utilisateur : on remonte son message tel quel.
      const detail = (await res.text().catch(() => '')).trim()
      throw createError({
        statusCode: res.status,
        // `message` plutôt que `statusMessage` : h3 réserve ce dernier aux
        // messages courts et le nettoiera dans une prochaine version (même
        // convention que `config.put.ts`).
        message: detail || `Backend a répondu ${res.status}`,
      })
    }

    setResponseStatus(event, 201)
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
