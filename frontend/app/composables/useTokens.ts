import { errorCodeOf } from '~/composables/useAccounts'

/**
 * Jetons d'API du compte connecté (`/compte`).
 *
 * Un agent — Claude Code, un script, une CI — n'a pas de navigateur, donc pas de
 * cookie : il présente un jeton en `Authorization: Bearer`. Ces jetons
 * **héritent des droits du compte** et se révoquent d'un clic.
 *
 * Comme les autres composables, celui-ci ne traduit rien : il expose le **code
 * d'erreur** du backend, que la page traduit dans son `setup`.
 */

/** Un jeton, tel que l'interface le montre : jamais sa valeur. */
export interface TokenView {
  uuid: string
  name: string
  created_at: number
  /** `null` tant qu'il n'a jamais servi — le signe qu'on peut le révoquer. */
  last_used_at: number | null
}

/** Réponse de la création : le jeton en clair, **une seule fois**. */
export interface CreatedToken {
  token: string
  uuid: string
  name: string
  created_at: number
}

export function useTokens() {
  const tokens = ref<TokenView[]>([])
  const loading = ref(false)
  const busy = ref(false)
  /** Code d'erreur de la dernière action (traduit par la page). */
  const errorCode = ref<string | null>(null)
  /**
   * Le jeton qui vient d'être créé.
   *
   * Il n'apparaît qu'ici : le backend ne stocke que son empreinte, donc il ne
   * peut plus être réaffiché après. La page le montre tant que l'utilisateur ne
   * l'a pas emporté.
   */
  const created = ref<CreatedToken | null>(null)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      tokens.value = await $fetch<TokenView[]>('/api/tokens')
      errorCode.value = null
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
    } finally {
      loading.value = false
    }
  }

  async function create(name: string): Promise<boolean> {
    busy.value = true
    errorCode.value = null
    try {
      created.value = await $fetch<CreatedToken>('/api/tokens', {
        method: 'POST',
        body: { name },
      })
      await refresh()
      return true
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
      return false
    } finally {
      busy.value = false
    }
  }

  async function revoke(uuid: string): Promise<boolean> {
    busy.value = true
    errorCode.value = null
    try {
      await $fetch(`/api/tokens/${encodeURIComponent(uuid)}`, { method: 'DELETE' })
      // Le jeton affiché ne doit pas survivre à sa révocation : il ne vaut plus
      // rien, et le laisser à l'écran ferait copier une chaîne morte.
      if (created.value?.uuid === uuid) created.value = null
      await refresh()
      return true
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
      return false
    } finally {
      busy.value = false
    }
  }

  /** Efface l'affichage du jeton créé (l'utilisateur l'a emporté). */
  function forget() {
    created.value = null
  }

  return { tokens, loading, busy, errorCode, created, refresh, create, revoke, forget }
}
