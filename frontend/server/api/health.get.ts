// Proxy vers le backend Rust : GET /health
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/health`, { responseType: 'text' })
  } catch (err: any) {
    throw createError({
      statusCode: 502,
      statusMessage: `Backend indisponible (${base}).`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
