import { PERM } from '~/utils/permissions'

/**
 * Garde des pages d'administration.
 *
 * `/admin` montre les réglages (droit `config.read`), `/admin/comptes` les
 * comptes et les rôles (`users.read` ou `roles.read`), `/admin/journal` le
 * journal d'audit (`users.read` : c'est un journal de comptes). Un compte qui n'a
 * aucun de ces droits n'a rien à y faire : mieux vaut le renvoyer au catalogue
 * que lui montrer des écrans vides et des refus.
 *
 * La garde tourne **après** `auth.global.ts` (les gardes globales passent
 * d'abord), donc l'état de l'authentification est déjà connu — et quand les
 * comptes sont éteints, `can()` répond `true` partout : l'administration reste
 * ouverte, comme avant.
 */
export default defineNuxtRouteMiddleware((to) => {
  const { can } = useAuth()

  const droits = to.path.startsWith('/admin/comptes')
    ? [PERM.usersRead, PERM.rolesRead]
    : to.path.startsWith('/admin/journal')
      ? [PERM.usersRead]
      : [PERM.configRead]

  if (!droits.some(can)) return navigateTo('/')
})
