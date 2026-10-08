<script setup lang="ts">
/**
 * Mon compte (`/account`) : identité, mot de passe, et **jetons d'API**.
 *
 * Deux URL pour une même page : `/account` montre l'identité et le mot de passe,
 * `/account/api` les jetons (voir la section « Onglets »).
 *
 * Page réservée au compte connecté — la garde `auth.global.ts` a déjà renvoyé
 * vers la connexion qui n'en a pas. Sans gestion des comptes (installation
 * ouverte), elle n'a rien à montrer : on repart au catalogue.
 *
 * Les jetons servent aux **agents** (Claude Code, un script, une CI) qui n'ont
 * pas de navigateur : ils présentent le jeton en `Authorization: Bearer`, et il
 * hérite des droits du compte — avec une durée de vie au choix, pour qu'un jeton
 * oublié finisse par mourir tout seul.
 */
import type { PermissionInfo } from '~/utils/permissions'
import { TOKEN_DEFAULT_DAYS, TOKEN_DURATIONS, durationKey, expiryKind } from '~/utils/tokens'
const { t, te } = useI18n()
const { status, ready, user } = useAuth()
const route = useRoute()
const jetons = useTokens()
onMounted(() => {
  if (!status.value.enabled) return
  void jetons.refresh()
  // Le catalogue sert au sélecteur de droits ; sans lui, la restriction n'est
  // pas proposée du tout (voir `catalogueMesDroits`), plutôt que d'afficher une
  // liste vide qu'on ne comprendrait pas.
  void $fetch<PermissionInfo[]>('/api/permissions')
    .then((liste) => {
      catalogue.value = liste
    })
    .catch(() => {})
})

// Sans comptes, cette page n'existe pas : on renvoie au catalogue plutôt que
// d'afficher un écran vide.
watchEffect(() => {
  if (ready.value && !status.value.enabled) void navigateTo('/')
})

/** Message de la dernière erreur, traduit depuis son code. */
const error = computed(() => {
  const code = jetons.errorCode.value
  if (!code) return ''
  const key = `account.errors.${code}`
  return te(key) ? t(key) : t('account.errors.unknown')
})

// ── Onglets ──────────────────────────────────────────────────────────────
/**
 * Trois vues, trois URL : `/account` (identité, mot de passe), `/account/api`
 * (jetons) et `/account/mcp` (brancher un agent). L'onglet **découle de l'URL** :
 * un lien vers `/account/mcp` ouvre directement la bonne vue, et le bouton
 * « retour » du navigateur refait le trajet inverse. Le contenu des onglets
 * masqués n'est pas monté (`v-if`/`v-else`).
 */
type Onglet = 'compte' | 'api' | 'mcp'

/** Onglets, dans l'ordre d'affichage — sert aussi à la navigation clavier. */
const ONGLETS: Onglet[] = ['compte', 'api', 'mcp']

/** Segment de chaque onglet : « compte » est la racine de la page. */
const chemins: Record<Onglet, string> = {
  compte: '/account',
  api: '/account/api',
  mcp: '/account/mcp',
}

/** L'onglet porté par l'URL, ou « compte » pour tout segment inconnu. */
const onglet = computed<Onglet>(
  () => ONGLETS.find((nom) => nom === route.params.tab) ?? 'compte',
)

/**
 * Onglets, dans l'ordre d'affichage. La barre elle-même est `PanelTabs` : elle
 * porte le clavier et la sémantique `tablist`, la même que sur les comptes.
 */
const onglets = computed(() => [
  { id: 'compte', label: t('account.tabAccount') },
  { id: 'api', label: t('account.tabApi') },
  { id: 'mcp', label: t('account.mcp') },
])

/** Changer d'onglet, c'est changer d'URL : c'est elle qui fait foi. */
function ouvreOnglet(id: string) {
  const cible = ONGLETS.find((nom) => nom === id)
  if (cible && cible !== onglet.value) void navigateTo(chemins[cible])
}

/**
 * Un segment inconnu (`/account/xyz`) n'est pas une vue : on revient à l'onglet
 * par défaut, sans laisser l'adresse fautive dans l'historique.
 */
watchEffect(() => {
  const segment = route.params.tab
  if (segment !== undefined && segment !== '' && !ONGLETS.some((nom) => nom === segment)) {
    void navigateTo(chemins.compte, { replace: true })
  }
})

