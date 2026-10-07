/**
 * Fournisseur d'identité OIDC **fictif** — développement uniquement.
 *
 * Il existe pour exercer le SSO d'easy3d (`backend/src/auth/oidc.rs`) sans
 * compte chez un vrai fournisseur : il annonce ses adresses, accepte toute
 * connexion **sans mot de passe** et décrit toujours le même compte simulé.
 *
 * # Ce qu'il implémente, et rien de plus
 *
 * Le backend n'a besoin que de quatre choses, et ce fichier ne fait que ça :
 *
 * 1. `GET /.well-known/openid-configuration` — la découverte, qui annonce les
 *    trois adresses ci-dessous ;
 * 2. `GET /authorize` — la page du fournisseur : ici, elle redirige
 *    **immédiatement** vers `redirect_uri` avec un `code` (aucune saisie) ;
 * 3. `POST /token` — l'échange `code` → `access_token` (formulaire
 *    `application/x-www-form-urlencoded`, `client_secret_post`) ;
 * 4. `GET /userinfo` — l'identité, lue par le backend avec le jeton porteur.
 *
 * Le `id_token` n'est **pas** produit : easy3d ne le vérifie pas (voir l'en-tête
 * de `oidc.rs`) et lit l'identité dans `userinfo`.
 *
 * # Les deux adresses d'une même instance
 *
 * Le backend et le navigateur ne voient pas le serveur de la même façon : le
 * premier l'atteint par le nom de service Compose (`http://fake-oidc:8788`),
 * le second par le port publié (`http://localhost:8788`). Seule
 * `authorization_endpoint` est visitée par le navigateur — les adresses
 * `token` et `userinfo` sont appelées par le backend — et la découverte
 * distingue donc les deux :
 *
 *   - `authorization_endpoint` = `PUBLIC_URL` (vue par le navigateur) ;
 *   - `token_endpoint` / `userinfo_endpoint` = hôte de la requête de découverte
 *     (l'adresse que le backend vient d'utiliser, donc joignable par lui).
 *
 * # Réglages (environnement)
 *
 *   PORT            port d'écoute (défaut 8788)
 *   PUBLIC_URL      base vue par le navigateur (défaut http://localhost:<PORT>)
 *   CLIENT_ID       client attendu (défaut easy3d) ; vide = n'importe lequel
 *   CLIENT_SECRET   secret attendu ; vide = aucun contrôle
 *   ACCOUNTS        plusieurs comptes simulés, en JSON :
 *                   `[{"id":"admin","email":"…","username":"…"}, …]`
 *                   Au-delà d'un compte, `/authorize` propose de choisir.
 *   SUBJECT/EMAIL/USERNAME/NAME   le compte simulé **unique** (sans `ACCOUNTS`)
 *
 * Tout est journalisé sur la sortie du conteneur (`docker compose logs -f fake-oidc`).
 */
import { createServer } from 'node:http'
import { randomBytes } from 'node:crypto'

const PORT = Number(process.env.PORT ?? 8788)
const PUBLIC_URL = (process.env.PUBLIC_URL ?? `http://localhost:${PORT}`).replace(/\/+$/, '')
const CLIENT_ID = process.env.CLIENT_ID ?? 'easy3d'
const CLIENT_SECRET = process.env.CLIENT_SECRET ?? ''

/**
 * Comptes simulés, dans l'ordre où la page de choix les propose.
 *
 * `ACCOUNTS` (JSON) en décrit plusieurs ; sinon `SUBJECT`/`EMAIL`/`USERNAME`/
 * `NAME` décrivent l'unique compte. Avec un seul compte, `/authorize` redirige
 * aussitôt (comportement d'origine) ; au-delà, il propose de choisir.
 */
const ACCOUNTS = comptesDepuisEnv()

