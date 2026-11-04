<script setup lang="ts">
import { LOGIN_PATH, oidcStartUrl, safeRedirect } from '~/utils/auth'

/**
 * Choix du compte **de développement**.
 *
 * Les droits d'easy3d sont **locaux** : pour essayer « un administrateur », « un
 * imprimeur » ou « un lecteur », il faut autant de sessions. Cette page propose
 * les comptes du royaume Keycloak (`docker/keycloak/realm.json`, lu par
 * `server/api/dev/accounts.get.ts`) et démarre pour chacun le **vrai** flux SSO,
 * avec `login_hint` pour que le fournisseur pré-remplisse l'identifiant.
 *
 * Ce qu'elle ne fait pas : connecter quelqu'un. Le mot de passe est affiché
 * parce qu'il faut le saisir **chez Keycloak** ; aucune session n'est ouverte
 * ici, et le backend ne connaît même pas cette page.
 *
 * `layout: false` comme la page de connexion : hors de la coquille du catalogue.
 * Hors développement (`NUXT_PUBLIC_DEV_LOGIN` absent), la page répond 404.
 */
definePageMeta({ layout: false })

interface DevAccount {
  login: string
  email: string
  password: string
  roles: string[]
}

const route = useRoute()
const config = useRuntimeConfig()

if (!config.public.devLogin) {
  throw createError({ statusCode: 404, statusMessage: 'Not Found', fatal: true })
}

/** Page à rejoindre après la connexion (chemin interne ; le backend revalide). */
const target = computed(() => safeRedirect(route.query.redirect))

const { data, error } = await useFetch<{ realm: string; accounts: DevAccount[] }>(
  '/api/dev/accounts',
)
</script>

<template>
  <div class="login">
    <div class="login__card login__card--wide">
      <div class="brand">
        <div class="logo">3D</div>
        <div>
          <p class="brand__name">easy3d</p>
          <small>Développement</small>
        </div>
      </div>

      <h1 class="login__title">Choisir un compte</h1>
      <p class="login__hint">
        Comptes du royaume <strong>{{ data?.realm || 'easy3d' }}</strong> : le mot de passe est à
        saisir chez le fournisseur, la connexion n'est pas contournée. Les rôles indiqués sont ceux
        de Keycloak — les droits de l'application se règlent dans <em>Administration → Comptes</em>.
      </p>

      <p v-if="error" class="login__error" role="alert">
        Royaume de développement illisible (voir <code>NUXT_DEV_REALM</code>).
      </p>

      <ul v-else class="dev__list">
        <li
          v-for="compte in data?.accounts ?? []"
          :key="compte.login"
          class="dev__account"
          :data-compte="compte.login"
        >
          <div class="dev__who">
            <strong>{{ compte.login }}</strong>
            <small>{{ compte.email }}</small>
            <small class="dev__roles">
              <span v-for="role in compte.roles" :key="role" class="dev__role">{{ role }}</span>
              <span v-if="compte.roles.length === 0" class="dev__role dev__role--none">sans rôle</span>
            </small>
          </div>
          <div class="dev__actions">
            <code class="dev__password" :title="`Mot de passe Keycloak de ${compte.login}`">
              {{ compte.password }}
            </code>
            <UButton
              size="sm"
              color="neutral"
              variant="soft"
              icon="i-lucide-key-round"
              :to="oidcStartUrl(target, compte.login)"
              external
            >
              Se connecter
            </UButton>
          </div>
        </li>
      </ul>

      <p v-if="!error && (data?.accounts ?? []).length === 0" class="login__hint">
        Aucun compte déclaré dans le royaume.
      </p>

      <NuxtLink :to="LOGIN_PATH" class="dev__back">
        Revenir à la connexion par mot de passe
      </NuxtLink>
    </div>
  </div>
</template>
