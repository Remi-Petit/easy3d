/**
 * Vérification du fournisseur fictif, exécutable **seul**.
 *
 *   node docker/fake-oidc/selftest.mjs
 *
 * Rien n'a besoin d'être démarré avant : si personne ne répond, le script lance
 * `server.mjs` le temps du test, puis l'arrête. C'est ce que la CI appelle, et
 * c'est aussi la façon la plus courte de vérifier une modification du
 * fournisseur.
 *
 * Contre une instance **déjà en place** (celle du compose, par exemple), les
 * réglages se passent en variables :
 *
 *   BASE           adresse à interroger            (défaut http://127.0.0.1:8788)
 *   PUBLIC_URL     adresse annoncée au navigateur  (défaut http://localhost:<port>)
 *   CLIENT_ID      client attendu                  (défaut easy3d)
 *   CLIENT_SECRET  secret attendu                  (défaut secret-idp)
 *   REDIRECT_URI   adresse de retour simulée       (défaut celle du compose de dev)
 */
import { spawn } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))

const base = (process.env.BASE ?? 'http://127.0.0.1:8788').replace(/\/+$/, '')
const port = Number(new URL(base).port || 8788)
const publicUrl = (process.env.PUBLIC_URL ?? `http://localhost:${port}`).replace(/\/+$/, '')
const clientId = process.env.CLIENT_ID ?? 'easy3d'
const clientSecret = process.env.CLIENT_SECRET ?? 'secret-idp'
const redirectUri = process.env.REDIRECT_URI ?? 'http://localhost:3100/api/auth/oidc/callback'

let ko = 0

/** Attend que `base` réponde, ou rend la main au bout du délai. */
async function joignable(delai) {
  const fin = Date.now() + delai
  while (Date.now() < fin) {
    try {
      const res = await fetch(`${base}/health`)
      if (res.ok) return true
    } catch {
      // Pas encore prêt : on réessaie.
    }
    await new Promise((suite) => setTimeout(suite, 100))
  }
  return false
}

/**
 * Démarre le fournisseur si personne n'écoute, et rend de quoi l'arrêter.
 *
 * Le serveur reçoit **exactement** les réglages que le test vérifie : sans cela
 * il annoncerait ses propres valeurs, et le test serait tautologique.
 */
async function demarrerSiBesoin() {
  if (await joignable(500)) return null

  const child = spawn(process.execPath, [join(here, 'server.mjs')], {
    env: { ...process.env, PORT: String(port), PUBLIC_URL: publicUrl, CLIENT_ID: clientId, CLIENT_SECRET: clientSecret },
    stdio: 'inherit',
  })
  if (!(await joignable(10_000))) {
    child.kill()
    throw new Error(`le fournisseur n'a pas répondu sur ${base}`)
  }
  return child
}

/** Instance lancée par ce script, s'il a fallu en démarrer une. */
const serveur = await demarrerSiBesoin()
const check = (nom, ok, detail) => {
  console.log(`${ok ? 'OK  ' : 'KO  '} ${nom}${detail ? ` → ${detail}` : ''}`)
  if (!ok) ko++
}

const sante = await fetch(`${base}/health`)
check('santé', sante.ok, String(sante.status))

const disc = await (await fetch(`${base}/.well-known/openid-configuration`)).json()
check('découverte', !!disc.authorization_endpoint && !!disc.token_endpoint && !!disc.userinfo_endpoint)
check('authorization_endpoint publique', disc.authorization_endpoint.startsWith(publicUrl), disc.authorization_endpoint)
check('token_endpoint joignable', disc.token_endpoint === `${base}/token`, disc.token_endpoint)

const authUrl = `${disc.authorization_endpoint}?response_type=code&client_id=${clientId}` +
  `&redirect_uri=${encodeURIComponent(redirectUri)}&scope=openid%20email%20profile&state=st-1`
const redir = await fetch(authUrl, { redirect: 'manual' })
check('autorisation = 302', redir.status === 302, String(redir.status))
const location = redir.headers.get('location') ?? ''
const params = new URL(location).searchParams
check('state conservé', params.get('state') === 'st-1', params.get('state') ?? '')
check('code délivré', !!params.get('code'))

const tokenBody = new URLSearchParams({
  grant_type: 'authorization_code',
  code: params.get('code'),
  redirect_uri: redirectUri,
  client_id: clientId,
  client_secret: clientSecret,
})
const tokenRes = await fetch(disc.token_endpoint, { method: 'POST', body: tokenBody })
const token = await tokenRes.json()
check('jeton délivré', tokenRes.status === 200 && !!token.access_token, token.token_type)

const userRes = await fetch(disc.userinfo_endpoint, {
  headers: { authorization: `Bearer ${token.access_token}` },
})
const user = await userRes.json()
check('userinfo', userRes.status === 200 && !!user.sub && !!user.email, `${user.sub} / ${user.email}`)

const rejoue = await fetch(disc.token_endpoint, { method: 'POST', body: tokenBody })
check('code non réutilisable', rejoue.status === 400, String(rejoue.status))

const faux = await fetch(disc.token_endpoint, {
  method: 'POST',
  body: new URLSearchParams({ ...Object.fromEntries(tokenBody), client_secret: 'faux' }),
})
check('secret refusé', faux.status === 401, String(faux.status))

const sansJeton = await fetch(disc.userinfo_endpoint)
check('userinfo sans jeton refusé', sansJeton.status === 401, String(sansJeton.status))

// L'instance lancée pour le test n'est pas laissée derrière : le port doit être
// libre à la sortie, sinon l'exécution suivante viserait l'ancienne — avec ses
// réglages, et sans le dire.
serveur?.kill()

console.log(ko === 0 ? '\nTout est vert.' : `\n${ko} vérification(s) en échec.`)
process.exit(ko === 0 ? 0 : 1)
