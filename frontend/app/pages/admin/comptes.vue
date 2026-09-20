<script setup lang="ts">
import type { AccountPatch, AccountView, RolePatch, RoleView } from '~/composables/useAccounts'
import { PERM } from '~/utils/permissions'

/**
 * Comptes et rôles (`/admin/comptes`).
 *
 * Page **à part** de `/admin` : les réglages et les notes y occupent déjà deux
 * colonnes, et un écran de gestion des comptes demande de la place (liste, rôle
 * sélectionné, cases à cocher). La garde `admin.ts` s'occupe du droit d'entrée.
 *
 * Tout ce qui n'est pas permis est **masqué**, à partir des droits du compte
 * (`can()`) : proposer un bouton que le serveur refusera est la pire façon
 * d'annoncer un droit manquant.
 */
definePageMeta({ middleware: 'admin' })

const { t, te } = useI18n()
const { can } = useAuth()
const comptes = useAccounts()

onMounted(() => void comptes.refresh())

const peutEcrireComptes = computed(() => can(PERM.usersWrite))
const peutEcrireRoles = computed(() => can(PERM.rolesWrite))

/** Message de la dernière erreur, traduit depuis son code. */
const error = computed(() => {
  const code = comptes.errorCode.value
  if (!code) return ''
  const key = `accounts.errors.${code}`
  return te(key) ? t(key) : t('accounts.errors.unknown')
})

// ── Rôles ────────────────────────────────────────────────────────────────
const selectedRole = ref<string | null>(null)
const draftRole = reactive<RolePatch & { name: string; description: string; permissions: string[] }>({
  name: '',
  description: '',
  permissions: [],
})

function selectRole(role: RoleView) {
  selectedRole.value = role.uuid
  draftRole.name = role.name
  draftRole.description = role.description
  draftRole.permissions = [...role.permissions]
}

/** `true` quand le rôle sélectionné est livré (figé). */
const roleFige = computed(
  () => comptes.roles.value.find((role) => role.uuid === selectedRole.value)?.builtin ?? false,
)

async function saveRole() {
  if (!selectedRole.value || roleFige.value) return
  await comptes.updateRole(selectedRole.value, {
    name: draftRole.name,
    description: draftRole.description,
    permissions: [...draftRole.permissions],
  })
}

/** Nouveau rôle : à partir de rien, ou cloné d'un rôle existant. */
const nouveauRole = reactive({ name: '', from: '' })

async function createRole(roleFrom: string | null = null) {
  if (!nouveauRole.name.trim()) return
  const created = await comptes.createRole({
    name: nouveauRole.name,
    from: roleFrom ?? nouveauRole.from ?? undefined,
  })
  if (created) {
    nouveauRole.name = ''
    nouveauRole.from = ''
  }
}

/** Suppression en deux temps : le second clic confirme. */
const roleASupprimer = ref<string | null>(null)
const compteASupprimer = ref<string | null>(null)

// ── Comptes ──────────────────────────────────────────────────────────────
const selectedUser = ref<string | null>(null)
const draftUser = reactive<Required<AccountPatch>>({
  username: '',
  email: '',
  password: '',
  roles: [],
  permissions: [],
  disabled: false,
})

function selectUser(user: AccountView) {
  selectedUser.value = user.uuid
  draftUser.username = user.username
  draftUser.email = user.email
  draftUser.password = ''
  draftUser.roles = user.roles.map((role) => role.uuid)
  draftUser.permissions = [...user.direct]
  draftUser.disabled = user.disabled
}

const compteSelectionne = computed(() =>
  comptes.users.value.find((user) => user.uuid === selectedUser.value),
)

/** `true` si le compte sélectionné est superutilisateur (rôle livré `admin`). */
const estSuperuser = computed(() =>
  (compteSelectionne.value?.roles ?? []).some((role) => role.name === 'admin'),
)

