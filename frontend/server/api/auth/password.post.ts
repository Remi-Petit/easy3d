// Proxy vers le backend Rust : POST /auth/password
//
// Chacun change son propre mot de passe (le mot de passe actuel est exigé). La
// réponse **renouvelle la session** : son `Set-Cookie` doit donc revenir intact
// au navigateur, d'où `proxyRequest` (un `$fetch` intermédiaire le perdrait).
export default defineEventHandler((event) => authProxy(event, '/auth/password'))
