#!/usr/bin/env node
/**
 * Auto-test du fournisseur d'identité **réel** (Keycloak) — développement et CI.
 *
 * Il répond à une question différente de `docker/fake-oidc/selftest.mjs` : le
 * fournisseur fictif est **notre** code, on vérifie donc qu'il implémente le
 * protocole ; Keycloak, lui, est un produit — ce qu'il faut vérifier, c'est que
 * **notre royaume** (`realm.json`) est bien celui qu'il a importé, et qu'il
 * expose ce qu'easy3d attend :
 *
 * 1. la **découverte** annonce l'émetteur configuré (le backend refuse le
 *    contraire — garde-fou contre une redirection DNS) et les trois adresses ;
 * 2. les comptes de test **existent** et s'authentifient (grant « mot de passe »
 *    du propriétaire de la ressource, activé exprès sur ce royaume de dev) ;
 * 3. `userinfo` rend `sub`, `email` et `preferred_username` — les trois champs
 *    que `oidc::parse_identity` lit, et sans lesquels aucun compte n'est créé ;
 * 4. un mot de passe erroné donne `invalid_grant` (et **pas** un 500) ;
 * 5. un secret client erroné donne `invalid_client` : easy3d doit traiter ce
 *    code comme **notre** panne de configuration, jamais comme une révocation
 *    (voir `oidc::refresh_refusal`) — c'est exactement la nuance que le test
 *    vérifie côté fournisseur ;
 * 6. le rafraîchissement fonctionne (rotation du `refresh_token`) : c'est lui
 *    qui permet à easy3d de revérifier une session SSO toutes les 5 minutes ;
 * 7. l'adresse de retour de la campagne e2e est **acceptée** à l'autorisation
 *    (un refus `Invalid parameter: redirect_uri` n'apparaît que dans le
 *    navigateur, jamais dans un test de protocole).
 *
 * Le conteneur est démarré par ce script s'il ne répond pas déjà (projet Compose
 * `easy3d-sso`, le même que la campagne e2e), sur `KEYCLOAK_URL`.
 *
 *   node docker/keycloak/selftest.mjs
 *   KEYCLOAK_URL=http://127.0.0.1:8088 node docker/keycloak/selftest.mjs   # instance existante
 *   KEYCLOAK_KEEP=1 node docker/keycloak/selftest.mjs                      # la laisser tourner
 */
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ICI = dirname(fileURLToPath(import.meta.url))
const RACINE = resolve(ICI, '../..')

/**
 * Le royaume **testé** est le fichier importé par le conteneur : tout ce qui
 * suit en est déduit (nom du royaume, comptes, rôles). Ajouter un compte dans
 * `realm.json` l'ajoute donc à l'auto-test — il n'y a pas deux listes à tenir.
 */
const ROYAUME_FICHIER = JSON.parse(readFileSync(resolve(ICI, 'realm.json'), 'utf8'))

/** Base de Keycloak. `127.0.0.1` : le script tourne sur l'hôte, comme le backend. */
const URL_BASE = (process.env.KEYCLOAK_URL ?? 'http://127.0.0.1:18088').replace(/\/+$/, '')
const PORT = new URL(URL_BASE).port || '80'
const PROJET = 'easy3d-sso'
const ROYAUME = ROYAUME_FICHIER.realm
const ISSUER = `${URL_BASE}/realms/${ROYAUME}`
const CLIENT_ID = 'easy3d'
const CLIENT_SECRET = 'secret-keycloak'

/** Comptes décrits par `realm.json` (et rôles que le fournisseur leur donne). */
const COMPTES = ROYAUME_FICHIER.users.map((compte) => ({
  login: compte.username,
  motDePasse: compte.credentials?.[0]?.value ?? '',
  email: compte.email,
  rolesKeycloak: compte.realmRoles ?? [],
}))

const echecs = []

/** Vérifie une condition, en gardant la trace au lieu de s'arrêter au premier échec. */
function verifier(condition, message, detail) {
  if (condition) {
    console.log(`  ok   ${message}`)
    return true
  }
  const complement = detail === undefined ? '' : ` — ${JSON.stringify(detail)}`
  console.error(`  FAIL ${message}${complement}`)
  echecs.push(message)
  return false
}

