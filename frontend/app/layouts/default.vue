<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'

// Layout global : coquille « dashboard » fournie par Nuxt UI (sidebar +
// panneau), habillée avec les tokens de l'app (voir `main.css`).
//
// La sidebar porte l'identité de l'app, la navigation et l'état du backend.
// Le panneau porte le titre de la **page courante** (déduit de la route, donc
// identique côté serveur et client) et la barre de filtres, collée en haut.
const route = useRoute()
const header = usePageHeaderState()

// Barre de filtre globale (recherche, types, tri), partagée avec les pages.
const { query, sortMode, types, availableTypes, hasTypeFilter, sortLabel, sortIcon, cycleSort, toggle, clearTypes } =
  useFilter()

/** Titre de la page courante, déduit de la route (pas de flash à l'hydratation). */
const title = computed(() => {
  if (route.params.name) return decodeURIComponent(String(route.params.name))
  if (route.params.rel) return basename(decodeURIComponent(String(route.params.rel)))
  return 'easy3d'
})

const isHome = computed(() => route.path === '/')
/** La vue fichier n'a pas de liste à filtrer. */
const showFilter = computed(() => !route.params.rel)
const placeholder = computed(() =>
  route.params.name ? 'Filtrer par nom de fichier…' : 'Filtrer par nom de fichier ou de dossier…',
)

// "connecté" = WS live (temps réel) OU données qui remontent (polling sans erreur).
const connected = computed(() => header.value.live || !header.value.offline)
const statusLabel = computed(() =>
  header.value.live ? 'temps réel' : header.value.offline ? 'hors ligne' : 'repli polling',
)

/** Routes rattachées à une section dont l'URL ne porte pas le préfixe. */
const SECTION_ROUTES: Record<string, string[]> = {
  '/': ['/dossiers', '/fichier'],
}

/** Une entrée est active si la route courante appartient à sa section. */
function isActive(to: string) {
  return route.path === to || (SECTION_ROUTES[to] ?? []).some((p) => route.path.startsWith(p))
}

/**
 * Navigation : une seule section pour l'instant (le catalogue) ; ajouter une
 * page = ajouter une entrée ici.
 */
const navItems = computed<NavigationMenuItem[]>(() => [
  { label: 'Models', icon: 'i-lucide-box', to: '/', active: isActive('/') },
])
</script>

<template>
  <!--
    Coquille Nuxt UI : `UDashboardGroup` gère la mise en page (sidebar à
    gauche + panneau de contenu) et bascule la sidebar en tiroir sous `lg`.
  -->
  <UDashboardGroup>
    <!--
      `resizable` = poignée de redimensionnement (offert par Nuxt UI).
      `collapsible` est volontairement absent : sur desktop le bouton de repli
      (UDashboardSidebarToggle) est `lg:hidden`, on se retrouverait avec une
      sidebar masquée sans bouton évident pour la rouvrir.
    -->
    <UDashboardSidebar
      id="easy3d"
      :default-size="18"
      :min-size="14"
      :max-size="24"
      resizable
      :menu="{ title: 'Navigation', description: 'Navigation de l’application' }"
      :ui="{ root: 'bg-elevated' }"
    >
      <!-- Identité de l'app. -->
      <template #header>
        <div class="brand">
          <div class="logo">3D</div>
          <div>
            <p class="brand__name">easy3d</p>
            <small>catalogue de modèles</small>
          </div>
        </div>
      </template>

      <!-- Navigation (une seule section pour l'instant). -->
      <UNavigationMenu :items="navItems" orientation="vertical" />

      <!-- État du backend. -->
      <template #footer>
        <div class="stats">
          <span class="pill">
            <span class="dot" :class="connected ? 'live' : 'err'" /> {{ statusLabel }}
          </span>
          <span v-if="isHome" class="pill"><b>{{ header.count }}</b> fichiers</span>
        </div>
      </template>
    </UDashboardSidebar>

    <UDashboardPanel id="catalog" :ui="{ body: 'p-0 sm:p-0 gap-0' }">
      <!-- Titre de la page courante + son sous-titre. -->
      <template #header>
        <UDashboardNavbar>
          <template #title>{{ title }}</template>
          <template #trailing>
            <small class="navbar__subtitle">{{ header.subtitle }}</small>
          </template>
        </UDashboardNavbar>
      </template>

      <template #body>
        <!-- Barre de filtre globale : recherche + tri, puis types. Collée en
             haut de la zone de défilement du panneau. -->
        <div v-if="showFilter" class="filters">
          <div class="toolbar">
            <div class="search">
              <span class="icon">🔍</span>
              <input v-model="query" type="text" :placeholder="placeholder" />
            </div>
            <button
              type="button"
              class="sort"
              :class="`sort--${sortMode}`"
              :title="`Trier par ${sortLabel} (cliquer pour changer)`"
              :aria-label="`Trier par ${sortLabel}`"
              @click="cycleSort"
            >
              <span class="sort__icon">{{ sortIcon }}</span>
              <span class="sort__label">Date</span>
            </button>
          </div>

          <!-- Filtre par type : formats détectés dans la vue courante. -->
          <div v-if="availableTypes.length > 1" class="types">
            <button
              v-for="entry in availableTypes"
              :key="entry.type"
              type="button"
              class="types__item"
              :class="{ 'types__item--on': types.includes(entry.type) }"
              :aria-pressed="types.includes(entry.type)"
              :title="`Filtrer : ${entry.count} fichier(s) ${entry.type.toUpperCase()}`"
              @click="toggle(entry.type)"
            >
              {{ entry.type.toUpperCase() }} <b>{{ entry.count }}</b>
            </button>
            <button v-if="hasTypeFilter" type="button" class="types__clear" @click="clearTypes()">
              ✕ Effacer
            </button>
          </div>
        </div>

        <div class="page">
          <slot />
        </div>
      </template>
    </UDashboardPanel>
  </UDashboardGroup>
</template>