async function saveUser() {
  if (!selectedUser.value) return
  const patch: AccountPatch = {
    username: draftUser.username,
    email: draftUser.email,
    roles: [...draftUser.roles],
    permissions: [...draftUser.permissions],
    disabled: draftUser.disabled,
  }
  // Le mot de passe n'est envoyé que s'il a été saisi : un champ vide veut dire
  // « ne le change pas », jamais « efface-le » (c'est l'interface qui parle, pas
  // un formulaire qu'on peut vider par accident).
  if (draftUser.password) patch.password = draftUser.password
  if (await comptes.updateAccount(selectedUser.value, patch)) draftUser.password = ''
}

// ── Création d'un compte ─────────────────────────────────────────────────
const nouveauCompte = reactive({
  username: '',
  email: '',
  password: '',
  roles: [] as string[],
})

async function createAccount() {
  if (!nouveauCompte.username.trim() || !nouveauCompte.email.trim()) return
  const created = await comptes.createAccount({
    username: nouveauCompte.username,
    email: nouveauCompte.email,
    password: nouveauCompte.password,
    roles: [...nouveauCompte.roles],
    permissions: [],
  })
  if (created) {
    nouveauCompte.username = ''
    nouveauCompte.email = ''
    nouveauCompte.password = ''
    nouveauCompte.roles = []
  }
}

/** `true` si un rôle donné fait partie de la sélection en cours. */
function has(items: string[], id: string) {
  return items.includes(id)
}

/** Coche ou décoche un rôle dans une liste (utile aux deux formulaires). */
function toggleIn(list: string[], id: string) {
  return has(list, id) ? list.filter((known) => known !== id) : [...list, id]
}
</script>

