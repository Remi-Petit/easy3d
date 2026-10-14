// Proxy vers le backend Rust : GET /sessions (les sessions du compte connecté)
export default defineEventHandler((event) => backendCall(event, '/sessions'))
