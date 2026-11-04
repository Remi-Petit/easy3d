/**
 * Réglages partagés de la campagne e2e « SSO réel » (Keycloak).
 *
 * Même principe que `tests/e2e-auth/params.ts` : les ports et les identités sont
 * écrits **une fois**, la configuration Playwright les passe au backend, au
 * serveur de développement et à Keycloak, et les tests s'en servent pour leurs
 * assertions.
 *
 * # Pourquoi une troisième campagne
 *
 * `e2e-auth` éprouve la chaîne complète contre `docker/fake-oidc`, un
 * fournisseur qui **accepte tout sans mot de passe** : il prouve le câblage
 * (adresse publique, `redirect_uri`, cookie, provisionnement) mais il ne prouve
 * pas qu'easy3d parle à un **vrai** fournisseur OIDC — découverte signée,
 * formulaire de connexion, jetons, `userinfo` avec les revendications réelles.
 * C'est ce que fait cette campagne, contre Keycloak.
 *
 * # Une seule adresse pour Keycloak
 *
 * `EASY3D_OIDC_ISSUER` doit être **exactement** l'émetteur que Keycloak annonce
 * (le backend refuse une découverte qui annonce autre chose, garde-fou contre
 * une redirection DNS). Keycloak annonce `KEYCLOAK_URL/realms/easy3d`, et le
 * backend tourne ici **sur l'hôte** : on prend donc `127.0.0.1`, joignable par
 * le backend comme par le navigateur. (Dans le conteneur, cette base serait
 * `http://host.docker.internal:<port>` — voir `docker/keycloak/README.md`.)
 */
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { port } from '../ports'

/**
 * Port publié par le conteneur Keycloak (`KEYCLOAK_PORT` du compose).
 *
 * Il est **différent** de celui du compose de développement (8088) : la
 * campagne recrée son propre conteneur (projet Compose `easy3d-sso`), et deux
 * instances ne doivent pas se disputer le port.
 */
export const KC_PORT = port('E2E_SSO_IDP_PORT', 18088)

/** Base de Keycloak, vue par le backend **et** par le navigateur. */
export const KC_URL = `http://127.0.0.1:${KC_PORT}`

/**
 * Le royaume **importé** par le conteneur, lu à la source.
 *
 * `docker/keycloak/realm.json` est la seule liste des comptes de test : en
 * ajouter un là-bas l'ajoute ici, à l'auto-test du royaume et à Keycloak. Une
 * seconde liste écrite à la main finirait par divergner du fichier importé —
 * et c'est justement le genre d'écart qu'on ne voit qu'au moment où le test
 * échoue pour une raison qui n'a rien à voir.
 */
interface RoyaumeFichier {
  realm: string
  users: Array<{
    username: string
    email: string
    credentials?: Array<{ value?: string }>
    realmRoles?: string[]
  }>
}

const ROYAUME: RoyaumeFichier = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../../docker/keycloak/realm.json'),
    'utf8',
  ),
)

/** Royaume importé (`docker/keycloak/realm.json`). */
export const KC_REALM = ROYAUME.realm

/** Un compte tel que Keycloak le connaît (mot de passe et rôles compris). */
export interface CompteKeycloak {
  login: string
  email: string
  motDePasse: string
  /** Rôles **Keycloak** — easy3d ne les lit pas (voir plus bas). */
  rolesKeycloak: string[]
}

/** Tous les comptes du royaume, dans l'ordre du fichier. */
export const COMPTES_KEYCLOAK: CompteKeycloak[] = ROYAUME.users.map((compte) => ({
  login: compte.username,
  email: compte.email,
  motDePasse: compte.credentials?.[0]?.value ?? '',
  rolesKeycloak: compte.realmRoles ?? [],
}))

/** Un compte du royaume ; une faute de frappe s'arrête tout de suite, en le disant. */
export function compteKeycloak(login: string): CompteKeycloak {
  const compte = COMPTES_KEYCLOAK.find((candidat) => candidat.login === login)
  if (!compte) {
    throw new Error(
      `compte « ${login} » absent de docker/keycloak/realm.json (comptes : ` +
        COMPTES_KEYCLOAK.map((candidat) => candidat.login).join(', '),
    )
  }
  return compte
}

