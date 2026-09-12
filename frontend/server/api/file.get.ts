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

    // `?download=1` : le navigateur doit enregistrer le fichier plutôt que
    // l'afficher. Sans cette en-tête, le `<a download>` suffit dans la plupart
    // des cas, mais pas pour une ouverture directe de l'URL (nouvel onglet,
    // clic-milieu).
    if (q.download) {
      setResponseHeader(event, 'content-disposition', attachmentHeader(basename(rel)))
    }

    return Buffer.from(await res.arrayBuffer())
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
