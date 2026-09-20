// Proxy vers le backend Rust : POST /auth/login
//
// Le `Set-Cookie` de la réponse doit revenir tel quel au navigateur (voir
// `server/utils/authProxy.ts`), sinon la connexion réussit sans session.
export default defineEventHandler((event) => authProxy(event, '/auth/login'))
