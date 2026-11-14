import { createError, defineEventHandler } from 'h3'
import { useRuntimeConfig } from '#imports'
import { devLoginEnabled } from '../../utils/devRealm'
import { authProxy } from '../../utils/authProxy'

/**
 * Connexion **d'un clic** de la page `/dev/login`.
 *
 * La page envoie le compte choisi et le mot de passe que `realm.json` publie de
 * toute façon ; le backend le présente au fournisseur
 * (`POST /auth/oidc/dev-login`) et ouvre la session SSO. Corps et `Set-Cookie`
 * passent tels quels (`authProxy`) : recopier la requête ici obligerait à tenir
 * deux validations à jour (le `redirect` est assaini par le backend, `return_to`),
 * et une réponse intermédiaire perdrait le cookie de session.
 *
 * Deux verrous indépendants, tous les deux nécessaires :
 * - ici, `NUXT_PUBLIC_DEV_LOGIN` (la page elle-même) ;
 * - côté backend, `EASY3D_OIDC_DEV_LOGIN` — sans lui, le point d'entrée répond
 *   `403 dev_login_disabled`. Le second ne se contourne pas depuis le
 *   navigateur, et le premier ne sert à rien sans la page qui l'appelle.
 */
export default defineEventHandler((event) => {
  if (!devLoginEnabled(useRuntimeConfig(event))) {
    throw createError({ statusCode: 404, statusMessage: 'Not Found' })
  }
  return authProxy(event, '/auth/oidc/dev-login')
})
