// Proxy vers le backend Rust : GET /models
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/models`, { responseType: 'json' })
  } catch (err: any) {
    throw createError({
      statusCode: 502,
      statusMessage: `Backend indisponible (${base}). Vérifiez que la crête Rust tourne.`,
      data: { cause: err?.message ?? String(err) },
    })
  }
})
