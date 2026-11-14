<script setup lang="ts">
import { LOGIN_PATH, oidcStartUrl, safeRedirect } from '~/utils/auth'

/**
 * Choix du compte **de développement**.
 *
 * Les droits d'easy3d sont **locaux** : pour essayer « un administrateur », « un
 * imprimeur » ou « un lecteur », il faut autant de sessions. Cette page propose
 * les comptes du royaume Keycloak (`docker/keycloak/realm.json`, lu par
 * `server/api/dev/accounts.get.ts`) et ouvre la session **en un clic** :
 * `POST /api/dev/login` présente le mot de passe du royaume au fournisseur
 * (`grant_type=password`), puis le backend suit le chemin d'un retour de flux —
 * `userinfo`, provisionnement, rattachement, session.
 *
 * Le lien « chez Keycloak » de chaque ligne fait l'inverse : il démarre le vrai
 * flux dans le navigateur, avec `login_hint` pour que le fournisseur pré-remplisse
 * l'identifiant, et le mot de passe se saisit **chez lui**. C'est là qu'on voit
 * la page du fournisseur, ce qu'un simple clic ne montre pas.
 *
 * `layout: false` comme la page de connexion : hors de la coquille du catalogue.
 * Hors développement (`NUXT_PUBLIC_DEV_LOGIN` absent), la page répond 404 ; sans
 * `EASY3D_OIDC_DEV_LOGIN` côté backend, le clic est refusé (`403`) avec un
 * message explicite plutôt qu'un bouton qui ne fait rien.
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

/** Compte en cours de connexion (le bouton de sa ligne tourne, les autres non). */
const enCours = ref('')
/** Refus à afficher dans le flux ; page de développement, donc hors i18n. */
const refus = ref('')
/** État d'authentification partagé : il doit être relu après une connexion. */
const { refresh } = useAuth()

/** Traduit un refus de la connexion directe en une phrase qui dit quoi faire. */
function messageDeRefus(erreur: unknown): string {
  const statut = (erreur as { status?: number })?.status ?? 0
  const corps = String((erreur as { data?: unknown })?.data ?? '')
  if (statut === 403 && corps.includes('dev_login_disabled')) {
    return 'Le backend refuse la connexion directe : démarrez-le avec EASY3D_OIDC_DEV_LOGIN=1 (docker compose le pose déjà).'
  }
  if (statut === 401) {
    return 'Le fournisseur a refusé ce mot de passe — le royaume a-t-il changé ? Passez par « chez Keycloak ».'
  }
  if (statut === 429) return 'Trop de tentatives : patientez un instant.'
  return `Connexion impossible (${statut || 'réseau'}). Passez par « chez Keycloak ».`
}

/** Ouvre la session du compte choisi, puis rejoint la page demandée. */
async function seConnecter(compte: DevAccount) {
  refus.value = ''
  enCours.value = compte.login
  try {
    const { redirect } = await $fetch<{ redirect: string }>('/api/dev/login', {
      method: 'POST',
      body: { login: compte.login, password: compte.password, redirect: target.value },
    })
    // ⚠️ L'état d'authentification est **en cache** côté client (une seule
    // lecture par session, voir `useAuth`) : sans ce rechargement, la garde de
    // route verrait encore « personne » et renverrait sur la page de connexion
    // juste après un clic réussi. Le retour de flux, lui, n'a pas ce problème :
    // il arrive par une navigation complète, donc avec un état neuf.
    await refresh()
    await navigateTo(redirect, { replace: true })
  } catch (erreur) {
    refus.value = messageDeRefus(erreur)
  } finally {
    enCours.value = ''
  }
}
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
        Comptes du royaume <strong>{{ data?.realm || 'easy3d' }}</strong> : un clic ouvre la session
        auprès du fournisseur. Les rôles indiqués sont ceux de Keycloak — les droits de
        l'application se règlent dans <em>Administration → Comptes</em>.
      </p>

      <p v-if="error" class="login__error" role="alert">
        Royaume de développement illisible (voir <code>NUXT_DEV_REALM</code>).
      </p>
      <p v-else-if="refus" class="login__error" role="alert">{{ refus }}</p>

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
            <div class="dev__go">
              <UButton
                size="sm"
                color="neutral"
                variant="soft"
                icon="i-lucide-key-round"
                :loading="enCours === compte.login"
                :disabled="enCours !== '' && enCours !== compte.login"
                @click="seConnecter(compte)"
              >
                Se connecter
              </UButton>
              <a
                class="dev__alt"
                :href="oidcStartUrl(target, compte.login)"
                title="Page de connexion du fournisseur : le mot de passe s'y saisit"
              >
                chez Keycloak
              </a>
            </div>
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
