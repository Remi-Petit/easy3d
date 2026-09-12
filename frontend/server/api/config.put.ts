// Proxy vers le backend Rust : PUT /config
//
// Le backend valide le dossier des modèles, écrit `backend/config.yml`, et le
// watcher applique le changement à chaud (dossier surveillé, mode d'affichage)
// puis rediffuse la nouvelle configuration sur le WebSocket.
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase
  const body = await readBody(event)

  try {
    return await $fetch(`${base}/config`, { method: 'PUT', body })
  } catch (err: any) {
    // 400 « dossier introuvable » : on remonte le message métier tel quel.
    const status: number = err?.response?.status ?? 502
    const detail = typeof err?.data === 'string' ? err.data : (err?.message ?? String(err))

    throw createError({
      statusCode: status,
      // `message` plutôt que `statusMessage` : h3 réserve ce dernier aux
      // messages courts et le nettoiera dans une prochaine version.
      message:
        status === 502
          ? `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`
          : detail,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
