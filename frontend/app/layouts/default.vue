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
const { t, locale, locales, setLocale } = useI18n()

/**
 * Version du projet, injectée par `nuxt.config.ts` depuis `package.json` :
 * c'est le repère qui dit quel build tourne réellement (backend et frontend
 * portent la même version, `scripts/bump-version.mjs` les tient alignés).
 */
const version = useRuntimeConfig().public.version

// Barre de filtre globale (recherche, types, tri), partagée avec les pages.
const { query, sortMode, types, availableTypes, hasTypeFilter, sortLabel, sortIcon, cycleSort, toggle, clearTypes } =
  useFilter()

// Envoi de fichiers : le dépôt (voir plus bas) et la barre d'outils partagent le
// même état, donc le même avancement (voir `useUpload`).
const { upload } = useUpload()

/** `true` sur la page d'administration : section distincte du catalogue. */
const isAdmin = computed(() => route.path.startsWith('/admin'))

/** Titre de la page courante, déduit de la route (pas de flash à l'hydratation). */
const title = computed(() => {
  if (isAdmin.value) return t('nav.admin')
  if (route.params.name) return decodeURIComponent(String(route.params.name))
  if (route.params.rel) return basename(decodeURIComponent(String(route.params.rel)))
  return t('catalog.title')
})

const isHome = computed(() => route.path === '/')
/** Barre de recherche du catalogue : ni sur la vue fichier, ni sur l'admin. */
const showFilter = computed(() => !route.params.rel && !isAdmin.value)

/**
 * Dossier d'accueil d'un envoi de fichiers : celui de la page courante quand on
 * est dans un dossier, sinon la racine du catalogue.
 */
const uploadFolder = computed(() =>
  route.params.name ? decodeURIComponent(String(route.params.name)) : null,
)

/**
 * Dépôt de fichiers sur le catalogue.
 *
 * `dragenter`/`dragleave` se déclenchent aussi pour chaque élément survolé :
 * un compteur évite que le voile clignote au passage d'un enfant à l'autre.
 */
const dragging = ref(false)
let dragDepth = 0

/** Seuls les fichiers nous intéressent (pas un texte ni un lien glissé). */
function droppingFiles(event: DragEvent) {
  return Array.from(event.dataTransfer?.types ?? []).includes('Files')
}

function onDragEnter(event: DragEvent) {
  if (!droppingFiles(event)) return
  dragDepth += 1
  dragging.value = true
}

function onDragLeave() {
  dragDepth = Math.max(0, dragDepth - 1)
  if (!dragDepth) dragging.value = false
}

function onDragOver(event: DragEvent) {
  // Sans `preventDefault`, le navigateur refuse le dépôt — et ouvrirait le
  // fichier déposé à la place.
  if (droppingFiles(event)) event.preventDefault()
}

async function onDrop(event: DragEvent) {
  event.preventDefault()
  dragDepth = 0
  dragging.value = false

  const data = event.dataTransfer
  if (!data) return
  // `dropPicks` lit les entrées du `DataTransfer` avant son premier `await` :
  // elles ne sont valides que le temps de l'événement.
  void upload(await dropPicks(data), uploadFolder.value ?? '')
}

const placeholder = computed(() =>
  route.params.name ? t('filter.placeholderFolder') : t('filter.placeholder'),
)

/**
 * Titre de l'en-tête. Sur l'accueil, il est suivi du nombre de fichiers
 * (`Models - 11`) ; le compteur n'apparaît qu'une fois les données chargées,
 * pour éviter un « Models - 0 » au premier rendu.
 */
const displayTitle = computed(() => {
  if (!isHome.value || !header.value.count) return title.value
  return `${title.value}  (${header.value.count})`
})

// "connecté" = WS live (temps réel) OU données qui remontent (polling sans erreur).
const connected = computed(() => header.value.live || !header.value.offline)
const statusLabel = computed(() =>
  header.value.live ? t('status.live') : header.value.offline ? t('status.offline') : t('status.polling'),
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
  { type: 'label', label: t('nav.catalog') },
  { label: t('nav.models'), icon: 'i-lucide-box', to: '/', active: isActive('/') },
  { type: 'label', label: t('nav.system') },
  { label: t('nav.admin'), icon: 'i-lucide-settings', to: '/admin', active: isAdmin.value },
])