function comptesDepuisEnv() {
  const brut = (process.env.ACCOUNTS ?? '').trim()
  if (brut) {
    let liste
    try {
      liste = JSON.parse(brut)
    } catch {
      throw new Error(`ACCOUNTS n'est pas du JSON valide : ${brut}`)
    }
    if (!Array.isArray(liste) || liste.length === 0) {
      throw new Error('ACCOUNTS doit être un tableau JSON non vide')
    }
    return liste.map((compte, index) => {
      const username = String(compte.username ?? compte.preferred_username ?? `compte-${index + 1}`)
      return {
        id: String(compte.id ?? username),
        sub: String(compte.sub ?? `fake-oidc-${index + 1}`),
        email: String(compte.email ?? '').toLowerCase(),
        preferred_username: username,
        name: String(compte.name ?? username),
      }
    })
  }
  const username = process.env.USERNAME ?? 'sso'
  return [
    {
      id: username,
      sub: process.env.SUBJECT ?? 'fake-oidc-1',
      email: (process.env.EMAIL ?? 'sso@example.com').toLowerCase(),
      preferred_username: username,
      name: process.env.NAME ?? username,
    },
  ]
}

/** Durée de vie annoncée du jeton d'accès, en secondes. */
const TOKEN_TTL = 3600
/** Durée de vie d'un code d'autorisation, en millisecondes. */
const CODE_TTL = 120_000

/** Codes d'autorisation en cours : `code` → demande. */
const codes = new Map()
/** Jetons d'accès en cours : `access_token` → compte décrit. */
const tokens = new Map()

const log = (...args) => console.log(new Date().toISOString(), ...args)

// ── Utilitaires HTTP ─────────────────────────────────────────────────────

/** Réponse JSON (ou texte) minimale. */
function send(res, status, body, type = 'application/json') {
  const payload = type === 'application/json' ? JSON.stringify(body, null, 2) : String(body)
  res.writeHead(status, {
    'content-type': type,
    'content-length': Buffer.byteLength(payload),
    'cache-control': 'no-store',
  })
  res.end(payload)
}

/** Refus OIDC, au format que le backend sait lire (`error`). */
function oidcError(res, status, error, description) {
  send(res, status, { error, error_description: description })
}

/** Corps complet d'une requête, en texte. */
function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = ''
    req.setEncoding('utf8')
    req.on('data', (chunk) => {
      data += chunk
      if (data.length > 1_000_000) req.destroy()
    })
    req.on('end', () => resolve(data))
    req.on('error', reject)
  })
}

/** Un client qui repart chez lui avec une erreur : `redirect_uri?error=…&state=…`. */
function redirectError(res, redirectUri, error, state) {
  const target = new URL(redirectUri)
  target.searchParams.set('error', error)
  if (state) target.searchParams.set('state', state)
  log(`refus : ${error} → ${target}`)
  res.writeHead(302, { location: target.toString() })
  res.end()
}

// ── Les quatre routes du flux ────────────────────────────────────────────

/** 1. Découverte : les adresses, telles que le backend les lira. */
function discovery(req, res) {
  // L'adresse que le backend vient d'utiliser pour nous joindre.
  const internal = `http://${req.headers.host ?? `127.0.0.1:${PORT}`}`
  const document = {
    issuer: internal,
    authorization_endpoint: `${PUBLIC_URL}/authorize`,
    token_endpoint: `${internal}/token`,
    userinfo_endpoint: `${internal}/userinfo`,
    response_types_supported: ['code'],
    grant_types_supported: ['authorization_code'],
    subject_types_supported: ['public'],
    scopes_supported: ['openid', 'email', 'profile'],
  }
  log(`découverte → token/userinfo sur ${internal}, autorisation sur ${PUBLIC_URL}`)
  send(res, 200, document)
}

/**
 * 2. Autorisation : aucune saisie. Un seul compte → on repart aussitôt avec un
 * code ; plusieurs → on propose d'abord lequel (`?as=<id>`).
 */
function authorize(url, req, res) {
  const redirectUri = url.searchParams.get('redirect_uri')
  const state = url.searchParams.get('state')
  const clientId = url.searchParams.get('client_id')

  if (!redirectUri) {
    return oidcError(res, 400, 'invalid_request', '« redirect_uri » manquant')
  }
  if (CLIENT_ID && clientId !== CLIENT_ID) {
    return redirectError(res, redirectUri, 'unauthorized_client', state)
  }

  const demande = url.searchParams.get('as')
  const compte = ACCOUNTS.find((a) => a.id === demande)
  // Plusieurs comptes et aucun choix valable : on affiche la page de choix.
  if (!compte && ACCOUNTS.length > 1) {
    return choisirCompte(url, res)
  }
  const choisi = compte ?? ACCOUNTS[0]

  const code = randomBytes(24).toString('hex')
  codes.set(code, {
    redirectUri,
    clientId,
    account: choisi,
    expiresAt: Date.now() + CODE_TTL,
  })
  log(`autorisation : client=${clientId} compte=${choisi.email} → ${redirectUri} (code ${code.slice(0, 12)}…)`)

  const target = new URL(redirectUri)
  target.searchParams.set('code', code)
  if (state) target.searchParams.set('state', state)
  res.writeHead(302, { location: target.toString() })
  res.end()
}

