import { demarrerKeycloak } from './keycloak'

/**
 * Démarre Keycloak **avant** les serveurs de la campagne.
 *
 * Il n'est pas déclaré dans `webServer` (qui sert à lancer des processus) mais
 * ici : Compose gère l'attente (`--wait` sur le `healthcheck`), et cette
 * fonction vérifie en plus que la découverte annonce bien l'émetteur attendu.
 * Voir `keycloak.ts`.
 */
export default async function globalSetup(): Promise<void> {
  await demarrerKeycloak()
}
