// Proxy vers le backend Rust : GET /file?path=<rel>[&v=<version>][&download=1]
// Relaie le contenu binaire d'un modèle (STL / OBJ / 3MF / GCODE) au navigateur.
//
// Le proxy **conserve les en-têtes de cache** du backend (`etag`, `cache-control`,
// `last-modified`) et lui transmet `If-None-Match`. C'est le backend qui connaît
// la version du fichier sur disque : c'est cette comparaison qui produit les
// `304`. Ne relayer que le `content-type` — ce que faisait ce fichier — rendait
// tout cache impossible, et chaque vignette était retéléchargée à chaque
// chargement de page.
//
// Le corps est **diffusé** (`sendStream`) au lieu d'être chargé en mémoire : un
// G-code de plusieurs mégaoctets ne doit pas passer par un `arrayBuffer()`.
export default defineEventHandler(async (event) => {
  const q = getQuery(event)
  const rel = q.path
  if (typeof rel !== 'string' || !rel) {
    throw createError({ statusCode: 400, statusMessage: 'Paramètre path manquant' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  const upstream = new URL(`${base}/file`)
  upstream.searchParams.set('path', rel)
  // Version annoncée par le scan : recopiée telle quelle, elle vaut « cette URL
  // ne changera plus » pour le backend (réponse `immutable`).
  if (typeof q.v === 'string' && q.v) {
    upstream.searchParams.set('v', q.v)
  }

  // Ce que le navigateur a déjà en cache : transmis au backend, seul juge.
  const ifNoneMatch = getRequestHeader(event, 'if-none-match')
  const ifModifiedSince = getRequestHeader(event, 'if-modified-since')

  try {
    const res = await fetch(upstream, {
      headers: {
        ...(ifNoneMatch ? { 'if-none-match': ifNoneMatch } : {}),
        ...(ifModifiedSince ? { 'if-modified-since': ifModifiedSince } : {}),
        ...backendHeaders(event),
      },
    })

    // En-têtes recopiés du backend : il est le seul à connaître la version.
    const relayed: Record<string, string> = {}
    for (const name of ['content-type', 'etag', 'cache-control', 'last-modified']) {
      const value = res.headers.get(name)
      if (value) relayed[name] = value
    }

    // `?download=1` : le navigateur doit enregistrer le fichier plutôt que
    // l'afficher. Sans cette en-tête, le `<a download>` suffit dans la plupart
    // des cas, mais pas pour une ouverture directe de l'URL (nouvel onglet,
    // clic-milieu).
    if (q.download) {
      relayed['content-disposition'] = attachmentHeader(basename(rel))
    }

    // Rien à transmettre : le cache du navigateur fait foi.
    if (res.status === 304) {
      setResponseStatus(event, 304)
      setResponseHeaders(event, relayed)
      return null
    }

    if (!res.ok) {
      throw createError({
        statusCode: res.status,
        statusMessage: `Backend a répondu ${res.status}`,
      })
    }

    setResponseHeaders(event, relayed)
    // Flux direct vers la réponse : rien n'est bufferisé côté Nitro.
    return res.body ? sendStream(event, res.body) : null
  } catch (err: any) {
    throw createError({
      statusCode: err?.statusCode || 502,
      statusMessage: `Backend indisponible (${base}).`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})

/**
 * En-tête `Content-Disposition` pour un nom de fichier quelconque : un repli
 * ASCII (caractères non imprimables et guillemets neutralisés) *plus* la forme
 * `filename*` en UTF-8, que les navigateurs modernes préfèrent — c'est elle qui
 * conserve les accents (« Boitier déma auto.stl »).
 */
function attachmentHeader(name: string): string {
  const ascii = name.replace(/[^\x20-\x7e]/g, '_').replace(/["\\]/g, '_')
  return `attachment; filename="${ascii}"; filename*=UTF-8''${encodeURIComponent(name)}`
}

/** Dernier segment d'un chemin, quel que soit le séparateur. */
function basename(p: string): string {
  return p.split(/[\\/]/).pop() || p
}
