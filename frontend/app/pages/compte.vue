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
 * hérite des droits du compte.
 */
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
const copied = ref(false)

async function createToken() {
  copied.value = false
  if (await jetons.create(newTokenName.value)) newTokenName.value = ''
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
            <span class="admin__item-label">{{ token.name }}</span>
            <span class="admin__item-where">
              {{ t('account.lastUsed') }} {{ quand(token.last_used_at) }} ·
              {{ t('account.created') }} {{ quand(token.created_at) }}
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
            <button
              type="button"
              class="admin__action admin__action--primary"
              :disabled="jetons.busy.value || !newTokenName.trim()"
              @click="createToken()"
            >
              {{ t('account.create') }}
            </button>
          </div>
        </div>

        <p class="admin__hint">{{ t('account.mcpHint') }}</p>
        <p v-if="error" class="admin__status admin__status--err">{{ error }}</p>
      </section>
    </div>
  </div>
</template>