/** Émetteur OIDC : `EASY3D_OIDC_ISSUER`, et ce que la découverte doit annoncer. */
export const KC_ISSUER = `${KC_URL}/realms/${KC_REALM}`

export const KC_CLIENT_ID = 'easy3d'
export const KC_CLIENT_SECRET = 'secret-keycloak'

/** Port du backend Rust de la campagne. */
export const API_PORT = port('E2E_SSO_API_PORT', 8093)

/** Port du serveur de développement Nuxt de la campagne. */
export const WEB_PORT = port('E2E_SSO_WEB_PORT', 3202)

/** Origine de l'interface, vue du navigateur : c'est `EASY3D_PUBLIC_URL`. */
export const WEB_ORIGIN = `http://localhost:${WEB_PORT}`

/**
 * Compte administrateur **local**, créé au premier démarrage (`EASY3D_ADMIN_*`).
 *
 * Son adresse est celle du compte `admin` de Keycloak : la connexion SSO
 * **rattache** ce compte au lieu d'en créer un second — c'est le cas concret
 * d'une installation qui existe déjà quand on branche l'annuaire.
 */
export const ADMIN_USERNAME = 'admin'
export const ADMIN_EMAIL = compteKeycloak('admin').email
export const ADMIN_PASSWORD = 'mot-de-passe-e2e'

/** Compte Keycloak qui correspond à l'administrateur local (`realm.json`). */
export const KC_ADMIN_LOGIN = 'admin'
export const KC_ADMIN_PASSWORD = compteKeycloak('admin').motDePasse

/**
 * Compte Keycloak **inconnu** d'easy3d au départ : c'est le provisionnement
 * automatique qui crée le compte local, avec le rôle livré `lecteur`.
 */
export const KC_USER_LOGIN = 'user'
export const KC_USER_PASSWORD = compteKeycloak('user').motDePasse
export const KC_USER_EMAIL = compteKeycloak('user').email
/** Nom d'utilisateur local attendu (`preferred_username` de Keycloak). */
export const KC_USERNAME = 'user'

/**
 * Les trois comptes qui montrent que les droits sont **locaux**.
 *
 * Keycloak leur donne des rôles (`imprimeur`, `admin`) : easy3d n'en lit aucun.
 * Ceux qui comptent sont ceux qu'un administrateur pose **ici**, dans
 * *Administration → Comptes* — et c'est ce que vérifie la campagne.
 */
export const KC_IMPRIMEUSE = compteKeycloak('imprimeuse')
export const KC_OBSERVATRICE = compteKeycloak('observatrice')
export const KC_SUPERUSER = compteKeycloak('superuser')

/** Projet Compose de la campagne : ses conteneurs ne touchent pas ceux du dev. */
export const COMPOSE_PROJECT = 'easy3d-sso'

/**
 * Fournisseur **supplémentaire** de la campagne : `docker/fake-oidc`, lancé par
 * Node (même script que le compose de développement), sur son propre port.
 *
 * Il vit à côté de Keycloak sans le remplacer : c'est ce que la page de
 * connexion doit proposer, deux boutons. Ses comptes ont des adresses
 * **distinctes** de celles du royaume — un compte n'appartient qu'à un
 * fournisseur (`oidc_conflict` sinon).
 */
export const EXTRA_PORT = port('E2E_SSO_EXTRA_PORT', 18792)
export const EXTRA_ORIGIN = `http://127.0.0.1:${EXTRA_PORT}`
/** Identifiant d'URL du fournisseur (`?provider=…`). */
export const EXTRA_ID = 'fake'
export const EXTRA_LABEL = 'fake-oidc'
export const EXTRA_CLIENT_SECRET = 'secret-e2e-fake'
export const EXTRA_EMAIL = 'fictif@e2e.test'
export const EXTRA_USERNAME = 'fictif-e2e'
export const EXTRA_SUBJECT = 'fake-e2e-sub'
