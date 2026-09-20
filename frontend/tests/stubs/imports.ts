/**
 * Doublure de `#imports` pour les tests unitaires.
 *
 * `#imports` est un alias fourni par Nuxt/Nitro à l'exécution : sans lui, un
 * module de `server/` ne peut pas être importé par vitest. Les tests des
 * utilitaires du relais WebSocket n'ont besoin que de la configuration lue par
 * `useRuntimeConfig`, on la leur fournit donc ici — modifiable, pour vérifier ce
 * que donne une adresse avec ou sans slash final.
 */
export const runtimeConfig = {
  hpccatApiBase: 'http://127.0.0.1:8090',
}

export function useRuntimeConfig() {
  return runtimeConfig
}
