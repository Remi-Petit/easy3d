/**
 * Réglages partagés de la campagne e2e « authentification ».
 *
 * Un seul endroit pour les ports et les identités : la configuration Playwright
 * les passe au backend, au serveur de développement et au fournisseur fictif, et
 * les tests s'en servent pour leurs assertions. Un identifiant recopié à deux
 * endroits finirait par diverger.
 *
 * Les ports sont choisis **hors de tout ce qui tourne déjà** : le conteneur de
 * développement publie 3100/3101, le backend de l'hôte 8090, le fournisseur
 * fictif du conteneur 8788, et la campagne du catalogue 8091/3200.
 *
 * Ils restent **réglables** (`tests/ports.ts`) pour une machine où l'un d'eux est
 * indisponible : sous Windows, une plage réservée fait échouer le démarrage du
 * backend (`os error 10013`) avant le premier test. Voir `frontend/.env.example`.
 */
import { port } from '../ports'

/** Port du fournisseur d'identité fictif. */
export const IDP_PORT = port('E2E_AUTH_IDP_PORT', 8790)

/** Port du backend Rust de la campagne. */
export const API_PORT = port('E2E_AUTH_API_PORT', 8092)

/** Port du serveur de développement Nuxt de la campagne. */
export const WEB_PORT = port('E2E_AUTH_WEB_PORT', 3201)

/**
 * Adresse du fournisseur **telle que le backend la joint**.
 *
 * Le backend tourne sur l'hôte (pas dans le conteneur) : c'est donc `127.0.0.1`,
 * et non le nom de service `fake-oidc` du compose. La même adresse sert au
 * navigateur, qui visite la page d'autorisation.
 */
export const IDP_ISSUER = `http://127.0.0.1:${IDP_PORT}`

export const IDP_CLIENT_ID = 'easy3d'
export const IDP_CLIENT_SECRET = 'secret-e2e'

/** Origine de l'interface, vue du navigateur : c'est `EASY3D_PUBLIC_URL`. */
export const WEB_ORIGIN = `http://localhost:${WEB_PORT}`

/** Compte administrateur, créé au premier démarrage (`EASY3D_ADMIN_*`). */
export const ADMIN_USERNAME = 'admin'
export const ADMIN_EMAIL = 'admin@e2e.test'
export const ADMIN_PASSWORD = 'mot-de-passe-e2e'

/**
 * Compte simulé décrit par le fournisseur fictif.
 *
 * Il n'existe **pas** avant la première connexion SSO : c'est le provisionnement
 * automatique qui le crée, et le test le vérifie.
 */
export const SSO_SUBJECT = 'sso-e2e-sub'
export const SSO_EMAIL = 'sso@e2e.test'
export const SSO_USERNAME = 'sso-e2e'
