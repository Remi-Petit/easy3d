/**
 * Comptes utilisateurs : ce qui se vérifie sans serveur.
 *
 * Le backend ne renvoie pas de phrases mais des **codes** (`invalid_credentials`,
 * `rate_limited`…) : c'est ici qu'ils deviennent des clés de traduction, pour que
 * le message suive la langue de l'interface (i18n ×4) et non celle du serveur.
 */

/** Chemin de la page de connexion. Une seule occurrence : ici. */
export const LOGIN_PATH = '/login'

/**
 * Page **de développement** qui propose les comptes du fournisseur.
 *
 * Elle n'existe que quand `NUXT_PUBLIC_DEV_LOGIN=1` (voir
 * `server/utils/devRealm.ts`) : elle sert à essayer plusieurs comptes — et donc
 * plusieurs jeux de droits — sans retaper un mot de passe à chaque fois.
 */
export const DEV_LOGIN_PATH = '/dev/login'

/** Libellé du bouton de développement (le fournisseur, lui, s'appelle autrement). */
export const DEV_SSO_LABEL = 'keycloak-dev'

/** Compte connecté, tel que `/api/auth/me` le décrit. */
export interface AuthUser {
  uuid: string
  username: string
  email: string
  /** Noms des rôles portés par le compte (facultatif dans un JSON plus ancien). */
  roles?: string[]
  /** Droits effectifs du compte, superutilisateur compris. */
  permissions?: string[]
  /**
   * `true` quand le compte est rattaché au fournisseur d'identité.
   *
   * La page « Mon compte » grise alors le changement de mot de passe : ce
   * compte n'en a pas ici, il se gère chez le fournisseur (le serveur refuse
   * de son côté, code `password_sso`).
   */
  oidc?: boolean
}

/** Réponse de `GET /auth/me` : toujours 200, même sans session. */
export interface AuthStatus {
  /** `false` quand `EASY3D_AUTH` n'est pas activé sur le serveur. */
  enabled: boolean
  user: AuthUser | null
  /** Présent quand une connexion par fournisseur d'identité est possible. */
  oidc?: OidcProvider | null
}

/**
 * Fournisseur d'identité (SSO) annoncé par le backend sur la page de connexion.
 *
 * Seul son **nom** est transmis : l'interface n'a pas besoin de l'émetteur ni du
 * secret client, qu'elle ne voit jamais (c'est le backend qui parle au
 * fournisseur). Le reste des réglages vit dans `~/utils/oidc`.
 */
export interface OidcProvider {
  /** Hôte du fournisseur, tel qu'affiché sur le bouton. */
  label: string
  /**
   * Fournisseurs **supplémentaires** (déclarés par `EASY3D_OIDC_PROVIDERS`).
   *
   * Chacun ajoute un bouton de connexion : le principal reste le premier, et ces
   * fournisseurs-là s'ajoutent (développement : Keycloak **et** le fournisseur
   * fictif en même temps).
   */
  providers?: OidcExtraProvider[]
}

/** Fournisseur supplémentaire : de quoi dessiner son bouton et partir chez lui. */
export interface OidcExtraProvider {
  /** Identifiant d'URL (`fake`, `keycloak`…), transmis au départ du flux. */
  id: string
  /** Nom affiché sur le bouton. */
  label: string
}

/**
 * Adresse de départ du flux SSO.
 *
 * `redirect` dit où revenir **après** la connexion : c'est un chemin interne,
 * et le backend le refuse s'il n'en est pas un.
 *
 * `loginHint` (facultatif) demande au fournisseur de **pré-remplir** le compte.
 * C'est une indication, pas une garantie : seul le fournisseur décide. La page
 * de développement s'en sert, et le backend la nettoie avant de la transmettre.
 */
export function oidcStartUrl(
  redirect?: string | null,
  loginHint?: string | null,
  provider?: string | null,
): string {
  // `encodeURIComponent` et non `URLSearchParams` : ce dernier écrit une espace
  // en `+`, ce qui changerait l'adresse produite jusqu'ici (`%20`). Rien ne
  // l'exige, et une adresse qui ne bouge pas évite de se demander si un relais
  // la décode pareil.
  const parametres: string[] = []
  if (redirect) parametres.push(`redirect=${encodeURIComponent(redirect)}`)
  if (loginHint) parametres.push(`login_hint=${encodeURIComponent(loginHint)}`)
  // Fournisseur supplémentaire ; absent = le principal.
  if (provider) parametres.push(`provider=${encodeURIComponent(provider)}`)
  return `/api/auth/oidc/start${parametres.length > 0 ? `?${parametres.join('&')}` : ''}`
}

/** Adresse de la page de choix des comptes de développement. */
export function devLoginUrl(redirect?: string | null): string {
  return redirect ? `${DEV_LOGIN_PATH}?redirect=${encodeURIComponent(redirect)}` : DEV_LOGIN_PATH
}

/**
 * Codes d'erreur de `POST /auth/login` (voir `auth::login` côté backend).
 *
 * Ils sont **stables** : c'est le contrat entre les deux côtés, comme les codes
 * de `formats` ou des fournisseurs d'IA.
 */
export const AUTH_ERROR_CODES = [
  'auth_disabled',
  'invalid_credentials',
  'rate_limited',
  'password_too_short',
  'password_too_long',
  // Le compte est rattaché au fournisseur d'identité : le mot de passe local ne
  // le fait pas entrer, même quand il est bon (voir `auth::login`).
  'password_sso',
  'internal_error',
  // Retours du fournisseur d'identité (`/auth/oidc/callback`), déposés dans
  // l'URL de la page de connexion : `?error=<code>`.
  'oidc_denied',
  'invalid_state',
  'oidc_no_email',
  'oidc_not_provisioned',
  'oidc_conflict',
  'pending_approval',
  'account_disabled',
] as const

export type AuthErrorCode = (typeof AUTH_ERROR_CODES)[number]

/**
 * Clé i18n d'un code d'erreur du backend.
 *
 * Un code inconnu (backend plus récent que l'interface) retombe sur un message
 * générique : afficher la clé brute `auth.errors.quelque_chose` à l'utilisateur
 * serait pire que de ne rien dire de précis.
 */
export function authErrorKey(code: unknown): string {
  const found = AUTH_ERROR_CODES.find((known) => known === code)
  return `auth.errors.${found ?? 'unknown'}`
}

/**
 * Chemin à ouvrir après une connexion réussie.
 *
 * Seul un chemin **interne** est accepté : `//exemple.fr` et les URL absolues
 * sont écartés. Sans ce filtre, `?redirect=` ferait de la page de connexion un
 * tremplin vers n'importe quel site (hameçonnage à partir d'une adresse
 * familière).
 */
export function safeRedirect(value: unknown, fallback = '/'): string {
  const path = typeof value === 'string' ? value.trim() : ''
  if (!path.startsWith('/') || path.startsWith('//')) return fallback
  return path
}
