// Proxy vers le backend Rust : DELETE /sessions/{uuid}
export default defineEventHandler((event) =>
  backendCall(event, `/sessions/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
