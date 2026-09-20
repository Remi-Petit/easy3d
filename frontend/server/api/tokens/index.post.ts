// Proxy vers le backend Rust : POST /tokens
//
// La réponse porte le jeton **en clair** : c'est la seule fois où il existe
// ailleurs qu'en empreinte (le backend ne stocke que celle-ci).
export default defineEventHandler((event) => backendCall(event, '/tokens'))
