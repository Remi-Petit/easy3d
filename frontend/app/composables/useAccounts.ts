import type { PermissionInfo } from '~/utils/permissions'

/**
 * Écran « comptes et rôles » : lecture et écriture.
 *
 * Comme `useAuth`, ce composable ne traduit rien : il expose le **code d'erreur**
 * du backend, et c'est la page (dans son `setup`) qui choisit les mots. Un
 * composable qui appelle `useI18n()` ne peut pas être utilisé depuis une garde de
 * route, et cette contrainte vaut pour tout ce qui vit à côté de l'état partagé.
 */

/** Désignation minimale d'un rôle. */
export interface RoleRef {
  uuid: string
  name: string
}

/** Un rôle, tel que l'écran des rôles le montre. */
export interface RoleView extends RoleRef {
  description: string
  /** Rôle livré (`admin`, `lecteur`) : figé, mais clonable. */
  builtin: boolean
  permissions: string[]
  members: number
}

/** Un compte. */
export interface AccountView {
  uuid: string
  username: string
  email: string
  roles: RoleRef[]
  /** Droits posés directement sur le compte. */
  direct: string[]
  /** Droits effectifs (rôles ∪ directs). */
  effective: string[]
  disabled: boolean
  /** Rattaché au fournisseur d'identité (voir `oidc_subject` en base). */
  oidc: boolean
  /** `true` si le compte a un mot de passe local — sans jamais sortir l'empreinte. */
  has_password: boolean
  created_at: number
}

interface AccountsResponse {
  users: AccountView[]
  roles: RoleRef[]
  default_role: string | null
}

interface RolesResponse {
  roles: RoleView[]
  default_role: string | null
}

/** Champs modifiables d'un compte (tous facultatifs : absent = inchangé). */
export interface AccountPatch {
  username?: string
  email?: string
  password?: string
  roles?: string[]
  permissions?: string[]
  disabled?: boolean
}

/** Champs d'un rôle. */
export interface RolePatch {
  name?: string
  description?: string
  permissions?: string[]
}

/**
 * Un compte **complet**, tel que le formulaire le rend : tous les champs sont
 * renseignés, sauf le mot de passe — absent veut dire « ne le change pas », et
 * un mot de passe vide est accepté à la création (le compte ne se connectera
 * alors que par le SSO).
 */
export type AccountDraft = Required<Omit<AccountPatch, 'password'>> &
  Pick<AccountPatch, 'password'>

/** Nouveau compte, tel que le formulaire le compose. */
export interface NewAccount {
  username: string
  email: string
  password: string
  roles: string[]
  permissions: string[]
}

/**
 * Code d'erreur renvoyé par le backend, ou une valeur générique.
 *
 * Le backend répond en texte brut (`last_admin`, `role_frozen`…) : c'est un
 * **code**, traduit par la page. Un refus de droit, lui, ne vient pas du handler
 * mais de la couche de contrôle : il arrive en 403 sans corps.
 */
export function errorCodeOf(error: unknown): string {
  const data = (error as { data?: unknown })?.data
  if (typeof data === 'string' && data.trim()) return data.trim()

  const status =
    (error as { statusCode?: number })?.statusCode ??
    (error as { response?: { status?: number } })?.response?.status
  if (status === 401) return 'unauthenticated'
  if (status === 403) return 'forbidden'
  if (status === 404) return 'not_found'
  return status ? `http_${status}` : 'unknown'
}

export function useAccounts() {
  const users = ref<AccountView[]>([])
  const roles = ref<RoleView[]>([])
  /** Rôles disponibles pour cocher les cases d'un compte (uuid + nom). */
  const roleRefs = ref<RoleRef[]>([])
  /** Catalogue des droits, servi par le backend. */
  const permissions = ref<PermissionInfo[]>([])
  const defaultRole = ref<string | null>(null)
  const loading = ref(false)
  const busy = ref(false)
  /** Code d'erreur de la dernière action, à traduire par la page. */
  const errorCode = ref<string | null>(null)

  /** Recharge comptes, rôles et catalogue des droits. */
  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const [comptes, rolesReponse, catalogue] = await Promise.all([
        $fetch<AccountsResponse>('/api/users'),
        $fetch<RolesResponse>('/api/roles'),
        $fetch<PermissionInfo[]>('/api/permissions'),
      ])
      users.value = comptes.users
      roleRefs.value = comptes.roles
      roles.value = rolesReponse.roles
      defaultRole.value = rolesReponse.default_role
      permissions.value = catalogue
      errorCode.value = null
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
    } finally {
      loading.value = false
    }
  }

  /**
   * Exécute une écriture puis recharge la liste.
   *
   * Le rechargement n'est pas un luxe : le serveur peut avoir appliqué autre
   * chose que ce qui a été demandé (rôle par défaut, contraintes d'unicité), et
   * c'est sa réponse qui fait foi — pas l'état optimiste du formulaire.
   */
  async function write(action: () => Promise<unknown>): Promise<boolean> {
    busy.value = true
    errorCode.value = null
    try {
      await action()
      await refresh()
      return true
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
      return false
    } finally {
      busy.value = false
    }
  }

  return {
    users,
    roles,
    roleRefs,
    permissions,
    defaultRole,
    loading,
    busy,
    errorCode,
    refresh,
    createAccount: (account: NewAccount) =>
      write(() => $fetch('/api/users', { method: 'POST', body: account })),
    updateAccount: (uuid: string, patch: AccountPatch) =>
      write(() => $fetch(`/api/users/${encodeURIComponent(uuid)}`, { method: 'PUT', body: patch })),
    /**
     * Retire le mot de passe local : le compte n'entre plus que par le
     * fournisseur d'identité (chaîne **vide**, distincte de « absent »).
     */
    revokePassword: (uuid: string) =>
      write(() =>
        $fetch(`/api/users/${encodeURIComponent(uuid)}`, {
          method: 'PUT',
          body: { password: '' },
        }),
      ),
    /**
     * Détache le compte du fournisseur d'identité.
     *
     * Un compte rattaché n'a pas de mot de passe local : le détachement en pose
     * un **dans le même geste**, sinon le compte ne serait plus joignable du tout.
     */
    detachOidc: (uuid: string, password?: string) =>
      write(() =>
        $fetch(`/api/users/${encodeURIComponent(uuid)}/oidc`, {
          method: 'DELETE',
          body: password ? { password } : {},
        }),
      ),
    deleteAccount: (uuid: string) =>
      write(() => $fetch(`/api/users/${encodeURIComponent(uuid)}`, { method: 'DELETE' })),
    createRole: (role: RolePatch & { name: string; from?: string }) =>
      write(() => $fetch('/api/roles', { method: 'POST', body: role })),
    updateRole: (uuid: string, patch: RolePatch) =>
      write(() => $fetch(`/api/roles/${encodeURIComponent(uuid)}`, { method: 'PUT', body: patch })),
    deleteRole: (uuid: string) =>
      write(() => $fetch(`/api/roles/${encodeURIComponent(uuid)}`, { method: 'DELETE' })),
    setDefaultRole: (uuid: string | null) =>
      write(() => $fetch('/api/roles/default', { method: 'PUT', body: { uuid } })),
  }
}
