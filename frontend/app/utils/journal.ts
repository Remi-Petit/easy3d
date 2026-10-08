/**
 * Journal d'audit : ce qui se lit sans serveur.
 *
 * Le backend écrit des **codes** (`login_ok`, `token_created`…) et une précision
 * courte (`detail`). C'est ici qu'on décide comment les présenter — la page, elle,
 * traduit : c'est la règle du projet (les composables et les utilitaires ne
 * connaissent pas la langue de l'utilisateur).
 */

// Import explicite : l'auto-import Nuxt n'existe pas sous Vitest.
import type { SortMode } from '~/utils/filter'

/** Types d'événement écrits par le backend (`auth::journal`). */
export const EVENT_KINDS = [
  'login_ok',
  'login_failed',
  'login_blocked',
  'logout',
  'password_changed',
  'sso_login',
  'sso_refused',
  'sso_detached',
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
  'password_sso',
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
export const CHANGE_FIELDS = ['identity', 'password', 'password_revoked', 'disabled', 'enabled', 'roles', 'permissions'] as const

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

/**
 * Familles d'événements : les puces de filtre de la page d'audit.
 *
 * Les quinze types regroupés en cinq familles lisibles — c'est ainsi qu'on
 * cherche : « les refus », « les jetons ». Une puce par type ferait une rangée
 * illisible, et personne ne filtre sur `login_blocked` tout seul.
 */
export const EVENT_FAMILIES = {
  sessions: ['login_ok', 'sso_login', 'logout'],
  refusals: ['login_failed', 'login_blocked', 'sso_refused'],
  tokens: ['token_created', 'token_revoked'],
  accounts: [
    'account_created',
    'account_updated',
    'account_deleted',
    'password_changed',
    'sso_detached',
  ],
  roles: ['role_created', 'role_updated', 'role_deleted'],
} as const satisfies Record<string, readonly string[]>

export type EventFamily = keyof typeof EVENT_FAMILIES

/** Les familles, dans l'ordre d'affichage. */
export const EVENT_FAMILY_LIST = Object.keys(EVENT_FAMILIES) as EventFamily[]

/**
 * Icône de chaque famille (lucide, comme le reste de l'interface).
 *
 * Elle sert au tableau : la colonne « événement » montre l'icône **et** la
 * couleur de la famille, pour qu'un refus se distingue d'une connexion sans
 * lire la ligne. Même parti pris que `SORT_ICONS` (voir `utils/filter.ts`) : les
 * icônes sont des données, elles se testent.
 */
export const EVENT_ICONS: Record<EventFamily, string> = {
  sessions: 'i-lucide-log-in',
  refusals: 'i-lucide-shield-alert',
  tokens: 'i-lucide-key-round',
  accounts: 'i-lucide-user-round',
  roles: 'i-lucide-shield-check',
}

/**
 * Famille d'un type d'événement (`null` si le type est inconnu).
 *
 * Un type venu d'un backend plus récent n'a ni icône ni couleur : la page
 * l'affiche alors tel quel, sans inventer de famille.
 */
export function eventFamily(kind: string): EventFamily | null {
  return (
    EVENT_FAMILY_LIST.find((famille) =>
      (EVENT_FAMILIES[famille] as readonly string[]).includes(kind),
    ) ?? null
  )
}

/** Filtres de la page d'audit : période (secondes epoch, `0` = pas de borne). */
export interface JournalFilters {
  text?: string
  families?: EventFamily[]
  since?: number
}

/**
 * Les événements qui passent les filtres.
 *
 * `describe` fournit le texte **affiché** d'un événement (type traduit, détail
 * traduit) : la recherche couvre donc exactement ce que l'utilisateur a sous les
 * yeux, ce qu'il aurait tapé dans le champ. L'utilitaire, lui, ne connaît pas la
 * langue — c'est la page qui la lui donne (règle du projet).
 */
export function filterEvents(
  events: JournalEvent[],
  filters: JournalFilters,
  describe: (event: JournalEvent) => string,
): JournalEvent[] {
  const texte = (filters.text ?? '').trim().toLowerCase()
  const borne = filters.since ?? 0
  const familles = filters.families ?? []
  const kinds = familles.flatMap((famille) => [...EVENT_FAMILIES[famille]])

  return events.filter((event) => {
    if (event.at < borne) return false
    if (familles.length && !kinds.includes(event.kind)) return false
    if (!texte) return true
    return describe(event).toLowerCase().includes(texte)
  })
}

/** Nombre d'événements par famille depuis `since` (`0` = tout). */
export function familyCounts(events: JournalEvent[], since = 0): Record<EventFamily, number> {
  const compte = {} as Record<EventFamily, number>
  for (const famille of EVENT_FAMILY_LIST) {
    const kinds: readonly string[] = EVENT_FAMILIES[famille]
    compte[famille] = events.filter((event) => event.at >= since && kinds.includes(event.kind)).length
  }
  return compte
}

/**
 * Trie les événements par date.
 *
 * `none` rend la liste telle qu'elle arrive du serveur : le journal est déjà rangé
 * du plus récent au plus ancien, c'est son ordre naturel. Même vocabulaire et même
 * cycle que le tri du catalogue (`utils/filter.ts`) — le bouton se comporte donc
 * pareil des deux côtés. La liste d'entrée n'est pas modifiée.
 */
export function sortEvents(events: JournalEvent[], mode: SortMode): JournalEvent[] {
  if (mode === 'none') return events
  const sens = mode === 'date-asc' ? 1 : -1
  return [...events].sort((a, b) => (a.at - b.at) * sens)
}
