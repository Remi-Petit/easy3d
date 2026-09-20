// Proxy vers le backend Rust : PUT /roles/{uuid}
//
// Refusé pour les rôles livrés (`admin`, `lecteur`) : ils sont figés, on les clone.
export default defineEventHandler((event) =>
  backendCall(event, `/roles/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
