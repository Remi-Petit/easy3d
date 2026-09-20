// Proxy vers le backend Rust : DELETE /users/{uuid}
export default defineEventHandler((event) =>
  backendCall(event, `/users/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