/** Démarre (ou retrouve) Keycloak et rend `true` s'il faut l'arrêter ensuite. */
async function garantirKeycloak() {
  const decouverte = await essayer(`${ISSUER}/.well-known/openid-configuration`)
  if (decouverte && decouverte.issuer === ISSUER) {
    console.log(`Keycloak répond déjà sur ${ISSUER}`)
    return false
  }
  if (decouverte) {
    throw new Error(
      `un Keycloak répond sur ${URL_BASE} mais annonce « ${decouverte.issuer} » au lieu de « ${ISSUER} ».\n` +
        `Il a été démarré avec un autre KEYCLOAK_URL : docker compose -p ${PROJET} --profile keycloak down -v`,
    )
  }

  console.log(`Démarrage de Keycloak (projet Compose ${PROJET}) sur ${URL_BASE}…`)
  execFileSync(
    'docker',
    ['compose', '-p', PROJET, '--profile', 'keycloak', 'up', '-d', '--wait', '--wait-timeout', '180', 'keycloak'],
    {
      cwd: RACINE,
      env: { ...process.env, KEYCLOAK_URL: URL_BASE, KEYCLOAK_PORT: PORT },
      stdio: 'inherit',
    },
  )
  for (let essai = 0; essai < 60; essai++) {
    const document = await essayer(`${ISSUER}/.well-known/openid-configuration`)
    if (document?.issuer === ISSUER) return true
    await new Promise((suite) => setTimeout(suite, 2000))
  }
  throw new Error(`Keycloak n'annonce pas « ${ISSUER} » après 2 min`)
}

/** `GET` JSON ; `null` si le serveur ne répond pas (ou répond du non-JSON). */
async function essayer(url) {
  try {
    const reponse = await fetch(url, { signal: AbortSignal.timeout(5000) })
    if (!reponse.ok) return null
    return await reponse.json()
  } catch {
    return null
  }
}

/** Échange de formulaire OAuth (le seul format que Keycloak accepte ici). */
async function formulaire(url, champs) {
  const reponse = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams(champs).toString(),
    signal: AbortSignal.timeout(10_000),
  })
  const texte = await reponse.text()
  let corps = null
  try {
    corps = JSON.parse(texte)
  } catch {
    // Un corps non-JSON est déjà une anomalie : on le rend tel quel au test.
  }
  return { statut: reponse.status, corps, texte }
}

