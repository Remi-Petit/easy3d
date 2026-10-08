<script setup lang="ts">
import type { AccountPatch, AccountView, RolePatch, RoleView } from '~/composables/useAccounts'
import type { GridColumn } from '~/utils/grid'
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
 * `/admin/accounts/roles` les rôles. Les **utilisateurs** sont un tableau à
 * cinq colonnes, avec ses filtres et son bouton d'ajout au-dessus ; les rôles
 * gardent la grille de l'administration — la liste et l'éditeur à gauche, la
 * création à droite. La saisie d'un compte (création, édition) vit dans une
 * **fenêtre** : elle demande de la place (rôles, cases à cocher) et n'a pas à
 * pousser le tableau.
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
    // La fenêtre d'édition ne décrit plus rien : elle se referme avec le compte.
    editionOuverte.value = false
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

/**
 * Fenêtres de saisie : une pour la création, une pour le compte choisi.
 *
 * Le formulaire vivait **sous** la liste, dans la colonne de gauche : il fallait
 * le faire défiler pour voir la fin d'un compte, et la deuxième colonne restait
 * vide dès qu'on n'avait pas le droit d'écrire. Dans une fenêtre, le tableau
 * occupe toute la largeur.
 */
const creationOuverte = ref(false)
const editionOuverte = ref(false)

function selectUser(user: AccountView) {
  selectedUser.value = user.uuid
  draftUser.username = user.username
  draftUser.email = user.email
  draftUser.password = ''
  draftUser.roles = user.roles.map((role) => role.uuid)
  draftUser.permissions = [...user.direct]
  draftUser.disabled = user.disabled
}

/** Ouvre l'éditeur sur ce compte : le brouillon part de ses valeurs actuelles. */
function editerUser(user: AccountView) {
  selectUser(user)
  compteASupprimer.value = null
  editionOuverte.value = true
}

/**
 * La grille rend la **ligne** qu'elle a reçue (un objet de données, pas un
 * compte typé) : on retrouve le compte par son identifiant, puisque c'est lui
 * qui fait foi — la grille peut avoir été rendue avant un rafraîchissement.
 */
function ouvrirCompte(row: Record<string, unknown>) {
  const compte = comptes.users.value.find((user) => user.uuid === row.uuid)
  if (compte) editerUser(compte)
}

const compteSelectionne = computed(() =>
  comptes.users.value.find((user) => user.uuid === selectedUser.value),
)

/** `true` si le compte sélectionné est superutilisateur (rôle livré `admin`). */
const estSuperuser = computed(() =>
  (compteSelectionne.value?.roles ?? []).some((role) => role.name === 'admin'),
)

/** Enregistre le compte sélectionné ; `true` si le serveur a accepté. */
async function saveUser(): Promise<boolean> {
  if (!selectedUser.value) return false
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
  const ok = await comptes.updateAccount(selectedUser.value, patch)
  if (ok) draftUser.password = ''
  return ok
}

/**
 * Le bouton « Enregistrer » de la fenêtre : un refus du serveur (nom déjà pris,
 * dernier administrateur…) laisse la fenêtre ouverte — on corrige et on
 * réessaie, sans avoir à rouvrir le compte.
 */
async function enregistrerCompte() {
  if (await saveUser()) editionOuverte.value = false
}

// ── Filtres du tableau ───────────────────────────────────────────────────
/** Recherche libre, sur le nom **et** l'adresse. */
const recherche = ref('')

/** Rôle retenu (son `uuid`), ou `''` pour tous. */
const filtreRole = ref('')

/** État retenu : tous, actifs, désactivés, ou comptes provisionnés par le SSO. */
type FiltreStatut = '' | 'active' | 'disabled' | 'oidc'
const filtreStatut = ref<FiltreStatut>('')

/** Les filtres se **cumulent** : chacun resserre la liste du précédent. */
const utilisateurs = computed(() =>
  comptes.users.value.filter((user) => {
    const texte = recherche.value.trim().toLowerCase()
    if (texte && !`${user.username} ${user.email}`.toLowerCase().includes(texte)) return false
    if (filtreRole.value && !user.roles.some((role) => role.uuid === filtreRole.value)) return false
    if (filtreStatut.value === 'active' && user.disabled) return false
    if (filtreStatut.value === 'disabled' && !user.disabled) return false
    if (filtreStatut.value === 'oidc' && !user.oidc) return false
    return true
  }),
)

const filtreActif = computed(
  () => !!recherche.value.trim() || !!filtreRole.value || !!filtreStatut.value,
)

function effacerFiltres() {
  recherche.value = ''
  filtreRole.value = ''
  filtreStatut.value = ''
}

