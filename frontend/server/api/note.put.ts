// Proxy vers le backend Rust : PUT /note?path=<rel>
//
// `path` est le chemin relatif d'un fichier (`DemaAuto/boitier.stl`) ou le nom
// d'un dossier (`DemaAuto`). Le backend range la note dans `<models>/.easy3d-notes/`.
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase
  const { path } = getQuery(event)
  const body = await readBody<{ content?: string }>(event)

  try {
    await $fetch(`${base}/note`, {
      method: 'PUT',
      query: { path: String(path ?? '') },
      body: { content: body?.content ?? '' },
    })
    return { ok: true }
  } catch (err: any) {
    // On préserve le code du backend (400 = chemin invalide) ; sinon 502.
    const status = err?.response?.status ?? err?.statusCode
    const code = typeof status === 'number' ? status : 502
    throw createError({
      statusCode: code,
      statusMessage:
        code === 400
          ? 'Chemin de note invalide.'
          : `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
