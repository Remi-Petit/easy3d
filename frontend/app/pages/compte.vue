<script setup lang="ts">
/**
 * Mon compte (`/compte`) : identité, mot de passe, et **jetons d'API**.
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
import { TOKEN_DEFAULT_DAYS, TOKEN_DURATIONS, durationKey, expiryKind } from '~/utils/tokens'
const { t, te } = useI18n()
const { status, ready, user } = useAuth()
const jetons = useTokens()
onMounted(() => {
  if (status.value.enabled) void jetons.refresh()
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

// ── Mot de passe ─────────────────────────────────────────────────────────
const password = reactive({ current: '', next: '', again: '' })
const passwordState = ref<'idle' | 'saving' | 'ok' | 'error'>('idle')
const passwordError = ref('')

/** Le formulaire est complet et les deux saisies concordent. */
const passwordReady = computed(
  () =>
    password.current.length > 0 &&
    password.next.length >= 8 &&
    password.next === password.again,
)

async function changePassword() {
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

/** Horloge partagée : les mentions « expire bientôt » se rafraîchissent seules. */
const maintenant = useNow()

async function createToken() {
  copied.value = false
  if (await jetons.create(newTokenName.value, newTokenDays.value)) newTokenName.value = ''
}

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
  <div class="admin">
    <div class="admin__col">
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
            :placeholder="t('account.currentPassword')"
            :aria-label="t('account.currentPassword')"
          />
          <input
            v-model="password.next"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :placeholder="t('account.newPassword')"
            :aria-label="t('account.newPassword')"
          />
          <input
            v-model="password.again"
            type="password"
            class="admin__input"
            autocomplete="new-password"
            :placeholder="t('account.repeatPassword')"
            :aria-label="t('account.repeatPassword')"
          />
          <div class="admin__field--actions">
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="!passwordReady || passwordState === 'saving'"
              @click="changePassword()"
            >
              {{ t('account.change') }}
            </button>
          </div>

          <p v-if="password.next && password.next !== password.again" class="admin__hint">
            {{ t('account.mismatch') }}
          </p>
          <p v-if="passwordState === 'ok'" class="admin__status admin__status--ok">
            {{ t('account.changed') }}
          </p>
          <p v-if="passwordState === 'error'" class="admin__status admin__status--err">
            {{ passwordError }}
          </p>
          <p class="admin__hint">{{ t('account.passwordHint') }}</p>
        </div>
      </section>
    </div>

    <div class="admin__col">
      <section class="admin__card">
        <h2 class="admin__title">
          {{ t('account.tokens') }}
          <span class="admin__count">{{ jetons.tokens.value.length }}</span>
        </h2>
        <p class="admin__count">{{ t('account.tokensHint') }}</p>

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

        <div class="admin__field">
          <span class="admin__label">{{ t('account.newToken') }}</span>
          <div class="admin__row">
            <input
              v-model="newTokenName"
              class="admin__input"
              :placeholder="t('account.tokenName')"
              :aria-label="t('account.tokenName')"
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
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="jetons.busy.value || !newTokenName.trim()"
              @click="createToken()"
            >
              {{ t('account.create') }}
            </button>
          </div>
          <p class="admin__hint">{{ t('account.durationHint') }}</p>
        </div>

        <p class="admin__hint">{{ t('account.mcpHint') }}</p>
        <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
      </section>
    </div>
  </div>
</template>