/** Échappe un texte inséré dans du HTML (noms et adresses viennent de l'environnement). */
function echapper(texte) {
  return String(texte).replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
  )
}

/**
 * Page de choix du compte, quand `ACCOUNTS` en décrit plusieurs.
 *
 * Chaque entrée renvoie la **même** demande d'autorisation avec `as=<id>` : le
 * navigateur reste chez le fournisseur, et le code délivré désigne ensuite le
 * compte choisi.
 */
function choisirCompte(url, res) {
  const liens = ACCOUNTS.map((compte) => {
    const cible = new URL(url.toString())
    cible.searchParams.set('as', compte.id)
    return `<li><a href="${echapper(cible.pathname + cible.search)}"><strong>${echapper(compte.name)}</strong> <small>${echapper(compte.email)}</small></a></li>`
  }).join('\n    ')
  log(`choix du compte (${ACCOUNTS.length}) → ${url.searchParams.get('redirect_uri')}`)
  send(
    res,
    200,
    `<!doctype html>
<meta charset="utf-8">
<title>easy3d — choisir un compte</title>
<style>
  body { font: 15px/1.6 system-ui, sans-serif; margin: 3rem auto; max-width: 30rem; padding: 0 1rem; }
  ul { list-style: none; padding: 0; display: grid; gap: .5rem; }
  a { display: block; padding: .7rem 1rem; border: 1px solid #d0d0d0; border-radius: 8px; text-decoration: none; color: inherit; }
  a:hover { border-color: #888; }
  small { color: #777; }
</style>
<h1>Choisir un compte</h1>
<p>Fournisseur d'identité fictif (développement) — aucun mot de passe n'est demandé.</p>
<ul>
    ${liens}
</ul>`,
    'text/html; charset=utf-8',
  )
}

/** 3. Échange du code contre un jeton d'accès. */
async function token(req, res) {
  const form = new URLSearchParams(await readBody(req))

  // Le client peut s'authentifier par le corps (`client_secret_post`) ou par
  // l'en-tête `Authorization: Basic` — easy3d utilise le corps.
  let clientId = form.get('client_id')
  let clientSecret = form.get('client_secret')
  const basic = req.headers.authorization
  if (basic?.startsWith('Basic ')) {
    const [id, secret] = Buffer.from(basic.slice(6), 'base64').toString('utf8').split(':')
    clientId = clientId ?? id
    clientSecret = clientSecret ?? secret
  }

  if (form.get('grant_type') !== 'authorization_code') {
    return oidcError(res, 400, 'unsupported_grant_type', 'seul « authorization_code » est accepté')
  }
  if (CLIENT_ID && clientId !== CLIENT_ID) {
    return oidcError(res, 401, 'invalid_client', `client_id « ${clientId} » inconnu`)
  }
  if (CLIENT_SECRET && clientSecret !== CLIENT_SECRET) {
    return oidcError(res, 401, 'invalid_client', 'client_secret incorrect')
  }

  const code = form.get('code')
  const demande = code && codes.get(code)
  if (!demande) {
    return oidcError(res, 400, 'invalid_grant', 'code inconnu ou déjà utilisé')
  }
  codes.delete(code)
  if (demande.expiresAt < Date.now()) {
    return oidcError(res, 400, 'invalid_grant', 'code expiré')
  }
  if (form.get('redirect_uri') !== demande.redirectUri) {
    return oidcError(res, 400, 'invalid_grant', '« redirect_uri » différent de celui de l’autorisation')
  }

  const accessToken = randomBytes(32).toString('hex')
  tokens.set(accessToken, { account: { ...demande.account }, expiresAt: Date.now() + TOKEN_TTL * 1000 })
  log(`jeton délivré pour ${demande.account.email} (${accessToken.slice(0, 12)}…)`)

  send(res, 200, {
    access_token: accessToken,
    token_type: 'Bearer',
    expires_in: TOKEN_TTL,
    scope: 'openid email profile',
  })
}

