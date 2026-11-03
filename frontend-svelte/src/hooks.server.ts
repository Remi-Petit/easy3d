// Middlewares du serveur : langue et en-têtes de sécurité.
//
// Reprise de `frontend/server/middleware/security.ts` (mêmes en-têtes, même CSP
// structurelle) et de la détection de langue par cookie.
import type { Handle } from '@sveltejs/kit'
import { langueParDefaut, langues } from '$lib/i18n'

const CSP = [
  "default-src 'self'",
  // Nuxt injecte ses scripts en ligne ; SvelteKit aussi (hydratation), d'où
  // `'unsafe-inline'` — même compromis que l'application d'origine.
  "script-src 'self' 'unsafe-inline'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self' data:",
  "connect-src 'self' ws: wss:",
  "media-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join('; ')

export const handle: Handle = async ({ event, resolve }) => {
  // Langue : cookie `easy3d_lang`, comme l'app Nuxt (aucun préfixe d'URL).
  const cookie = event.cookies.get('easy3d_lang')
  const connues = new Set(langues.map((l) => l.code))
  event.locals.langue = cookie && connues.has(cookie) ? cookie : langueParDefaut

  const reponse = await resolve(event, {
    transformPageChunk: ({ html }) => html.replace('%langue%', event.locals.langue),
  })

  reponse.headers.set('content-security-policy', CSP)
  reponse.headers.set('x-content-type-options', 'nosniff')
  reponse.headers.set('x-frame-options', 'DENY')
  reponse.headers.set('referrer-policy', 'no-referrer')
  reponse.headers.set('permissions-policy', 'geolocation=(), microphone=(), camera=()')
  return reponse
}
