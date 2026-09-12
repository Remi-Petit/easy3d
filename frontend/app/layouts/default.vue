<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'

// Layout global : coquille « dashboard » fournie par Nuxt UI (sidebar +
// panneau), habillée avec les tokens de l'app (voir `main.css`).
//
// La sidebar porte l'identité de l'app, la navigation et l'état du backend.
// Le panneau porte, en haut, le titre de la **page courante** — déduit de la
// route, donc identique côté serveur et client — puis la barre de filtres,
// collée sous ce titre.
const route = useRoute()
const header = usePageHeaderState()

// Barre de filtre globale (recherche, types, tri), partagée avec les pages.
const { query, sortMode, types, availableTypes, hasTypeFilter, sortLabel, sortIcon, cycleSort, toggle, clearTypes } =
  useFilter()

/** Titre de la page courante, déduit de la route (pas de flash à l'hydratation). */
const title = computed(() => {
  if (route.params.name) return decodeURIComponent(String(route.params.name))
  if (route.params.rel) return basename(decodeURIComponent(String(route.params.rel)))
  return 'Models'
})

const isHome = computed(() => route.path === '/')
/** La vue fichier n'a pas de liste à filtrer. */
const showFilter = computed(() => !route.params.rel)

const placeholder = computed(() =>
  route.params.name ? 'Filtrer par nom de fichier…' : 'Filtrer par nom de fichier ou de dossier…',
)

/**
 * Titre de l'en-tête. Sur l'accueil, il est suivi du nombre de fichiers
 * (`Models - 11`) ; le compteur n'apparaît qu'une fois les données chargées,
 * pour éviter un « Models - 0 » au premier rendu.
 */
const displayTitle = computed(() => {
  if (!isHome.value || !header.value.count) return title.value
  return `${title.value} - ${header.value.count}`
})

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
  { type: 'label', label: 'Catalogue' },
  { label: 'Models', icon: 'i-lucide-box', to: '/', active: isActive('/') },
])

/**
 * Habillage de la navigation : entrées bien détachées (pastille arrondie sur
 * l'entrée active) et intitulé de section en petites capitales.
 */
const navUi = {
  list: 'flex flex-col gap-1',
  label: 'px-3 pt-1 pb-1 text-[0.7rem] font-semibold uppercase tracking-wider text-dimmed',
  link: 'px-3 py-2.5 gap-3 before:rounded-lg',
}
</script>

<template>
  <!--
    Coquille Nuxt UI : `UDashboardGroup` gère la mise en page (sidebar à
    gauche + panneau de contenu) et bascule la sidebar en tiroir sous `lg`.
  -->
  <UDashboardGroup>
    <!--
      Largeur fixée par `.app-sidebar` (Nuxt UI dimensionne en % de la
      fenêtre). Ni `resizable` ni `collapsible` : sur desktop le bouton de
      repli (UDashboardSidebarToggle) est `lg:hidden`, une sidebar repliée
      serait impossible à rouvrir. `bg-muted` = un cran plus clair que la
      page, pour que la pastille active du menu (bg-elevated) ressorte.
    -->
    <UDashboardSidebar
      id="easy3d"
      class="app-sidebar"
      :menu="{ title: 'Navigation', description: 'Navigation de l’application' }"
      :ui="{ root: 'bg-muted' }"
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

      <!-- Navigation : intitulé de section + entrées. `color="neutral"` =
           libellé blanc sur pastille grise pour l'entrée active. -->
      <UNavigationMenu :items="navItems" orientation="vertical" color="neutral" :ui="navUi" />

      <!-- État du backend. -->
      <template #footer>
        <div class="stats">
          <span class="pill">
            <span class="dot" :class="connected ? 'live' : 'err'" /> {{ statusLabel }}
          </span>
        </div>
      </template>
    </UDashboardSidebar>

    <UDashboardPanel id="catalog" :ui="{ body: 'p-0 sm:p-0 gap-0' }">
      <!--
        En-tête : titre de la page courante. Sur les pages de détail, il est
        complété par le contexte (nombre de fichiers, type et date) ; sur
        l'accueil, le titre "Models" se suffit à lui-même.
        La navbar porte aussi la bascule du tiroir sous `lg`.
      -->
      <template #header>
        <UDashboardNavbar>
          <template #title>{{ displayTitle }}</template>
          <template #trailing>
            <small v-if="!isHome" class="navbar__subtitle">{{ header.subtitle }}</small>
          </template>
        </UDashboardNavbar>
      </template>

      <template #body>
        <!-- Barre de recherche + filtres, collée sous l'en-tête pendant le
             défilement. -->
        <div v-if="showFilter" class="topbar">
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
