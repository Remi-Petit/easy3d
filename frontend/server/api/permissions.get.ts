// Proxy vers le backend Rust : GET /permissions
//
// Le catalogue des droits vient du backend : l'interface d'administration
// construit ses cases à cocher à partir de lui, donc ajouter un droit côté Rust
// suffit à le proposer ici (même principe que les formats et les fournisseurs).
export default defineEventHandler((event) => backendCall(event, '/permissions'))
