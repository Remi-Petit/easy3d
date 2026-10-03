/**
 * Vérification du fournisseur fictif, depuis l'intérieur du réseau Compose.
 *
 * Usage : docker compose exec fake-oidc node /app/selftest.mjs
 */
const base = (process.env.BASE ?? 'http://127.0.0.1:8788').replace(/\/+$/, '')
const clientId = process.env.CLIENT_ID ?? 'easy3d'
const clientSecret = process.env.CLIENT_SECRET ?? 'secret-idp'
const redirectUri = 'http://localhost:3100/api/auth/oidc/callback'

let ko = 0
const check = (nom, ok, detail) => {
  console.log(`${ok ? 'OK  ' : 'KO  '} ${nom}${detail ? ` → ${detail}` : ''}`)
  if (!ok) ko++
}

const disc = await (await fetch(`${base}/.well-known/openid-configuration`)).json()
check('découverte', !!disc.authorization_endpoint && !!disc.token_endpoint && !!disc.userinfo_endpoint)
check('authorization_endpoint publique', disc.authorization_endpoint.startsWith('http://localhost:8788'), disc.authorization_endpoint)
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

console.log(ko === 0 ? '\nTout est vert.' : `\n${ko} vérification(s) en échec.`)
process.exit(ko === 0 ? 0 : 1)
