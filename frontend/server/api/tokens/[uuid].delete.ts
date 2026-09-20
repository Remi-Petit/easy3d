// Proxy vers le backend Rust : DELETE /tokens/{uuid}
export default defineEventHandler((event) =>
  backendCall(event, `/tokens/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
