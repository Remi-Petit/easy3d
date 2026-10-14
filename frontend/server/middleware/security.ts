import { defineEventHandler, getRequestProtocol, setResponseHeader } from 'h3'

/**
 * En-têtes de sécurité sur **toutes** les réponses de l'interface.
 *
 * Le backend en pose de son côté (`api::security_headers`, pour le JSON et les
 * fichiers) : ici, c'est le HTML et les ressources que le navigateur va
 * réellement interpréter, donc ce qui compte pour le clickjacking et le
 * chargement de ressources tierces.
 *
 * - `X-Frame-Options: DENY` **et** `frame-ancestors 'none'` : l'interface ne
 *   s'affiche dans aucun cadre. Les deux sont posés, car le premier est ignoré
 *   par les navigateurs récents dès qu'une CSP est présente, et le second par
 *   les anciens ;
 * - `X-Content-Type-Options: nosniff` : un contenu servi avec un type MIME
 *   volontairement neutre ne doit pas être deviné (et exécuté) ;
 * - `Referrer-Policy: strict-origin-when-cross-origin` : l'URL d'un modèle
 *   (`/files/DemaAuto/boitier.stl`) n'a rien à apprendre à un site tiers ;
 * - `Permissions-Policy` : ni caméra, ni micro, ni géolocalisation — l'appli
 *   n'en fait rien, autant le dire au navigateur plutôt que de laisser un site
 *   encadré les demander ;
 * - `Strict-Transport-Security`, **uniquement si la requête est arrivée en
 *   HTTPS** (le protocole est celui que voit Nitro, en-tête `x-forwarded-proto`
 *   du proxy amont compris) : annoncer HSTS sur un déploiement local en `http`
 *   obligerait le navigateur à ne plus jamais revenir en clair sur cette
 *   adresse ;
 * - `Content-Security-Policy` : **restreinte à ce qui est structurel**, pas de
 *   `default-src` complet. L'interface est une application Nuxt (scripts
 *   inlinés de préchargement, styles injectés par Tailwind, `blob:` pour les
 *   *workers* du moteur 3D) : une politique stricte écrite ici casserait
 *   l'application sans qu'aucun test unitaire ne le voie. Ce qui est posé
 *   (`frame-ancestors`, `object-src`, `base-uri`, `form-action`) ferme les
 *   vecteurs réels — encadrement, greffon, détournement d'URL de base,
 *   exfiltration par formulaire — sans toucher à ce que l'appli charge.
 *
 * Un contrôle CSP complet (`default-src 'self'`, nonces) se décide avec une
 * vérification en conteneur, comme tout le reste de l'interface.
 */
const CSP = [
  "frame-ancestors 'none'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join('; ')

export default defineEventHandler((event) => {
  setResponseHeader(event, 'x-frame-options', 'DENY')
  setResponseHeader(event, 'x-content-type-options', 'nosniff')
  setResponseHeader(event, 'referrer-policy', 'strict-origin-when-cross-origin')
  setResponseHeader(event, 'content-security-policy', CSP)
  setResponseHeader(event, 'permissions-policy', 'camera=(), microphone=(), geolocation=()')

  // `getRequestProtocol` tient compte de `x-forwarded-proto` (proxy amont) :
  // c'est le seul moyen de savoir qu'un visiteur est en HTTPS quand le
  // certificat est terminé devant Nitro.
  if (getRequestProtocol(event) === 'https') {
    setResponseHeader(event, 'strict-transport-security', 'max-age=31536000; includeSubDomains')
  }
})
