// Proxy vers le backend Rust : DELETE /users/{uuid}/oidc — détachement du SSO.
//
// Il porte un corps (le mot de passe à poser si le compte n'en a pas) : depuis
// le 2026-10-08, `backendCall` le recopie aussi pour `DELETE`, sinon le
// détachement d'un compte sans mot de passe serait refusé pour une raison
// invisible côté navigateur.
export default defineEventHandler((event) =>
  backendCall(
    event,
    `/users/${encodeURIComponent(String(getRouterParam(event, 'uuid')))}/oidc`,
  ),
)
