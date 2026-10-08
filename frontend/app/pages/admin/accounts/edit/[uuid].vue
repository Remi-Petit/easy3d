<script setup lang="ts">
import type { AccountDraft, AccountView } from '~/composables/useAccounts'
import { PERM } from '~/utils/permissions'

/**
 * Modification d'un compte (`/admin/accounts/edit/<uuid>`).
 *
 * Une **page**, pas une fenêtre : les droits directs demandent de la place, et
 * une adresse se met en signet, se partage et survit à un rechargement — trois
 * choses qu'une fenêtre ne fait pas. La liste n'édite donc rien : elle envoie
 * ici.
 *
 * Les champs sont ceux de `AccountForm` (partagés avec la création) ; la page
 * ne s'occupe que du reste : retrouver le compte, appeler le serveur, confirmer.
 */
definePageMeta({ middleware: 'admin' })

/** Durée d'affichage de la confirmation (le temps de la lire). */
const SAVED_MS = 4000

const { t, te } = useI18n()
const toast = useToast()
const { can } = useAuth()
const route = useRoute()
const comptes = useAccounts()

onMounted(() => void comptes.refresh())

const peutEcrire = computed(() => can(PERM.usersWrite))

/** Identifiant porté par l'adresse. */
const uuid = computed(() => String(route.params.uuid ?? ''))

/**
 * Le compte, retrouvé dans la liste que `useAccounts` charge déjà.
 *
 * Il n'est pas lu à part : un compte supprimé entre-temps doit se voir ici comme
 * introuvable, et non comme une page qui afficherait encore ses anciennes
 * valeurs parce qu'elle les garderait de côté.
 */
const compte = computed<AccountView | null>(
  () => comptes.users.value.find((user) => user.uuid === uuid.value) ?? null,
)

/** Message de la dernière erreur, traduit depuis son code (comme la liste). */
const error = computed(() => {
  const code = comptes.errorCode.value
  if (!code) return ''
  const key = `accounts.errors.${code}`
  return te(key) ? t(key) : t('accounts.errors.unknown')
})

/**
 * Enregistre **sans quitter la page** : la réussite part en notification.
 *
 * On modifie souvent un seul réglage à la fois ; revenir à la liste ferait
 * perdre le compte qu'on venait de régler. La notification dit ce qui a été
 * enregistré et pour qui — c'est la seule chose qui l'annonce, l'écran ne
 * changeant plus.
 *
 * Un refus du serveur (nom déjà pris, dernier administrateur…) laisse la page
 * en place : on corrige et on réessaie, et l'erreur s'affiche juste en dessous.
 */
async function enregistrer(patch: AccountDraft) {
  if (!(await comptes.updateAccount(uuid.value, patch))) return
  toast.add({
    title: t('accounts.saved'),
    description: t('accounts.savedFor', { name: patch.username }),
    color: 'success',
    icon: 'i-lucide-check',
    duration: SAVED_MS,
  })
}

/** Supprime le compte, puis revient à la liste — il n'y a plus rien à montrer. */
async function supprimer() {
  if (await comptes.deleteAccount(uuid.value)) void navigateTo('/admin/accounts')
}

/** Retour à la liste, sans rien enregistrer. */
function retour() {
  void navigateTo('/admin/accounts')
}
</script>

<template>
  <div class="accounts-page">
    <!-- Le retour vise la **liste**, pas l'historique : l'adresse d'un compte
         peut être ouverte directement, et l'on sait où l'on doit revenir. -->
    <NuxtLink to="/admin/accounts" class="back">{{ t('common.back') }}</NuxtLink>

    <section class="admin__card">
      <h2 class="admin__title">
        {{ t('accounts.editUser') }}
        <span v-if="compte" class="admin__count">{{ compte.username }}</span>
      </h2>

      <p v-if="!compte && comptes.loading.value" class="admin__hint">{{ t('common.loading') }}</p>
      <p v-else-if="!compte" class="admin__status admin__status--err">
        {{ t('accounts.errors.not_found') }}
      </p>

      <AccountForm
        v-else
        :account="compte"
        :role-refs="comptes.roleRefs.value"
        :roles="comptes.roles.value"
        :permissions="comptes.permissions.value"
        :busy="comptes.busy.value"
        :writable="peutEcrire"
        @submit="enregistrer"
        @remove="supprimer"
      />

      <div v-if="compte" class="admin__field admin__field--actions">
        <button type="button" class="admin__action" :disabled="comptes.busy.value" @click="retour()">
          {{ t('common.cancel') }}
        </button>
      </div>
    </section>

    <!-- Une seule erreur, sous la carte : c'est la dernière action qui l'a
         produite, quelle qu'elle soit. -->
    <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
  </div>
</template>
