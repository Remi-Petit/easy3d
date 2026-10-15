// Proxy vers le backend Rust : GET /print-info?path=<rel>
//
// Ce qu'un slicer annonce sur un fichier : durée, matière, poids, hauteur de
// couche… Le backend le lit **dans le fichier** (en-tête, pied de fichier,
// toolpath) et le ramène au vocabulaire commun des slicers — voir
// `backend/src/formats/print_info.rs`.
//
// Ce n'est pas le catalogue qui le porte : la lecture coûte plusieurs mégaoctets
// pour un G-code, et seul l'écran de détail en a besoin.
export default defineEventHandler(async (event) => {
  const rel = getQuery(event).path
  if (typeof rel !== 'string' || !rel) {
    throw createError({ statusCode: 400, statusMessage: 'Paramètre path manquant' })
  }

  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/print-info`, {
      responseType: 'json',
      query: { path: rel },
      headers: backendHeaders(event),
    })
  } catch (err: any) {
    throw backendError(base, err)
  }
})
