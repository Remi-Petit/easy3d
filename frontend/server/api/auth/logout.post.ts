// Proxy vers le backend Rust : POST /auth/logout
//
// La réponse efface le cookie (`Max-Age=0`) : elle doit donc passer telle quelle,
// comme à la connexion.
export default defineEventHandler((event) => authProxy(event, '/auth/logout'))
