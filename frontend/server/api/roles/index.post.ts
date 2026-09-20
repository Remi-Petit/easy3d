// Proxy vers le backend Rust : POST /roles
//
// Création, ou clonage d'un rôle existant (`from`) dont les droits sont recopiés.
export default defineEventHandler((event) => backendCall(event, '/roles'))
