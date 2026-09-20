// Proxy vers le backend Rust : GET /health
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/health`, {
      responseType: 'text',
      headers: backendHeaders(event),
    })
  } catch (err: any) {
    throw backendError(base, err)
  }
})
