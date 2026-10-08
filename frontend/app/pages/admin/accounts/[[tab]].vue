<script setup lang="ts">
import type { AccountPatch, AccountView, RolePatch, RoleView } from '~/composables/useAccounts'
import { PERM } from '~/utils/permissions'

/**
 * Comptes et rôles (`/admin/accounts`), en deux onglets.
 *
 * Page **à part** de `/admin` : les réglages et les notes y occupent déjà deux
 * colonnes, et un écran de gestion des comptes demande de la place (liste, rôle
 * sélectionné, cases à cocher). La garde `admin.ts` s'occupe du droit d'entrée.
 *
 * Deux vues, deux URL — le système d'onglets de « Mon compte » (voir
 * `PanelTabs`) : `/admin/accounts` liste les utilisateurs,
 * `/admin/accounts/roles` les rôles. Chacune garde la grille de
 * l'administration : la liste et l'éditeur à gauche, la création à droite.
 *
 * Tout ce qui n'est pas permis est **masqué**, à partir des droits du compte
 * (`can()`) : proposer un bouton que le serveur refusera est la pire façon
 * d'annoncer un droit manquant.
 */
definePageMeta({ middleware: 'admin' })

const { t, te } = useI18n()
const { can } = useAuth()
const route = useRoute()
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

// ── Onglets ──────────────────────────────────────────────────────────────
/**
 * Deux onglets, deux URL : la page ne montre qu'une vue à la fois, et c'est
 * l'adresse qui fait foi. Un lien vers `/admin/accounts/roles` ouvre
 * directement les rôles, et le bouton « retour » refait le trajet inverse.
 */
type Onglet = 'utilisateurs' | 'roles'

/** Onglets, dans l'ordre d'affichage. */
const ONGLETS: Onglet[] = ['utilisateurs', 'roles']

/** Segment de chaque onglet : la liste des utilisateurs est la racine. */
const chemins: Record<Onglet, string> = {
  utilisateurs: '/admin/accounts',
  roles: '/admin/accounts/roles',
}

/** L'onglet porté par l'URL, ou les utilisateurs pour tout segment inconnu. */
const onglet = computed<Onglet>(
  () => ONGLETS.find((nom) => nom === route.params.tab) ?? 'utilisateurs',
)

/** Libellés, déjà traduits (voir `PanelTabs`). */
const onglets = computed(() => [
  { id: 'utilisateurs', label: t('accounts.users') },
  { id: 'roles', label: t('accounts.roles') },
])

/** Changer d'onglet, c'est changer d'URL : c'est elle qui fait foi. */
function ouvreOnglet(id: string) {
  const cible = ONGLETS.find((nom) => nom === id)
  if (cible && cible !== onglet.value) void navigateTo(chemins[cible])
}

/**
 * Un segment inconnu (`/admin/accounts/xyz`) n'est pas une vue : on revient à
 * l'onglet par défaut, sans laisser l'adresse fautive dans l'historique.
 */
watchEffect(() => {
  const segment = route.params.tab
  if (segment !== undefined && segment !== '' && !ONGLETS.some((nom) => nom === segment)) {
    void navigateTo(chemins.utilisateurs, { replace: true })
  }
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

/** Nom du nouveau rôle, et le rôle à cloner le cas échéant (`''` = aucun). */
const nouveauRole = reactive({ name: '', from: '' })

/**
 * Nouveau rôle : à partir de rien, ou cloné d'un rôle existant.
 *
 * Le `<select>` dit « partir d'un rôle vide » par une **chaîne vide**, et non
 * par une valeur absente : sans le nettoyage ci-dessous, le backend cherche un
 * rôle dont l'identifiant est `''` et refuse la création (`unknown_role`) — le
 * bouton « Créer » ne marchait pas, seul « Cloner » aboutissait.
 */
async function createRole(roleFrom: string | null = null) {
  if (!nouveauRole.name.trim()) return
  const source = roleFrom || nouveauRole.from || undefined
  const created = await comptes.createRole({
    name: nouveauRole.name,
    from: source,
  })
  if (created) {
    nouveauRole.name = ''
    nouveauRole.from = ''
  }
}

/** Suppression en deux temps : le second clic confirme. */
const roleASupprimer = ref<string | null>(null)
const compteASupprimer = ref<string | null>(null)

/**
 * Supprime le rôle sélectionné, puis referme son éditeur : il ne décrit plus
 * rien, et ses champs garderaient sinon le nom d'un rôle qui n'existe plus.
 */
async function deleteRole() {
  if (!selectedRole.value) return
  if (await comptes.deleteRole(selectedRole.value)) {
    selectedRole.value = null
    roleASupprimer.value = null
  }
}

/** Même chose pour un compte. */
async function deleteAccount() {
  if (!selectedUser.value) return
  if (await comptes.deleteAccount(selectedUser.value)) {
    selectedUser.value = null
    compteASupprimer.value = null
  }
}

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
  <div class="tabbed-page">
    <!--
      Deux onglets, deux URL : c'est l'adresse qui décide de la vue affichée.
      La barre est la même que sur « Mon compte » (voir `PanelTabs`) : flèches
      gauche/droite, et l'URL suit.
    -->
    <PanelTabs :tabs="onglets" :active="onglet" :label="t('nav.accounts')" @select="ouvreOnglet" />

    <!-- ── Utilisateurs ──────────────────────────────────────────────── -->
    <div
      v-if="onglet === 'utilisateurs'"
      id="panneau-utilisateurs"
      role="tabpanel"
      aria-labelledby="onglet-utilisateurs"
      class="admin"
    >
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
                @click="deleteAccount()"
              >
                {{ t('accounts.remove') }}
              </button>
            </div>

            <p class="admin__hint">{{ t('accounts.hint') }}</p>
          </div>
        </section>
      </div>

      <!-- Nouveau compte -->
      <div class="admin__col">
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
      </div>
    </div>

    <!-- ── Rôles ─────────────────────────────────────────────────────── -->
    <div
      v-else
      id="panneau-roles"
      role="tabpanel"
      aria-labelledby="onglet-roles"
      class="admin"
    >
      <div class="admin__col">
        <section class="admin__card">
          <h2 class="admin__title">
            {{ t('accounts.roles') }}
            <span class="admin__count">{{ comptes.roles.value.length }}</span>
          </h2>
          <!-- Une phrase, pas un compteur : c'est le style des explications. -->
          <p class="admin__hint">{{ t('accounts.rolesHint') }}</p>

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
                @click="deleteRole()"
              >
                {{ t('accounts.remove') }}
              </button>
            </div>

            <p v-if="roleFige" class="admin__hint">{{ t('accounts.frozen') }}</p>
          </div>
        </section>
      </div>

      <!-- Nouveau rôle, ou clone d'un rôle existant -->
      <div class="admin__col">
        <section v-if="peutEcrireRoles" class="admin__card">
          <h2 class="admin__title">{{ t('accounts.newRole') }}</h2>
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
        </section>
      </div>
    </div>

    <!-- Une seule erreur pour les deux vues : c'est la dernière action, quelle
         qu'elle soit, qui l'a produite. -->
    <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
  </div>
</template>