// ── Mot de passe ─────────────────────────────────────────────────────────
const password = reactive({ current: '', next: '', again: '' })
const passwordState = ref<'idle' | 'saving' | 'ok' | 'error'>('idle')
const passwordError = ref('')

/**
 * Compte rattaché au fournisseur d'identité : le mot de passe se gère **là-bas**.
 *
 * Le formulaire reste affiché (on voit de quoi il s'agit) mais grisé, avec
 * l'explication au survol et en clair : le serveur refuse de toute façon
 * (`password_sso`), et c'est voulu — un mot de passe local continuerait de
 * fonctionner après une désactivation chez le fournisseur.
 *
 * **Sauf pour un administrateur** : son mot de passe est la porte de secours de
 * l'installation si le fournisseur est en panne. Le serveur applique la même
 * règle.
 */
const compteSso = computed(
  () => user.value?.oidc === true && !(user.value?.roles ?? []).includes('admin'),
)

/** Le formulaire est complet et les deux saisies concordent. */
const passwordReady = computed(
  () =>
    password.current.length > 0 &&
    password.next.length >= 8 &&
    password.next === password.again,
)

async function changePassword() {
  // Rien à envoyer : ce compte n'a pas de mot de passe ici, le serveur le
  // refuserait (`password_sso`).
  if (compteSso.value) return
  passwordState.value = 'saving'
  passwordError.value = ''
  try {
    await $fetch('/api/auth/password', {
      method: 'POST',
      body: { current: password.current, new: password.next },
    })
    password.current = ''
    password.next = ''
    password.again = ''
    passwordState.value = 'ok'
  } catch (e: unknown) {
    const data = (e as { data?: unknown })?.data
    const code = typeof data === 'string' && data.trim() ? data.trim() : 'unknown'
    const key = `account.errors.${code}`
    passwordError.value = te(key) ? t(key) : t('account.errors.unknown')
    passwordState.value = 'error'
  }
}

// ── Jetons ───────────────────────────────────────────────────────────────
const newTokenName = ref('')
// Durée de vie choisie (0 = sans expiration) : le défaut est le plus sûr, un
// agent qui s'arrête tout seul sans qu'on l'ait demandé serait une surprise.
const newTokenDays = ref<number>(TOKEN_DEFAULT_DAYS)
const copied = ref(false)

// ── Portée du jeton (facultative) ────────────────────────────────────────
/**
 * Catalogue des droits publié par le backend, et ceux que **ce compte**
 * détient : on ne propose que ces derniers — on n'accorde pas à un jeton un
 * droit qu'on n'a pas (le backend refuse, de toute façon).
 */
const catalogue = ref<PermissionInfo[]>([])
const mesDroits = computed(() => new Set(user.value?.permissions ?? []))
const catalogueMesDroits = computed(() =>
  catalogue.value.filter((droit) => mesDroits.value.has(droit.id)),
)

/** Restriction active, et droits cochés — tout est coché à l'activation. */
const restreindre = ref(false)
const droitsJeton = ref<string[]>([])

function basculeRestriction() {
  droitsJeton.value = restreindre.value ? [...mesDroits.value] : []
}

/**
 * Création possible : un nom est **obligatoire** (le backend refuse un jeton
 * sans nom), une portée vide n'aurait pas de sens, et pas deux créations en même
 * temps. La touche Entrée suit la même règle que le bouton — sinon elle
 * déclencherait un refus pour rien.
 */
const peutCreer = computed(
  () =>
    !jetons.busy.value &&
    !!newTokenName.value.trim() &&
    (!restreindre.value || droitsJeton.value.length > 0),
)

/** Horloge partagée : les mentions « expire bientôt » se rafraîchissent seules. */
const maintenant = useNow()

async function createToken() {
  copied.value = false
  const portee = restreindre.value ? droitsJeton.value : null
  if (!(await jetons.create(newTokenName.value, newTokenDays.value, portee))) return
  newTokenName.value = ''
  // La restriction est un réglage **du jeton créé** : on revient au défaut,
  // sinon le suivant hériterait d'un choix qu'on n'a pas refait.
  restreindre.value = false
  droitsJeton.value = []
}

