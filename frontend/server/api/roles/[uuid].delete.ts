// Proxy vers le backend Rust : DELETE /roles/{uuid}
export default defineEventHandler((event) =>
  backendCall(event, `/roles/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
