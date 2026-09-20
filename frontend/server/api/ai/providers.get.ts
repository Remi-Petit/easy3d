// Proxy vers le backend Rust : GET /ai/providers
//
// Les fournisseurs proposés (identifiant, libellé, besoin d'une clé, adresse et
// modèle par défaut) viennent du backend : l'interface n'en connaît aucun en
// dur, donc ajouter un fournisseur côté Rust suffit à le proposer ici.
export default defineEventHandler(async (event) => {
  const config = useRuntimeConfig(event)
  const base: string = config.hpccatApiBase

  try {
    return await $fetch(`${base}/ai/providers`, {
      responseType: 'json',
      headers: backendHeaders(event),
    })
  } catch (err: any) {
    throw backendError(base, err)
  }
})