/** Une ligne du tableau : le compte, et ses rôles déjà mis en mots. */
type LigneCompte = AccountView & { rolesLabel: string }

const lignes = computed<LigneCompte[]>(() =>
  utilisateurs.value.map((user) => ({
    ...user,
    rolesLabel: user.roles.map((role) => role.name).join(', '),
  })),
)

/**
 * Colonnes de la grille (voir `DataGrid.vue` et `utils/grid.ts`).
 *
 * Les largeurs sont des **propositions** : la grille les laisse ajuster à la
 * souris, et le redimensionnement vit chez elle — la page ne le sait pas.
 *
 * Leur somme (≈ 780 px) tient dans la carte aux largeurs de fenêtre usuelles :
 * au-delà, la grille défile horizontalement (la colonne des actions sortirait
 * sinon de la zone visible).
 *
 * Deux colonnes ne se trient pas sur ce qu'elles montrent : les rôles (mis en
 * mots, donc triés sur le libellé) et l'état (`0`/`1` : un compte désactivé
 * passe en tête, ce qui est exactement ce qu'on vient chercher).
 */
const colonnes = computed<GridColumn<LigneCompte>[]>(() => [
  { id: 'username', header: t('accounts.userName'), width: 180, sortable: true },
  { id: 'email', header: t('accounts.email'), width: 230, sortable: true },
  {
    id: 'rolesLabel',
    header: t('accounts.roles'),
    width: 140,
    sortable: true,
    sortValue: (compte) => compte.rolesLabel,
  },
  {
    id: 'statut',
    header: t('accounts.status'),
    width: 130,
    sortable: true,
    sortValue: (compte) => (compte.disabled ? '0' : '1'),
  },
  { id: 'actions', header: t('accounts.actions'), width: 100, align: 'center' },
])

// ── Création d'un compte ─────────────────────────────────────────────────
const nouveauCompte = reactive({
  username: '',
  email: '',
  password: '',
  roles: [] as string[],
})

