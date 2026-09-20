// Proxy vers le backend Rust : GET /auth/me
//
// Répond toujours 200, même sans session ni authentification activée : c'est
// l'appel que l'interface fait au démarrage pour savoir s'il faut une page de
// connexion, et un 401 y serait un cas d'erreur pour une réponse qui est en
// réalité une information normale.
export default defineEventHandler((event) => authProxy(event, '/auth/me'))
