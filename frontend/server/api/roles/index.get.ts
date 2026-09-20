// Proxy vers le backend Rust : GET /roles
//
// Rôles, leurs droits, le nombre de comptes qui les portent et le rôle par défaut.
export default defineEventHandler((event) => backendCall(event, '/roles'))
