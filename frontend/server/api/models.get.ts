// Proxy vers le backend Rust : GET /models
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/models`, {
      responseType: 'json',
      headers: backendHeaders(event),
    })
  } catch (err: any) {
    throw backendError(base, err)
  }
})
