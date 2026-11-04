/**
 * Keycloak de la campagne e2e « SSO réel » : démarrage, attente, arrêt.
 *
 * Le fournisseur est le **même service Compose** que celui du développement
 * (`docker-compose.yml`, profil `keycloak`, `docker/keycloak/realm.json`) : un
 * seul royaume, une seule définition, deux usages. La campagne lui donne un
 * **projet** Compose à elle (`easy3d-sso`) et un **port** à elle : elle peut
 * donc tourner pendant qu'un Keycloak de développement est déjà démarré, sans
 * écraser ni son conteneur ni son port.
 *
 * ⚠️ Le nom d'hôte annoncé par Keycloak (`KC_HOSTNAME`) doit être celui que le
 * backend et le navigateur utilisent — ici `127.0.0.1:<port>`, puisque le
 * backend tourne sur l'hôte. C'est `KEYCLOAK_URL` qui le fixe, et il est donc
 * passé **à chaque** commande Compose : un conteneur démarré avec une autre
 * valeur annoncerait un émetteur que le backend refuserait.
 */
import { execFileSync } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { COMPOSE_PROJECT, KC_ISSUER, KC_PORT, KC_URL } from './params'

/** Racine du dépôt (le compose y vit). */
const RACINE = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')

/** Environnement de Compose : l'adresse et le port de **cette** campagne. */
function envCompose(): NodeJS.ProcessEnv {
  return {
    ...process.env,
    KEYCLOAK_URL: KC_URL,
    KEYCLOAK_PORT: String(KC_PORT),
  }
}

/** Lance une commande Compose, en échouant avec un message lisible. */
function compose(args: string[]): void {
  try {
    execFileSync('docker', ['compose', '-p', COMPOSE_PROJECT, '--profile', 'keycloak', ...args], {
      cwd: RACINE,
      env: envCompose(),
      stdio: 'inherit',
    })
  } catch (erreur) {
    const detail = erreur instanceof Error ? erreur.message : String(erreur)
    throw new Error(
      `Keycloak n'a pas pu démarrer (docker compose). Docker Desktop est-il lancé ?\n${detail}`,
    )
  }
}

/**
 * Démarre Keycloak et attend qu'il réponde **avec le bon émetteur**.
 *
 * `--wait` attend le `healthcheck` du service ; la boucle qui suit vérifie ce
 * que le healthcheck ne sait pas : que la découverte est servie **et** qu'elle
 * annonce l'émetteur attendu. Un conteneur recréé avec une autre `KC_HOSTNAME`
 * (reste d'une exécution précédente avec un autre port) est attrapé ici, avec
 * un message qui dit quoi faire, au lieu d'un `invalid_state` incompréhensible
 * trois écrans plus loin.
 */
export async function demarrerKeycloak(): Promise<void> {
  // `--force-recreate` : le royaume est **importé** au démarrage, et le nom
  // d'hôte est figé dans la configuration du conteneur. Réutiliser un conteneur
  // d'un autre port donnerait un émetteur périmé.
  compose(['up', '-d', '--wait', '--wait-timeout', '180', '--force-recreate', 'keycloak'])
  await attendreDecouverte()
}

/** Arrête le Keycloak de la campagne (et son disque, donc le royaume réimporté). */
export function arreterKeycloak(): void {
  compose(['down', '-v', '--remove-orphans'])
}

/** Attend que `KC_ISSUER/.well-known/openid-configuration` annonce bien `KC_ISSUER`. */
async function attendreDecouverte(): Promise<void> {
  const url = `${KC_ISSUER}/.well-known/openid-configuration`
  const limite = Date.now() + 120_000
  let derniere = 'aucune réponse'
  while (Date.now() < limite) {
    try {
      const reponse = await fetch(url, { signal: AbortSignal.timeout(5000) })
      if (reponse.ok) {
        const doc = (await reponse.json()) as { issuer?: string }
        if (doc.issuer === KC_ISSUER) return
        derniere = `la découverte annonce « ${doc.issuer} » au lieu de « ${KC_ISSUER} »`
      } else {
        derniere = `HTTP ${reponse.status}`
      }
    } catch (erreur) {
      derniere = erreur instanceof Error ? erreur.message : String(erreur)
    }
    await new Promise((suite) => setTimeout(suite, 2000))
  }
  throw new Error(
    `Keycloak ne répond pas correctement sur ${url} après 2 min (${derniere}).\n` +
      `Un conteneur démarré avec un autre KEYCLOAK_URL ? ` +
      `docker compose -p ${COMPOSE_PROJECT} --profile keycloak down -v`,
  )
}