/** Ouvre la fenêtre de création sur un formulaire **vierge** (ouverte deux fois). */
function ouvrirCreation() {
  nouveauCompte.username = ''
  nouveauCompte.email = ''
  nouveauCompte.password = ''
  nouveauCompte.roles = []
  creationOuverte.value = true
}

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
    creationOuverte.value = false
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
      class="accounts"
    >
      <section class="admin__card">
        <h2 class="admin__title">
          {{ t('accounts.users') }}
          <span class="admin__count">{{ comptes.users.value.length }}</span>
        </h2>

        <!--
          Filtres, puis l'action d'ajout : la rangée reprend celle du catalogue
          (`.toolbar`) pour la facture — le champ large avec sa loupe, puis les
          deux listes. Le bouton d'ajout ferme la rangée, à droite : c'est le
          geste qui ne dépend pas de ce qu'on filtre.
        -->
        <div class="toolbar accounts__toolbar">
          <div class="search">
            <span class="icon">🔍</span>
            <input
              v-model="recherche"
              type="search"
              :placeholder="t('accounts.search')"
              :aria-label="t('accounts.search')"
            />
          </div>

          <select v-model="filtreRole" class="admin__input" :aria-label="t('accounts.roles')">
            <option value="">{{ t('accounts.filter.allRoles') }}</option>
            <option v-for="role in comptes.roles.value" :key="role.uuid" :value="role.uuid">
              {{ role.name }}
            </option>
          </select>

          <select v-model="filtreStatut" class="admin__input" :aria-label="t('accounts.status')">
            <option value="">{{ t('accounts.filter.allStatus') }}</option>
            <option value="active">{{ t('accounts.filter.active') }}</option>
            <option value="disabled">{{ t('accounts.filter.disabled') }}</option>
            <option value="oidc">{{ t('accounts.oidc') }}</option>
          </select>

          <button v-if="filtreActif" type="button" class="types__clear" @click="effacerFiltres()">
            {{ t('filter.clear') }}
          </button>

          <button
            v-if="peutEcrireComptes"
            type="button"
            class="admin__action admin__action--primary accounts__add"
            @click="ouvrirCreation()"
          >
            {{ t('accounts.addUser') }}
          </button>
        </div>

        <!-- Compte des lignes affichées, puis l'explication du droit en retrait. -->
        <p class="accounts__meta">
          <span class="section-label">
            {{ t('accounts.filter.count', { shown: lignes.length, total: comptes.users.value.length }) }}
          </span>
          <span class="admin__hint">{{ t('accounts.hint') }}</span>
        </p>

        <!--
          La grille maison (`DataGrid.vue`) : virtualisée, en-tête collant, tri
          au clic sur l'en-tête, colonnes ajustables, flèches au clavier. C'est
          la facture d'un `st.dataframe` de Streamlit, sans second framework.
        -->
        <DataGrid
          v-model:selected="selectedUser"
          :columns="colonnes"
          :rows="lignes"
          :label="t('accounts.users')"
          :loading="comptes.loading.value"
          :empty="filtreActif ? t('accounts.noMatch') : t('accounts.noUser')"
          @open="ouvrirCompte"
        >
          <template #cell-rolesLabel="{ row }">
            {{ row.rolesLabel || t('accounts.noRole') }}
          </template>

          <!--
            L'état du compte tient dans une pastille : « désactivé » l'emporte
            (c'est ce qui empêche d'entrer), puis le SSO, et « actif » est le
            cas ordinaire — il se dit, lui aussi, plutôt que de laisser la
            cellule vide.
          -->
          <template #cell-statut="{ row }">
            <span v-if="row.disabled" class="accounts__tag accounts__tag--off">
              {{ t('accounts.disabled') }}
            </span>
            <span v-if="row.oidc" class="accounts__tag accounts__tag--on">
              {{ t('accounts.oidc') }}
            </span>
            <span v-if="!row.disabled && !row.oidc" class="accounts__tag">
              {{ t('accounts.active') }}
            </span>
          </template>

          <template #cell-actions="{ row }">
            <button
              v-if="peutEcrireComptes"
              type="button"
              class="admin__action"
              @click="ouvrirCompte(row)"
            >
              {{ t('accounts.edit') }}
            </button>
            <span v-else class="admin__hint">{{ t('common.none') }}</span>
          </template>
        </DataGrid>
      </section>

      <!-- Création d'un compte : une fenêtre, ouverte sur un formulaire vierge
           (le mot de passe est facultatif — voir l'explication dans le corps). -->
      <UModal
        v-model:open="creationOuverte"
        :title="t('accounts.newUser')"
        :description="t('accounts.defaultHint')"
      >
        <template #body>
          <div class="accounts__form">
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

            <span class="admin__label">{{ t('accounts.rolesOf') }}</span>
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
          </div>
        </template>

        <template #footer>
          <div class="admin__field admin__field--actions">
            <button type="button" class="admin__action" @click="creationOuverte = false">
              {{ t('common.cancel') }}
            </button>
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="
                comptes.busy.value || !nouveauCompte.username.trim() || !nouveauCompte.email.trim()
              "
              @click="createAccount()"
            >
              {{ t('accounts.create') }}
            </button>
          </div>
        </template>
      </UModal>

      <!--
        Édition du compte choisi. Le compte peut disparaître sous la fenêtre (le
        serveur fait foi après chaque écriture) : le corps est donc gardé par
        `v-if`, sans quoi les champs liraient un compte qui n'existe plus.
      -->
      <UModal
        v-model:open="editionOuverte"
        :title="t('accounts.editUser')"
        :description="compteSelectionne?.username ?? ''"
      >
        <template #body>
          <div v-if="compteSelectionne" class="accounts__form">
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

            <!-- Désactivation et suppression : le corps de la fenêtre, pas le
                 pied — on n'y vient pas pour confirmer, mais pour agir sur ce
                 compte-là. La suppression se confirme en deux temps. -->
            <div v-if="peutEcrireComptes" class="admin__field admin__field--actions">
              <button
                type="button"
                class="admin__action"
                :disabled="comptes.busy.value || estSuperuser"
                @click="draftUser.disabled = !draftUser.disabled; saveUser()"
              >
                {{ draftUser.disabled ? t('accounts.enable') : t('accounts.disable') }}
              </button>
              <button
                type="button"
                class="admin__action"
                :disabled="comptes.busy.value"
                @click="compteASupprimer = compteASupprimer === selectedUser ? null : selectedUser"
              >
                {{ compteASupprimer === selectedUser ? t('accounts.confirm') : t('accounts.remove') }}
              </button>
              <button
                v-if="compteASupprimer === selectedUser"
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
        </template>

        <template #footer>
          <div class="admin__field admin__field--actions">
            <button type="button" class="admin__action" @click="editionOuverte = false">
              {{ t('common.cancel') }}
            </button>
            <button
              v-if="peutEcrireComptes"
              type="button"
              class="admin__action admin__action--primary"
              :disabled="comptes.busy.value"
              @click="enregistrerCompte()"
            >
              {{ t('accounts.save') }}
            </button>
          </div>
        </template>
      </UModal>
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