// ── Onglet MCP ───────────────────────────────────────────────────────────
/** Adresse du serveur MCP, telle qu'on la montre (voir `~/utils/mcp`). */
const adresseMcp = computed(() => mcpUrl(useRuntimeConfig().public, useRequestURL()))

const copieAdresse = ref(false)

/** Copie l'adresse (le presse-papiers peut être refusé : on l'ignore). */
async function copierAdresse() {
  try {
    await navigator.clipboard.writeText(adresseMcp.value)
    copieAdresse.value = true
  } catch {
    copieAdresse.value = false
  }
}

/**
 * Extraits à recopier, avec l'adresse **réelle** et un jeton d'exemple : c'est
 * tout ce qu'il faut pour brancher un agent, et le seul endroit où un
 * utilisateur trouverait cette adresse.
 */
const extraitVscode = computed(
  () => `{
  "servers": {
    "easy3d": {
      "type": "http",
      "url": "${adresseMcp.value}",
      "headers": { "Authorization": "Bearer e3d_…" }
    }
  }
}`,
)

const extraitClaude = computed(() =>
  [
    `claude mcp add --transport http easy3d ${adresseMcp.value} \\`,
    '  --header "Authorization: Bearer e3d_…"',
  ].join('\n'),
)

/** Libellé d'une durée proposée, avec repli sur le libellé générique. */
function durationLabel(days: number): string {
  const key = durationKey(days)
  return te(key) ? t(key) : t('account.daysMany', { count: days })
}

/**
 * Ce qu'on dit de la date de fin d'un jeton.
 *
 * `null` (sans expiration) et « expiré » ont chacun leur libellé : la liste doit
 * dire ce qu'il en est sans qu'on ait à comparer des dates de tête.
 */
function expiryLabel(expiresAt: number | null): string {
  const kind = expiryKind(expiresAt, Math.floor(maintenant.value.getTime() / 1000))
  if (kind === 'never') return t('account.neverExpires')
  if (kind === 'expired') return t('account.expired')
  return t('account.expiresOn', { date: quand(expiresAt) })
}

/** Mise en avant des jetons qui vont s'arrêter (ou qui le sont déjà). */
function expiryClass(expiresAt: number | null): string {
  const kind = expiryKind(expiresAt, Math.floor(maintenant.value.getTime() / 1000))
  if (kind === 'expired') return 'accounts__tag'
  return kind === 'soon' ? 'accounts__tag accounts__tag--on' : ''
}

/** Copie le jeton affiché (le presse-papiers peut être refusé : on l'ignore). */
async function copyToken() {
  const token = jetons.created.value?.token
  if (!token) return
  try {
    await navigator.clipboard.writeText(token)
    copied.value = true
  } catch {
    copied.value = false
  }
}

/** Révocation en deux temps : le second clic confirme. */
const aRevoquer = ref<string | null>(null)

const quand = (valeur: number | null) =>
  valeur ? new Date(valeur * 1000).toLocaleDateString() : t('account.neverUsed')
</script>

