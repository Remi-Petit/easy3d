// Proxy vers le backend Rust : GET /users
//
// Comptes, rôles disponibles et rôle par défaut, pour l'écran d'administration.
export default defineEventHandler((event) => backendCall(event, '/users'))
