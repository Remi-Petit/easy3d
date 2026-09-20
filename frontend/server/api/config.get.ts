// Proxy vers le backend Rust : GET /config
//
// Renvoie la configuration **appliquée** et le chemin résolu du dossier des
// modèles (`models_root` peut être vide ou relatif à `backend/`).
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/config`, {
      responseType: 'json',
      headers: backendHeaders(event),
    })
  } catch (err: any) {
    throw backendError(base, err)
  }
})