<template>
  <div class="tabbed-page">
    <!--
      Trois onglets, trois URL : `/account` (identité, mot de passe),
      `/account/api` (jetons) et `/account/mcp` (brancher un agent). La barre est
      partagée avec la page des comptes (voir `PanelTabs`) : les flèches
      gauche/droite passent d'un onglet à l'autre, et l'URL suit.
    -->
    <PanelTabs :tabs="onglets" :active="onglet" :label="t('nav.account')" @select="ouvreOnglet" />

    <!-- Compte : identité puis mot de passe. -->
    <div
      v-if="onglet === 'compte'"
      id="panneau-compte"
      role="tabpanel"
      aria-labelledby="onglet-compte"
      class="admin__col"
    >
      <section class="admin__card">
        <h2 class="admin__title">{{ t('account.identity') }}</h2>
        <dl class="admin__applied">
          <div>
            <dt>{{ t('account.userName') }}</dt>
            <dd>{{ user?.username }}</dd>
          </div>
          <div>
            <dt>{{ t('account.email') }}</dt>
            <dd>{{ user?.email }}</dd>
          </div>
          <div>
            <dt>{{ t('account.roles') }}</dt>
            <dd>{{ user?.roles?.join(', ') || t('account.noRole') }}</dd>
          </div>
        </dl>
      </section>

      <section class="admin__card">
        <h2 class="admin__title">{{ t('account.password') }}</h2>
        <div class="admin__field">
          <input
            v-model="password.current"
            type="password"
            class="admin__input"
            autocomplete="current-password"
            :disabled="compteSso"
            :placeholder="t('account.currentPassword')"
            :aria-label="t('account.currentPassword')"
          />
          <input
            v-model="password.next"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :disabled="compteSso"
            :placeholder="t('account.newPassword')"
            :aria-label="t('account.newPassword')"
          />
          <input
            v-model="password.again"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :disabled="compteSso"
            :placeholder="t('account.repeatPassword')"
            :aria-label="t('account.repeatPassword')"
          />
          <div class="admin__field--actions">
            <!--
              Un bouton désactivé n'annonce pas toujours le survol : c'est le
              porteur de l'infobulle qui le reçoit (même procédé que le bouton
              ✦ de la recherche IA).
            -->
            <span class="tip">
              <button
                type="button"
                class="admin__action admin__action--primary"
                :disabled="compteSso || !passwordReady || passwordState === 'saving'"
                :aria-describedby="compteSso ? 'mot-de-passe-sso' : undefined"
                @click="changePassword()"
              >
                {{ t('account.change') }}
              </button>
              <span v-if="compteSso" id="mot-de-passe-sso" role="tooltip" class="tip__bubble">
                {{ t('account.passwordSso') }}
              </span>
            </span>
          </div>

          <p v-if="!compteSso && password.next && password.next !== password.again" class="admin__hint">
            {{ t('account.mismatch') }}
          </p>
          <p v-if="passwordState === 'ok'" class="admin__status admin__status--ok">
            {{ t('account.changed') }}
          </p>
          <p v-if="passwordState === 'error'" class="admin__status admin__status--err">
            {{ passwordError }}
          </p>
          <p class="admin__hint">
            {{ compteSso ? t('account.passwordSso') : t('account.passwordHint') }}
          </p>
        </div>
      </section>
    </div>

    <div
      v-else-if="onglet === 'api'"
      id="panneau-api"
      role="tabpanel"
      aria-labelledby="onglet-api"
      class="admin__col"
    >
      <!-- API : les jetons, qui servent aux agents sans navigateur. -->
      <section class="admin__card">
        <h2 class="admin__title">
          {{ t('account.tokens') }}
          <span class="admin__count">{{ jetons.tokens.value.length }}</span>
        </h2>

        <!-- Le jeton en clair : montré **une fois**, avec de quoi le copier. -->
        <div v-if="jetons.created.value" class="admin__field token__new">
          <span class="admin__label">{{ t('account.tokenOnce') }}</span>
          <code class="token__value">{{ jetons.created.value.token }}</code>
          <div class="admin__field--actions">
            <button type="button" class="admin__action" @click="copyToken()">
              {{ copied ? t('account.copied') : t('account.copy') }}
            </button>
            <button type="button" class="admin__action" @click="jetons.forget()">
              {{ t('account.done') }}
            </button>
          </div>
        </div>

        <ul v-if="jetons.tokens.value.length" class="admin__list">
          <li v-for="token in jetons.tokens.value" :key="token.uuid" class="admin__item">
            <span class="admin__item-label">
              {{ token.name }}
              <!-- Un jeton qui va s'arrêter (ou qui l'est déjà) se voit tout de
                   suite : c'est ce qui évite de chercher pourquoi un agent a
                   cessé de fonctionner. -->
              <span v-if="expiryClass(token.expires_at)" :class="expiryClass(token.expires_at)">
                {{ expiryLabel(token.expires_at) }}
              </span>
              <!-- Jeton restreint : on dit à quoi il se limite, sinon il
                   faudrait le deviner. -->
              <span v-if="token.permissions" class="accounts__tag">
                {{ t('account.rightsCount', { count: token.permissions.length }) }}
              </span>
            </span>
            <span class="admin__item-where">
              {{ t('account.lastUsed') }} {{ quand(token.last_used_at) }} ·
              {{ t('account.created') }} {{ quand(token.created_at) }} ·
              {{ expiryLabel(token.expires_at) }}
            </span>
            <div class="admin__field--actions">
              <button
                type="button"
                class="admin__action"
                :disabled="jetons.busy.value"
                @click="aRevoquer = aRevoquer === token.uuid ? null : token.uuid"
              >
                {{ aRevoquer === token.uuid ? t('account.confirm') : t('account.revoke') }}
              </button>
              <button
                v-if="aRevoquer === token.uuid"
                type="button"
                class="admin__action admin__action--danger"
                :disabled="jetons.busy.value"
                @click="jetons.revoke(token.uuid)"
              >
                {{ t('account.revoke') }}
              </button>
            </div>
          </li>
        </ul>
        <p v-else class="admin__empty">{{ t('account.noToken') }}</p>

        <div class="admin__field token__form">
          <span class="admin__label">{{ t('account.newToken') }}</span>
          <div class="admin__row">
            <input
              v-model="newTokenName"
              class="admin__input"
              :placeholder="t('account.tokenName')"
              :aria-label="t('account.tokenName')"
              @keydown.enter.prevent="peutCreer && createToken()"
            />
            <!--
              Durée de vie : `0` (en tête, et par défaut) veut dire « sans
              expiration ». Le reste est proposé en jours — un agent qui s'arrête
              tout seul est plus facile à comprendre quand on l'a choisi.
            -->
            <select
              v-model.number="newTokenDays"
              class="admin__input admin__input--short"
              :aria-label="t('account.duration')"
            >
              <option v-for="days in TOKEN_DURATIONS" :key="days" :value="days">
                {{ durationLabel(days) }}
              </option>
            </select>
          </div>
          <p class="admin__hint">{{ t('account.durationHint') }}</p>

          <!--
            Restriction des droits : facultative, et repliée tant qu'on n'y
            touche pas — la plupart des jetons prennent tous les droits du
            compte. On ne propose que les droits **du compte** : on n'accorde pas
            ce qu'on ne détient pas (le backend refuse aussi, de son côté).
          -->
          <template v-if="catalogueMesDroits.length">
            <label class="admin__choice" :class="{ 'admin__choice--on': restreindre }">
              <input v-model="restreindre" type="checkbox" @change="basculeRestriction" />
              <span>{{ t('account.limitRights') }}</span>
            </label>
            <PermissionPicker
              v-if="restreindre"
              v-model="droitsJeton"
              :permissions="catalogueMesDroits"
            />
            <p class="admin__hint">{{ t('account.limitRightsHint') }}</p>
          </template>

          <div class="admin__field--actions">
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="!peutCreer"
              @click="createToken()"
            >
              {{ t('account.create') }}
            </button>
          </div>
        </div>

        <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
      </section>
    </div>

    <!-- MCP : de quoi brancher un agent (Claude Code, VS Code, un script). -->
    <div
      v-else
      id="panneau-mcp"
      role="tabpanel"
      aria-labelledby="onglet-mcp"
      class="admin__col"
    >
      <section class="admin__card">
        <h2 class="admin__title">{{ t('account.mcp') }}</h2>
        <p class="admin__hint">{{ t('account.mcpIntro') }}</p>

        <div class="admin__field">
          <span class="admin__label">{{ t('account.mcpAddress') }}</span>
          <div class="mcp__ligne">
            <code class="mcp__url">{{ adresseMcp }}</code>
            <button type="button" class="admin__action" @click="copierAdresse()">
              {{ copieAdresse ? t('account.copied') : t('account.copy') }}
            </button>
          </div>
          <p class="admin__hint">{{ t('account.mcpAddressHint') }}</p>
        </div>
      </section>

      <section class="admin__card">
        <h2 class="admin__title">{{ t('account.mcpClients') }}</h2>

        <div class="admin__field">
          <span class="admin__label">{{ t('account.mcpVscode') }}</span>
          <pre class="mcp__code"><code>{{ extraitVscode }}</code></pre>
        </div>

        <div class="admin__field">
          <span class="admin__label">{{ t('account.mcpClaude') }}</span>
          <pre class="mcp__code"><code>{{ extraitClaude }}</code></pre>
        </div>

        <p class="admin__hint">{{ t('account.mcpTokenHint') }}</p>
        <div class="admin__field--actions">
          <button type="button" class="admin__action" @click="ouvreOnglet('api')">
            {{ t('account.mcpGetToken') }}
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