/** 4. Identité, lue avec le jeton porteur. */
function userinfo(req, res) {
  const header = req.headers.authorization ?? ''
  const token = header.startsWith('Bearer ') ? header.slice(7) : null
  const entree = token && tokens.get(token)
  if (!entree) {
    return oidcError(res, 401, 'invalid_token', 'jeton absent, inconnu ou expiré')
  }
  if (entree.expiresAt < Date.now()) {
    tokens.delete(token)
    return oidcError(res, 401, 'invalid_token', 'jeton expiré')
  }
  log(`userinfo : ${entree.account.email}`)

  send(res, 200, {
    sub: entree.account.sub,
    email: entree.account.email,
    email_verified: true,
    preferred_username: entree.account.preferred_username,
    name: entree.account.name,
  })
}

/** Page d'accueil : où l'on voit d'un coup d'œil ce que le serveur annonce. */
function home(req, res) {
  const internal = `http://${req.headers.host ?? `127.0.0.1:${PORT}`}`
  send(
    res,
    200,
    `<!doctype html>
<meta charset="utf-8">
<title>easy3d — fournisseur d'identité fictif</title>
<style>
  body { font: 15px/1.6 system-ui, sans-serif; margin: 2rem auto; max-width: 44rem; padding: 0 1rem; }
  code { background: #f2f2f2; padding: .1rem .3rem; border-radius: 4px; }
</style>
<h1>Fournisseur d'identité fictif</h1>
<p>Serveur OIDC de développement : aucune vérification, aucun mot de passe.</p>
<ul>
  <li>découverte : <code>${internal}/.well-known/openid-configuration</code></li>
  <li>autorisation (navigateur) : <code>${PUBLIC_URL}/authorize</code></li>
  <li>échange du code : <code>${internal}/token</code></li>
  <li>identité : <code>${internal}/userinfo</code></li>
</ul>
<p>Comptes simulés :</p>
<ul>
  ${ACCOUNTS.map((compte) => `<li><code>${compte.email}</code> (<code>sub</code> = <code>${compte.sub}</code>)</li>`).join('\n  ')}
</ul>
<p>Interrupteur : <code>EASY3D_OIDC_ISSUER=${internal}</code>.</p>`,
    'text/html; charset=utf-8',
  )
}

// ── Routeur ──────────────────────────────────────────────────────────────

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host ?? 'localhost'}`)
  try {
    if (req.method === 'GET' && url.pathname === '/.well-known/openid-configuration') {
      return discovery(req, res)
    }
    if (req.method === 'GET' && url.pathname === '/authorize') {
      return authorize(url, req, res)
    }
    if (req.method === 'POST' && url.pathname === '/token') {
      return await token(req, res)
    }
    if (req.method === 'GET' && url.pathname === '/userinfo') {
      return userinfo(req, res)
    }
    if (req.method === 'GET' && url.pathname === '/health') {
      return send(res, 200, { status: 'ok' })
    }
    if (req.method === 'GET' && url.pathname === '/') {
      return home(req, res)
    }
    log(`inattendu : ${req.method} ${url.pathname}`)
    oidcError(res, 404, 'not_found', `${req.method} ${url.pathname}`)
  } catch (erreur) {
    log('erreur inattendue :', erreur)
    oidcError(res, 500, 'server_error', String(erreur))
  }
})

server.listen(PORT, '0.0.0.0', () => {
  log(`fournisseur d'identité fictif à l'écoute sur http://0.0.0.0:${PORT}`)
  log(`  client_id=${CLIENT_ID || '(aucun contrôle)'}`)
  for (const compte of ACCOUNTS) log(`  compte=${compte.email} (id=${compte.id})`)
  if (ACCOUNTS.length > 1) {
    log(`  ${ACCOUNTS.length} comptes : la page d'autorisation propose de choisir`)
  }
  log(`  autorisation annoncée sur ${PUBLIC_URL}`)
})
