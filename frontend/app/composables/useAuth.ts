import { LOGIN_PATH, type AuthStatus, type AuthUser } from '~/utils/auth'

/**
 * État partagé de l'authentification.
 *
 * La gestion des comptes est **facultative** : quand le backend répond
 * `enabled: false` (pas de `EASY3D_AUTH`), rien de ce qui suit n'a d'effet
 * visible — pas de page de connexion, pas de garde, pas de bloc de compte. C'est
 * ce qui rend la fonctionnalité indolore sur une installation existante.
 *
 * Trois points à connaître :
 *
 * - **Le cookie est transmis à la main au rendu serveur.** Depuis Nitro, un
 *   appel interne à `/api/auth/me` n'hérite pas des en-têtes du navigateur :
 *   sans `useRequestHeaders(['cookie'])`, le serveur se croirait anonyme et
 *   renverrait tout le monde vers la page de connexion à chaque rechargement.
 * - **L'état vit dans `useState`**, donc partagé entre le serveur et le client
 *   (et entre composants) : le premier rendu connaît déjà la session, sans
 *   clignotement.
 * - **Aucun appel à `useI18n()` ici.** Ce composable est utilisé par une garde de
 *   route, où `useI18n` refuse de s'exécuter (« Must be called at the top of a
 *   `setup` function ») : il expose donc le **code** d'erreur du backend, et
 *   c'est l'appelant — une page, dans son `setup` — qui le traduit.
 */
export function useAuth() {
  const status = useState<AuthStatus>('easy3d:auth', () => ({ enabled: false, user: null }))
  /** `true` dès que le serveur a été interrogé une fois (par requête / session). */
  const ready = useState<boolean>('easy3d:auth:ready', () => false)
  const busy = ref(false)
  /** Code d'erreur du backend (`invalid_credentials`, `rate_limited`…). */
  const errorCode = ref<string | null>(null)

  /** En-têtes d'un appel : le cookie ne suit pas tout seul côté serveur. */
  function withCookie() {
    return import.meta.server ? useRequestHeaders(['cookie']) : undefined
  }

  /**
   * Interroge le serveur sur l'état de l'authentification.
   *
   * Un backend injoignable **ne verrouille rien** : on laisse `enabled` à
   * `false` et c'est le catalogue qui affichera son propre message d'erreur.
   * « Je ne sais pas » ne doit jamais se traduire par « accès refusé ».
   */
  async function refresh(): Promise<AuthStatus> {
    try {
      status.value = await $fetch<AuthStatus>('/api/auth/me', { headers: withCookie() })
    } catch {
      status.value = { enabled: false, user: null }
    } finally {
      ready.value = true
    }
    return status.value
  }

  /**
   * Connexion par mot de passe. Renvoie `true` si la session est ouverte.
   *
   * En cas d'échec, `errorCode` porte le code renvoyé par le backend — «
   * identifiant ou mot de passe incorrect » ne dit pas lequel des deux est faux,
   * et c'est voulu.
   */
  async function login(identifiant: string, password: string): Promise<boolean> {
    busy.value = true
    errorCode.value = null
    try {
      const user = await $fetch<AuthUser>('/api/auth/login', {
        method: 'POST',
        body: { login: identifiant, password },
      })
      // Le reste de l'état est conservé : le fournisseur d'identité annoncé au
      // chargement de la page ne dépend pas de qui se connecte.
      status.value = { ...status.value, enabled: true, user }
      ready.value = true
      return true
    } catch (e: unknown) {
      // `proxyRequest` laisse passer le corps du backend tel quel : le code
      // arrive donc en texte brut dans `data`.
      const data = (e as { data?: unknown })?.data
      errorCode.value = typeof data === 'string' && data.trim() ? data.trim() : 'unknown'
      return false
    } finally {
      busy.value = false
    }
  }

  /**
   * Ferme la session.
   *
   * L'état est mis à jour même si l'appel échoue : côté interface, l'utilisateur
   * a demandé à sortir, et le cookie a de toute façon été effacé par le serveur
   * (ou le sera au prochain démarrage).
   */  async function logout() {
    try {
      await $fetch('/api/auth/logout', { method: 'POST' })
    } catch {
      // Sans importance : voir ci-dessus.
    }
    status.value = { ...status.value, enabled: true, user: null }
  }

  /** Déconnexion puis retour à la page de connexion. */
  async function signOut() {
    await logout()
    await navigateTo(LOGIN_PATH)
  }

  return {
    status,
    ready,
    busy,
    errorCode,
    /** `true` si le serveur applique l'authentification. */
    enabled: computed(() => status.value.enabled),
    /** Compte connecté, ou `null`. */
    user: computed(() => status.value.user),
    /**
     * Fournisseur d'identité (SSO) proposé par le serveur, ou `null`.
     *
     * C'est ce qui décide de l'affichage du bouton sur la page de connexion :
     * pas de fournisseur configuré, pas de bouton — et aucune liste de
     * fournisseurs en dur ici (le backend est seul à savoir).
     */
    oidc: computed(() => status.value.oidc ?? null),
    can,
    refresh,
    login,
    logout,
    signOut,
  }
}

/**
 * `true` si le compte connecté a ce droit.
 *
 * Quand l'authentification est **éteinte** (installation sans comptes, le cas
 * par défaut), tout est permis : l'interface ne doit rien masquer, elle est
 * exactement celle d'avant.
 */
function can(permission: string): boolean {
  const status = useState<AuthStatus>('easy3d:auth', () => ({ enabled: false, user: null }))
  if (!status.value.enabled) return true
  return (status.value.user?.permissions ?? []).includes(permission)
}
