<script setup lang="ts">
import { DEV_SSO_LABEL, devLoginUrl, oidcStartUrl, safeRedirect, authErrorKey } from '~/utils/auth'

/**
 * Page de connexion.
 *
 * `layout: false` : elle se tient hors de la coquille du catalogue — ni sidebar,
 * ni barre de recherche, rien de tout ça n'ayant de sens avant d'être identifié.
 * `UApp` reste au-dessus (voir `app.vue`), donc les composants Nuxt UI et la
 * langue choisie fonctionnent normalement.
 */
definePageMeta({ layout: false })

const route = useRoute()
const { t } = useI18n()
const { status, ready, busy, errorCode, oidc, login } = useAuth()

const form = reactive({ login: '', password: '' })

/**
 * Retour du fournisseur d'identité : le backend redirige ici avec un code
 * (`?error=oidc_not_provisioned`), que l'on traduit comme les autres.
 *
 * Le paramètre est **consommé une fois** : recharger la page ne doit pas
 * réafficher indéfiniment l'échec d'une connexion abandonnée.
 */
const retourSso = ref<string | null>(null)
if (typeof route.query.error === 'string' && route.query.error) {
  retourSso.value = route.query.error
}

// Le code du backend devient un message ici, dans le `setup` de la page : la
// traduction n'a pas sa place dans le composable (voir `useAuth`).
const error = computed(() => {
  const code = errorCode.value ?? retourSso.value
  return code ? t(authErrorKey(code)) : ''
})

/** Page demandée avant la connexion, à rejoindre dans les deux cas. */
const target = computed(() => safeRedirect(route.query.redirect))

/**
 * Option de **développement** : quand `NUXT_PUBLIC_DEV_LOGIN=1`, le bouton ne
 * saute pas directement chez le fournisseur — il passe par `/dev/login`, qui
 * propose les comptes du royaume Keycloak (et repart ensuite en vrai flux SSO).
 * Il s'appelle alors « keycloak-dev », le nom du fournisseur de dev, plutôt que
 * l'hôte annoncé par le backend.
 */
const devLogin = useRuntimeConfig().public.devLogin
const ssoUrl = computed(() => (devLogin ? devLoginUrl(target.value) : oidcStartUrl(target.value)))
const ssoLabel = computed(() => (devLogin ? DEV_SSO_LABEL : (oidc.value?.label ?? '')))

// Sans authentification activée, cette page n'a rien à proposer : la garde
// laisse passer (elle ne s'applique qu'aux installations avec comptes), donc
// c'est ici que l'on renvoie au catalogue.
watchEffect(() => {
  if (ready.value && !status.value.enabled) void navigateTo('/')
})

async function submit() {
  if (await login(form.login, form.password)) {
    // `redirect` vient d'un lien avec garde : on repart où l'on voulait aller.
    await navigateTo(target.value)
  }
}
</script>

<template>
  <div class="login">
    <form class="login__card" @submit.prevent="submit">
      <div class="brand">
        <div class="logo">3D</div>
        <div>
          <p class="brand__name">easy3d</p>
          <small>{{ t('app.tagline') }}</small>
        </div>
      </div>

      <h1 class="login__title">{{ t('auth.title') }}</h1>
      <p class="login__hint">{{ t('auth.hint') }}</p>

      <UFormField :label="t('auth.login')">
        <UInput
          v-model="form.login"
          class="w-full"
          autocomplete="username"
          autofocus
          :disabled="busy"
        />
      </UFormField>

      <UFormField :label="t('auth.password')">
        <UInput
          v-model="form.password"
          type="password"
          class="w-full"
          autocomplete="current-password"
          :disabled="busy"
        />
      </UFormField>

      <!-- Le message vient du **code** renvoyé par le backend, traduit ici : le
           serveur ne connaît pas la langue de l'interface. -->
      <p v-if="error" class="login__error" role="alert">{{ error }}</p>

      <UButton
        type="submit"
        block
        :loading="busy"
        :disabled="busy || !form.login.trim() || !form.password"
      >
        {{ t('auth.submit') }}
      </UButton>

      <!--
        Connexion par le fournisseur d'identité de l'organisation, quand le
        serveur en annonce un. C'est un **lien** et non un bouton de
        formulaire : le flux commence par une navigation complète (le backend
        répond un 302 vers le fournisseur), pas par un appel `fetch`.
      -->
      <template v-if="oidc">
        <p class="login__sep"><span>{{ t('auth.or') }}</span></p>
        <UButton
          :to="ssoUrl"
          external
          color="neutral"
          variant="soft"
          block
          icon="i-lucide-key-round"
        >
          {{ t('auth.sso', { provider: ssoLabel }) }}
        </UButton>
        <!--
          Fournisseurs **supplémentaires** (`EASY3D_OIDC_PROVIDERS`) : un bouton
          chacun, à côté du principal. C'est ce qui permet d'avoir en
          développement un vrai Keycloak *et* le fournisseur fictif sans choisir.
        -->
        <UButton
          v-for="extra in oidc.providers ?? []"
          :key="extra.id"
          :to="oidcStartUrl(target, null, extra.id)"
          external
          color="neutral"
          variant="soft"
          block
          icon="i-lucide-key-round"
        >
          {{ t('auth.sso', { provider: extra.label }) }}
        </UButton>
      </template>
    </form>
  </div>
</template>
