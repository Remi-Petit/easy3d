// Proxy vers le backend Rust : GET /config
//
// Renvoie la configuration **appliquée** et le chemin résolu du dossier des
// modèles (`models_root` peut être vide ou relatif à `backend/`).
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/config`, { responseType: 'json' })
  } catch (err: any) {
    throw createError({
      statusCode: 502,
      message: `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
