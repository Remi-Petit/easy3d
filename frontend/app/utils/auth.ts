/**
 * Comptes utilisateurs : ce qui se vérifie sans serveur.
 *
 * Le backend ne renvoie pas de phrases mais des **codes** (`invalid_credentials`,
 * `rate_limited`…) : c'est ici qu'ils deviennent des clés de traduction, pour que
 * le message suive la langue de l'interface (i18n ×4) et non celle du serveur.
 */

/** Chemin de la page de connexion. Une seule occurrence : ici. */
export const LOGIN_PATH = '/login'

/** Compte connecté, tel que `/api/auth/me` le décrit. */
export interface AuthUser {
  uuid: string
  username: string
  email: string
  /** Noms des rôles portés par le compte (facultatif dans un JSON plus ancien). */
  roles?: string[]
  /** Droits effectifs du compte, superutilisateur compris. */
  permissions?: string[]
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
}

/**
 * Adresse de départ du flux SSO.
 *
 * `redirect` dit où revenir **après** la connexion : c'est un chemin interne,
 * et le backend le refuse s'il n'en est pas un.
 */
export function oidcStartUrl(redirect?: string | null): string {
  const cible = redirect ? `?redirect=${encodeURIComponent(redirect)}` : ''
  return `/api/auth/oidc/start${cible}`
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