/** Langues du sélecteur : nom affiché + code (`locales` peut être mixte). */
const languageItems = computed(() =>
  locales.value.map((l) =>
    typeof l === 'string' ? { code: l, name: l } : { code: l.code, name: l.name ?? l.code },
  ),
)

/** Le changement passe par `setLocale` : lui seul écrit le cookie relu au SSR. */
function onLanguageChange(event: Event) {
  setLocale((event.target as HTMLSelectElement).value)
}

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
      :menu="{ title: $t('nav.menuTitle'), description: $t('nav.menuDescription') }"
      :ui="{ root: 'bg-muted' }"
    >
      <!-- Identité de l'app. -->
      <template #header>
        <div class="brand">
          <div class="logo">3D</div>
          <div>
            <p class="brand__name">easy3d</p>
            <small>{{ $t('app.tagline') }}</small>
          </div>
        </div>
      </template>

      <!-- Navigation : intitulé de section + entrées. `color="neutral"` =
           libellé blanc sur pastille grise pour l'entrée active. -->
      <UNavigationMenu :items="navItems" orientation="vertical" color="neutral" :ui="navUi" />

      <!-- État du backend + langue de l'interface. -->
      <template #footer>
        <div class="foot">
          
          <div class="stats">
            <span class="pill">
              <span class="dot" :class="connected ? 'live' : 'err'" /> {{ statusLabel }}
            </span>
          </div>

          <!-- Version du projet, sous l'état du backend. -->
          <p class="version">v{{ version }}</p>

          <label class="lang">
            <span class="lang__label">{{ $t('language.label') }}</span>
            <select
              class="lang__select"
              :value="locale"
              :aria-label="$t('language.choose')"
              @change="onLanguageChange"
            >
              <option v-for="item in languageItems" :key="item.code" :value="item.code">
                {{ item.name }}
              </option>
            </select>
          </label>
        </div>
      </template>
    </UDashboardSidebar>

    <!--
      Zone de dépôt : tout le panneau de contenu. Les écouteurs sont posés sur
      le panneau — `UDashboardPanel` transmet ses attributs à son élément racine
      — et le voile est rendu dans le corps, donc sous l'en-tête.
    -->
    <UDashboardPanel
      id="catalog"
      class="drop"
      :ui="{ body: 'p-0 sm:p-0 gap-0' }"
      @dragenter="onDragEnter"
      @dragover="onDragOver"
      @dragleave="onDragLeave"
      @drop="onDrop"
    >
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
              :title="$t('filter.sortHint', { mode: sortLabel })"
              :aria-label="$t('filter.sortBy', { mode: sortLabel })"
              @click="cycleSort"
            >
              <span class="sort__icon">{{ sortIcon }}</span>
              <span class="sort__label">{{ $t('filter.date') }}</span>
            </button>

            <!-- Ajout de fichiers/dossiers dans le catalogue (le dossier
                 courant sert d'accueil quand on est dans un dossier). -->
            <UploadButton :folder="uploadFolder" />
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
              :title="$t('filter.typeTitle', { count: entry.count, type: entry.type.toUpperCase() }, entry.count)"
              @click="toggle(entry.type)"
            >
              {{ entry.type.toUpperCase() }} <b>{{ entry.count }}</b>
            </button>
            <button v-if="hasTypeFilter" type="button" class="types__clear" @click="clearTypes()">
              {{ $t('filter.clear') }}
            </button>
          </div>
        </div>

        <div class="page">
          <slot />
        </div>

        <!-- Voile de dépôt : `pointer-events: none`, sinon il intercepterait
             les `dragover`/`drop` et le dépôt n'aboutirait jamais. -->
        <div v-if="dragging" class="drop__veil">
          <p class="drop__hint">{{ $t('upload.dropHere') }}</p>
          <p class="drop__where">
            {{ uploadFolder ? $t('upload.into', { folder: uploadFolder }) : $t('upload.atRoot') }}
          </p>
        </div>
      </template>
    </UDashboardPanel>
  </UDashboardGroup>
</template>