async function principal() {
  const demarre = await garantirKeycloak()
  try {
    console.log(`\nRoyaume « ${ROYAUME} » sur ${URL_BASE}`)

    // 1. Découverte
    const decouverte = await essayer(`${ISSUER}/.well-known/openid-configuration`)
    verifier(decouverte?.issuer === ISSUER, 'la découverte annonce l’émetteur attendu', decouverte?.issuer)
    verifier(
      typeof decouverte?.authorization_endpoint === 'string' &&
        typeof decouverte?.token_endpoint === 'string' &&
        typeof decouverte?.userinfo_endpoint === 'string',
      'la découverte annonce autorisation, jeton et userinfo',
    )

    // 7. Adresse de retour acceptée (page de connexion servie, pas « Invalid parameter »)
    const retour = encodeURIComponent('http://localhost:13202/api/auth/oidc/callback')
    const page = await fetch(
      `${decouverte.authorization_endpoint}?response_type=code&client_id=${CLIENT_ID}` +
        `&redirect_uri=${retour}&scope=openid%20email%20profile&state=selftest`,
      { signal: AbortSignal.timeout(10_000) },
    ).then((reponse) => reponse.text())
    verifier(!page.includes('Invalid parameter'), 'l’adresse de retour de la campagne est acceptée')
    verifier(page.includes('name="password"'), 'la page de connexion est bien servie')

    // 2, 3, 4, 5, 6 : les comptes, userinfo, les refus et le rafraîchissement.
    for (const compte of COMPTES) {
      const { statut, corps } = await formulaire(decouverte.token_endpoint, {
        grant_type: 'password',
        client_id: CLIENT_ID,
        client_secret: CLIENT_SECRET,
        username: compte.login,
        password: compte.motDePasse,
        scope: 'openid email profile',
      })
      if (!verifier(statut === 200 && corps?.access_token, `le compte ${compte.login} s’authentifie`, statut)) {
        continue
      }

      const userinfo = await fetch(decouverte.userinfo_endpoint, {
        headers: { authorization: `Bearer ${corps.access_token}` },
        signal: AbortSignal.timeout(10_000),
      }).then((reponse) => reponse.json())
      verifier(userinfo.email === compte.email, `userinfo rend l’adresse de ${compte.login}`, userinfo.email)
      verifier(userinfo.preferred_username === compte.login, `userinfo rend preferred_username de ${compte.login}`)
      verifier(typeof userinfo.sub === 'string' && userinfo.sub.length > 0, `userinfo rend un sub pour ${compte.login}`)

      // Rôles **Keycloak** (importés du fichier) : ils voyagent dans le jeton
      // d'accès, et easy3d les **ignore** volontairement. Les vérifier ici dit
      // que le royaume est bien celui du fichier — la campagne e2e, elle, vérifie
      // qu'ils ne donnent aucun droit dans l'application.
      if (compte.rolesKeycloak.length > 0) {
        const charge = JSON.parse(Buffer.from(corps.access_token.split('.')[1], 'base64url').toString('utf8'))
        const portes = charge.realm_access?.roles ?? []
        verifier(
          compte.rolesKeycloak.every((role) => portes.includes(role)),
          `le jeton de ${compte.login} porte ses rôles Keycloak`,
          { attendus: compte.rolesKeycloak, portes },
        )
      }

      if (compte.login !== 'admin') continue

      // Rafraîchissement : c'est ce que fait easy3d toutes les 5 minutes.
      const rafraichi = await formulaire(decouverte.token_endpoint, {
        grant_type: 'refresh_token',
        client_id: CLIENT_ID,
        client_secret: CLIENT_SECRET,
        refresh_token: corps.refresh_token,
      })
      verifier(rafraichi.statut === 200 && rafraichi.corps?.access_token, 'le rafraîchissement rend un jeton')

      // Mot de passe erroné : un refus propre, au format OAuth. Keycloak répond
      // **401** (la spécification dit 400) avec `invalid_grant` : easy3d lit le
      // corps, pas le code HTTP, pour cette distinction.
      const faux = await formulaire(decouverte.token_endpoint, {
        grant_type: 'password',
        client_id: CLIENT_ID,
        client_secret: CLIENT_SECRET,
        username: compte.login,
        password: 'ce-n-est-pas-le-bon',
        scope: 'openid',
      })
      verifier(
        faux.statut >= 400 && faux.statut < 500 && faux.corps?.error === 'invalid_grant',
        'un mot de passe erroné donne invalid_grant',
        { statut: faux.statut, error: faux.corps?.error },
      )

      // Secret erroné : Keycloak répond `unauthorized_client`. Le point qui
      // compte pour easy3d est **négatif** : ce n'est pas un code de révocation
      // (`oidc::refresh_refusal`), donc un secret mal recopié ne doit fermer
      // aucun compte. On vérifie la liste, ici, sur la réponse réelle.
      const mauvaisSecret = await formulaire(decouverte.token_endpoint, {
        grant_type: 'password',
        client_id: CLIENT_ID,
        client_secret: 'mauvais-secret',
        username: compte.login,
        password: compte.motDePasse,
        scope: 'openid',
      })
      const REVOCATIONS = ['invalid_grant', 'invalid_token', 'expired_token', 'access_denied']
      verifier(
        mauvaisSecret.statut >= 400 &&
          mauvaisSecret.corps?.error &&
          !REVOCATIONS.includes(mauvaisSecret.corps.error),
        'un secret erroné n’est pas lu comme une révocation',
        { statut: mauvaisSecret.statut, error: mauvaisSecret.corps?.error },
      )

      // Révocation réelle : déconnexion côté Keycloak, puis rafraîchissement du
      // même jeton. C'est la réponse que `revalidate_sessions` doit reconnaître
      // (et qui ferme la session et le compte) — donc autant vérifier qu'elle
      // porte bien un des codes de la liste ci-dessus.
      await formulaire(`${ISSUER}/protocol/openid-connect/logout`, {
        client_id: CLIENT_ID,
        client_secret: CLIENT_SECRET,
        refresh_token: corps.refresh_token,
      })
      const apresDeconnexion = await formulaire(decouverte.token_endpoint, {
        grant_type: 'refresh_token',
        client_id: CLIENT_ID,
        client_secret: CLIENT_SECRET,
        refresh_token: corps.refresh_token,
      })
      verifier(
        REVOCATIONS.includes(apresDeconnexion.corps?.error),
        'un jeton révoqué donne un code de révocation',
        { statut: apresDeconnexion.statut, error: apresDeconnexion.corps?.error },
      )
    }
  } finally {
    if (demarre && process.env.KEYCLOAK_KEEP !== '1') {
      console.log('\nArrêt du Keycloak démarré par l’auto-test…')
      execFileSync(
        'docker',
        ['compose', '-p', PROJET, '--profile', 'keycloak', 'down', '-v', '--remove-orphans'],
        { cwd: RACINE, env: { ...process.env, KEYCLOAK_URL: URL_BASE, KEYCLOAK_PORT: PORT }, stdio: 'inherit' },
      )
    }
  }
}

principal().then(
  () => {
    if (echecs.length > 0) {
      console.error(`\n${echecs.length} vérification(s) en échec.`)
      process.exit(1)
    }
    console.log('\nRoyaume Keycloak conforme.')
  },
  (erreur) => {
    console.error(`\nECHEC : ${erreur instanceof Error ? erreur.message : erreur}`)
    process.exit(1)
  },
)
