<script setup lang="ts">
import type { AccountDraft } from '~/composables/useAccounts'
import { PERM } from '~/utils/permissions'

/**
 * Création d'un compte (`/admin/accounts/new`).
 *
 * Même formulaire que la modification (`AccountForm`), mais sans compte : les
 * deux pages sont faites pour se ressembler, et l'adresse dit laquelle on
 * regarde.
 */
definePageMeta({ middleware: 'admin' })

const { t, te } = useI18n()
const { can } = useAuth()
const comptes = useAccounts()

onMounted(() => void comptes.refresh())

const peutEcrire = computed(() => can(PERM.usersWrite))

/** Message de la dernière erreur, traduit depuis son code (comme la liste). */
const error = computed(() => {
  const code = comptes.errorCode.value
  if (!code) return ''
  const key = `accounts.errors.${code}`
  return te(key) ? t(key) : t('accounts.errors.unknown')
})

/**
 * Crée le compte, puis revient à la liste : le nouveau compte y est, et c'est
 * là qu'on veut le voir. Un refus (nom déjà pris, mot de passe trop court…)
 * laisse la page ouverte, avec son message.
 */
async function creer(patch: AccountDraft) {
  const created = await comptes.createAccount({
    username: patch.username,
    email: patch.email,
    // Vide est accepté : le compte ne se connectera que par le SSO.
    password: patch.password ?? '',
    roles: patch.roles,
    permissions: patch.permissions,
  })
  if (created) void navigateTo('/admin/accounts')
}

/** Retour à la liste, sans rien créer. */
function retour() {
  void navigateTo('/admin/accounts')
}
</script>

<template>
  <div class="accounts-page">
    <NuxtLink to="/admin/accounts" class="back">{{ t('common.back') }}</NuxtLink>

    <section class="admin__card">
      <h2 class="admin__title">{{ t('accounts.newUser') }}</h2>

      <AccountForm
        :role-refs="comptes.roleRefs.value"
        :roles="comptes.roles.value"
        :permissions="comptes.permissions.value"
        :busy="comptes.busy.value"
        :writable="peutEcrire"
        @submit="creer"
      />

      <div class="admin__field admin__field--actions">
        <button type="button" class="admin__action" :disabled="comptes.busy.value" @click="retour()">
          {{ t('common.cancel') }}
        </button>
      </div>
    </section>

    <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
  </div>
</template>
