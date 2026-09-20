/**
 * Journal d'audit : ce qui se lit sans serveur.
 *
 * Le backend écrit des **codes** (`login_ok`, `token_created`…) et une précision
 * courte (`detail`). C'est ici qu'on décide comment les présenter — la page, elle,
 * traduit : c'est la règle du projet (les composables et les utilitaires ne
 * connaissent pas la langue de l'utilisateur).
 */

/** Types d'événement écrits par le backend (`auth::journal`). */
export const EVENT_KINDS = [
  'login_ok',
  'login_failed',
  'login_blocked',
  'logout',
  'password_changed',
  'sso_login',
  'sso_refused',
  'token_created',
  'token_revoked',
  'account_created',
  'account_updated',
  'account_deleted',
  'role_created',
  'role_updated',
  'role_deleted',
] as const

/**
 * Précisions qui ont un libellé traduit.
 *
 * Ce sont des codes **stables** : ce que le backend met dans `detail` et que
 * l'interface sait dire. Tout le reste est affiché tel quel (un UUID, par
 * exemple) — mieux vaut une valeur brute qu'une traduction inventée.
 */
export const DETAIL_CODES = [
  'password',
  'invalid_credentials',
  'rate_limited',
  'auto',
  'manual',
  'approval',
  'oidc_no_email',
  'oidc_not_provisioned',
  'oidc_conflict',
  'pending_approval',
  'account_disabled',
] as const

/** Champs d'un compte qu'une modification peut concerner. */
export const CHANGE_FIELDS = ['identity', 'password', 'disabled', 'enabled', 'roles', 'permissions'] as const

/** Un événement, tel que `/api/journal` le renvoie. */
export interface JournalEvent {
  at: number
  kind: string
  actor: string | null
  subject: string
  detail: string
  ip: string
  user_agent: string
}

/** Ce qu'il y a à dire d'un `detail`, selon le type d'événement. */
export type DetailShape =
  | { kind: 'none' }
  | { kind: 'code'; code: string }
  | { kind: 'days'; days: number }
  | { kind: 'changes'; fields: string[] }
  | { kind: 'raw'; value: string }

/** Clé i18n d'un type d'événement. */
export function eventKey(kind: string): string {
  return `journal.kind.${kind}`
}

/**
 * Classe la précision d'un événement, pour que la page sache quoi en dire.
 *
 * Trois cas particuliers, parce que le sens dépend du type :
 *
 * - `token_created` : le `detail` est une **durée en jours** (`0` = sans
 *   expiration) ;
 * - `account_updated` : une liste de champs modifiés, séparés par des virgules ;
 * - le reste : un code connu (traduit) ou une valeur brute (un UUID).
 */
export function describeDetail(kind: string, detail: string): DetailShape {
  const value = detail.trim()
  if (!value) return { kind: 'none' }

  if (kind === 'token_created' && /^\d+$/.test(value)) {
    return { kind: 'days', days: Number(value) }
  }
  if (kind === 'account_updated') {
    return {
      kind: 'changes',
      fields: value
        .split(',')
        .map((field) => field.trim())
        .filter(Boolean),
    }
  }
  if ((DETAIL_CODES as readonly string[]).includes(value)) {
    return { kind: 'code', code: value }
  }
  return { kind: 'raw', value }
}
