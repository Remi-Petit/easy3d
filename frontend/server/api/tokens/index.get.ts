// Proxy vers le backend Rust : GET /tokens (les jetons du compte connecté)
export default defineEventHandler((event) => backendCall(event, '/tokens'))
