// Proxy vers le backend Rust : PUT /roles/default
//
// Rôle attribué aux nouveaux comptes (il ne redistribue rien à l'existant).
//
// Ce fichier est un **segment statique** : Nitro le fait primer sur
// `[uuid].put.ts`, donc « default » n'est jamais pris pour un UUID de rôle.
export default defineEventHandler((event) => backendCall(event, '/roles/default'))
