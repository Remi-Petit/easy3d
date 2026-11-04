import { arreterKeycloak } from './keycloak'

/**
 * Arrête le Keycloak de la campagne, et efface son disque.
 *
 * Le royaume est **importé** au démarrage : le conserver d'une exécution à
 * l'autre ferait passer les tests sur un royaume modifié à la main, et non sur
 * `docker/keycloak/realm.json`. `E2E_SSO_KEEP=1` garde le conteneur (pour
 * regarder la console Keycloak après un échec).
 */
export default async function globalTeardown(): Promise<void> {
  if (process.env.E2E_SSO_KEEP === '1') {
    console.log('[e2e-sso] Keycloak laissé en marche (E2E_SSO_KEEP=1).')
    return
  }
  arreterKeycloak()
}
