// Proxy vers le backend Rust : PUT /users/{uuid}
//
// Modification **partielle** : seuls les champs envoyés sont appliqués.
export default defineEventHandler((event) =>
  backendCall(event, `/users/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}`),
)
