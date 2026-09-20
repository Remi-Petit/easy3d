import { LOGIN_PATH, safeRedirect } from '~/utils/auth'

/**
 * Garde d'accès : sans session, on ne va pas plus loin que la page de connexion.
 *
 * Elle tourne **aussi au rendu serveur** — c'est ce qui évite d'afficher le
 * catalogue puis de le retirer une fois la page chargée. `useAuth().refresh()`
 * se charge d'y transmettre le cookie du navigateur.
 *
 * Quand l'authentification est éteinte (`enabled: false`), la garde ne fait
 * rien : l'application se comporte exactement comme avant l'existence des
 * comptes, et `EASY3D_AUTH` reste le seul interrupteur.
 */
export default defineNuxtRouteMiddleware(async (to) => {
  const { status, ready, refresh } = useAuth()

  // Un seul appel par requête (côté serveur) ou par session (côté client).
  if (!ready.value) await refresh()

  if (!status.value.enabled) return

  if (to.path === LOGIN_PATH) {
    // Déjà connecté : la page de connexion n'a plus rien à proposer.
    if (status.value.user) return navigateTo(safeRedirect(to.query.redirect))
    return
  }

  if (!status.value.user) {
    // `redirect` garde la destination : on revient là où l'on voulait aller.
    return navigateTo({ path: LOGIN_PATH, query: { redirect: to.fullPath } })
  }
})