<template>
  <div class="admin">
    <!-- ── Colonne des rôles ─────────────────────────────────────────── -->
    <div class="admin__col">
      <section class="admin__card">
        <h2 class="admin__title">{{ t('accounts.roles') }}</h2>
        <p class="admin__count">
          {{ t('accounts.rolesHint') }}
        </p>

        <ul class="admin__list">
          <li
            v-for="role in comptes.roles.value"
            :key="role.uuid"
            class="admin__item"
            :class="{ 'admin__item--on': role.uuid === selectedRole }"
          >
            <button type="button" class="accounts__pick" @click="selectRole(role)">
              <span class="admin__item-label">
                {{ role.name }}
                <span v-if="role.builtin" class="accounts__tag">{{ t('accounts.builtin') }}</span>
                <span v-if="role.uuid === comptes.defaultRole.value" class="accounts__tag accounts__tag--on">
                  {{ t('accounts.byDefault') }}
                </span>
              </span>
              <span class="admin__item-where">
                {{ t('accounts.members', role.members, { count: role.members }) }}
              </span>
            </button>
          </li>
        </ul>

        <!-- Éditeur du rôle sélectionné -->
        <div v-if="selectedRole" class="admin__field">
          <div class="admin__row">
            <input
              v-model="draftRole.name"
              class="admin__input"
              :disabled="roleFige || !peutEcrireRoles"
              :aria-label="t('accounts.roleName')"
            />
          </div>
          <div class="admin__row">
            <input
              v-model="draftRole.description"
              class="admin__input"
              :disabled="roleFige || !peutEcrireRoles"
              :placeholder="t('accounts.roleDescription')"
              :aria-label="t('accounts.roleDescription')"
            />
          </div>

          <PermissionPicker
            v-model="draftRole.permissions"
            :permissions="comptes.permissions.value"
            :disabled="roleFige || !peutEcrireRoles"
          />

          <div class="admin__field admin__field--actions">
            <button
              v-if="peutEcrireRoles"
              type="button"
              class="admin__action admin__action--primary"
              :disabled="roleFige || comptes.busy.value"
              @click="saveRole()"
            >
              {{ t('accounts.save') }}
            </button>
            <button
              v-if="peutEcrireRoles"
              type="button"
              class="admin__action"
              :disabled="comptes.busy.value"
              @click="comptes.setDefaultRole(selectedRole)"
            >
              {{ t('accounts.setDefault') }}
            </button>
            <button
              v-if="peutEcrireRoles && !roleFige"
              type="button"
              class="admin__action"
              :disabled="comptes.busy.value"
              @click="roleASupprimer = roleASupprimer === selectedRole ? null : selectedRole"
            >
              {{ roleASupprimer === selectedRole ? t('accounts.confirm') : t('accounts.remove') }}
            </button>
            <button
              v-if="peutEcrireRoles && roleASupprimer === selectedRole"
              type="button"
              class="admin__action admin__action--danger"
              :disabled="comptes.busy.value"
              @click="comptes.deleteRole(selectedRole!)"
            >
              {{ t('accounts.remove') }}
            </button>
          </div>

          <p v-if="roleFige" class="admin__hint">{{ t('accounts.frozen') }}</p>
        </div>

        <!-- Nouveau rôle, ou clone d'un rôle existant -->
        <div v-if="peutEcrireRoles" class="admin__field">
          <span class="admin__label">{{ t('accounts.newRole') }}</span>
          <div class="admin__row">
            <input
              v-model="nouveauRole.name"
              class="admin__input"
              :placeholder="t('accounts.roleName')"
              :aria-label="t('accounts.roleName')"
            />
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="comptes.busy.value || !nouveauRole.name.trim()"
              @click="createRole()"
            >
              {{ t('accounts.create') }}
            </button>
          </div>
          <p class="admin__hint">{{ t('accounts.cloneHint') }}</p>
          <div class="admin__row">
            <select v-model="nouveauRole.from" class="admin__input" :aria-label="t('accounts.cloneFrom')">
              <option value="">{{ t('accounts.cloneNone') }}</option>
              <option v-for="role in comptes.roles.value" :key="role.uuid" :value="role.uuid">
                {{ role.name }}
              </option>
            </select>
            <button
              type="button"
              class="admin__action"
              :disabled="comptes.busy.value || !nouveauRole.name.trim() || !nouveauRole.from"
              @click="createRole(nouveauRole.from)"
            >
              {{ t('accounts.clone') }}
            </button>
          </div>
        </div>
      </section>
    </div>

    <!-- ── Colonne des comptes ───────────────────────────────────────── -->
    <div class="admin__col">
      <section class="admin__card">
        <h2 class="admin__title">
          {{ t('accounts.users') }}
          <span class="admin__count">{{ comptes.users.value.length }}</span>
        </h2>

        <ul class="admin__list">
          <li
            v-for="user in comptes.users.value"
            :key="user.uuid"
            class="admin__item"
            :class="{ 'admin__item--on': user.uuid === selectedUser }"
          >
            <button type="button" class="accounts__pick" @click="selectUser(user)">
              <span class="admin__item-label">
                {{ user.username }}
                <span v-if="user.disabled" class="accounts__tag">{{ t('accounts.disabled') }}</span>
                <!-- Rattaché au fournisseur d'identité : en provisionnement
                     « manuel », c'est ce qui distingue un compte qui peut
                     entrer par le SSO d'un compte qui ne le peut pas encore. -->
                <span v-if="user.oidc" class="accounts__tag accounts__tag--on">
                  {{ t('accounts.oidc') }}
                </span>
              </span>
              <span class="admin__item-where">
                {{ user.email }} · {{ user.roles.map((role) => role.name).join(', ') || t('accounts.noRole') }}
              </span>
            </button>
          </li>
        </ul>

        <p v-if="!comptes.users.value.length" class="admin__empty">{{ t('accounts.noUser') }}</p>

        <!-- Éditeur du compte sélectionné -->
        <div v-if="selectedUser && compteSelectionne" class="admin__field">
          <div class="admin__row">
            <input
              v-model="draftUser.username"
              class="admin__input"
              :disabled="!peutEcrireComptes"
              :aria-label="t('accounts.userName')"
            />
            <input
              v-model="draftUser.email"
              class="admin__input"
              :disabled="!peutEcrireComptes"
              :aria-label="t('accounts.email')"
            />
          </div>
          <div class="admin__row">
            <input
              v-model="draftUser.password"
              type="password"
              class="admin__input"
              autocomplete="new-password"
              :disabled="!peutEcrireComptes"
              :placeholder="t('accounts.newPassword')"
              :aria-label="t('accounts.newPassword')"
            />
          </div>

          <span class="admin__label">{{ t('accounts.rolesOf') }}</span>
          <div class="admin__choices">
            <label
              v-for="role in comptes.roleRefs.value"
              :key="role.uuid"
              class="admin__choice"
              :class="{ 'admin__choice--on': has(draftUser.roles, role.uuid) }"
            >
              <input
                type="checkbox"
                :checked="has(draftUser.roles, role.uuid)"
                :disabled="!peutEcrireComptes"
                @change="draftUser.roles = toggleIn(draftUser.roles, role.uuid)"
              />
              {{ role.name }}
            </label>
          </div>

          <span class="admin__label">{{ t('accounts.direct') }}</span>
          <PermissionPicker
            v-model="draftUser.permissions"
            :permissions="comptes.permissions.value"
            :disabled="!peutEcrireComptes"
            :locked="estSuperuser"
          />

          <div class="admin__field admin__field--actions">
            <button
              v-if="peutEcrireComptes"
              type="button"
              class="admin__action admin__action--primary"
              :disabled="comptes.busy.value"
              @click="saveUser()"
            >
              {{ t('accounts.save') }}
            </button>
            <button
              v-if="peutEcrireComptes"
              type="button"
              class="admin__action"
              :disabled="comptes.busy.value || estSuperuser"
              @click="draftUser.disabled = !draftUser.disabled; saveUser()"
            >
              {{ draftUser.disabled ? t('accounts.enable') : t('accounts.disable') }}
            </button>
            <button
              v-if="peutEcrireComptes"
              type="button"
              class="admin__action"
              :disabled="comptes.busy.value"
              @click="compteASupprimer = compteASupprimer === selectedUser ? null : selectedUser"
            >
              {{ compteASupprimer === selectedUser ? t('accounts.confirm') : t('accounts.remove') }}
            </button>
            <button
              v-if="peutEcrireComptes && compteASupprimer === selectedUser"
              type="button"
              class="admin__action admin__action--danger"
              :disabled="comptes.busy.value"
              @click="comptes.deleteAccount(selectedUser!)"
            >
              {{ t('accounts.remove') }}
            </button>
          </div>

          <p class="admin__hint">{{ t('accounts.hint') }}</p>
        </div>
      </section>

      <!-- Nouveau compte -->
      <section v-if="peutEcrireComptes" class="admin__card">
        <h2 class="admin__title">{{ t('accounts.newUser') }}</h2>
        <div class="admin__row">
          <input
            v-model="nouveauCompte.username"
            class="admin__input"
            :placeholder="t('accounts.userName')"
            :aria-label="t('accounts.userName')"
          />
          <input
            v-model="nouveauCompte.email"
            class="admin__input"
            :placeholder="t('accounts.email')"
            :aria-label="t('accounts.email')"
          />
        </div>
        <div class="admin__row">
          <input
            v-model="nouveauCompte.password"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :placeholder="t('accounts.password')"
            :aria-label="t('accounts.password')"
          />
        </div>
        <!-- Le mot de passe est facultatif : un compte sans mot de passe ne se
             connecte que par le fournisseur d'identité (SSO). -->
        <p class="admin__hint">{{ t('accounts.passwordHint') }}</p>
        <div class="admin__choices">
          <label
            v-for="role in comptes.roleRefs.value"
            :key="role.uuid"
            class="admin__choice"
            :class="{ 'admin__choice--on': has(nouveauCompte.roles, role.uuid) }"
          >
            <input
              type="checkbox"
              :checked="has(nouveauCompte.roles, role.uuid)"
              @change="nouveauCompte.roles = toggleIn(nouveauCompte.roles, role.uuid)"
            />
            {{ role.name }}
          </label>
        </div>
        <div class="admin__field admin__field--actions">
          <button
            type="button"
            class="admin__action admin__action--primary"
            :disabled="comptes.busy.value || !nouveauCompte.username.trim() || !nouveauCompte.email.trim()"
            @click="createAccount()"
          >
            {{ t('accounts.create') }}
          </button>
        </div>
        <p class="admin__hint">{{ t('accounts.defaultHint') }}</p>
      </section>

      <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
    </div>
  </div>
</template>
