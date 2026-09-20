// Proxy vers le backend Rust : POST /users
export default defineEventHandler((event) => backendCall(event, '/users'))
