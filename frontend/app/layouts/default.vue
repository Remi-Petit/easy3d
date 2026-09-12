<script setup lang="ts">
// Layout global : conteneur + en-tête communs à toutes les pages.
//
// Le logo, le titre et le lien de retour sont déduits de la **route** : ils sont
// identiques côté serveur et client (pas de « hydration mismatch » ni de flash).
// Les valeurs dynamiques (sous-titre, compteur, connexion) viennent de
// `usePageHeader()`, rempli par la page après l'hydratation.
const route = useRoute()
const header = usePageHeaderState()

// Barre de filtre globale (recherche, types, tri), partagée avec les pages.
const { query, sortMode, types, availableTypes, hasTypeFilter, sortLabel, sortIcon, cycleSort, toggle, clearTypes } =
  useFilter()

const page = computed(() => {
  // /dossiers/[name]
  if (route.params.name) {
    return {
      logo: '📁',
      title: decodeURIComponent(String(route.params.name)),
      home: false,
      filter: true,
      placeholder: 'Filtrer par nom de fichier…',
    }
  }
  // /fichier/[rel]
  if (route.params.rel) {
    const rel = decodeURIComponent(String(route.params.rel))
    return {
      logo: fileIcon(rel),
      title: basename(rel),
      home: false,
      filter: false,
      placeholder: '',
    }
  }
  // Accueil
  return {
    logo: '3D',
    title: 'easy3d',
    home: true,
    filter: true,
    placeholder: 'Filtrer par nom de fichier ou de dossier…',
  }
})

// "connecté" = WS live (temps réel) OU données qui remontent (polling sans erreur).
const connected = computed(() => header.value.live || !header.value.offline)
const statusLabel = computed(() =>
  header.value.live ? 'temps réel' : header.value.offline ? 'hors ligne' : 'repli polling',
)

/**
 * Sections de navigation. Une seule entrée pour l'instant (le catalogue) ;
 * les pages de détail `/dossiers/...` et `/fichier/...` en font partie.
 */
const navItems = [{ to: '/', icon: '🧊', label: 'Models' }]

/** Routes rattachées à une section dont l'URL ne porte pas le préfixe. */
const SECTION_ROUTES: Record<string, string[]> = {
  '/': ['/dossiers', '/fichier'],
}

/** Une entrée est active si la route courante appartient à sa section. */
function isActive(to: string) {
  return route.path === to || (SECTION_ROUTES[to] ?? []).some((p) => route.path.startsWith(p))
}

// Tiroir mobile : ouvert par le bouton ☰, fermé au clic sur le voile, sur
// Échap, ou après un changement de page.
const sidebarOpen = ref(false)

function closeSidebar() {
  sidebarOpen.value = false
}

watch(() => route.fullPath, closeSidebar)

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeSidebar()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div class="shell">
    <!--
      Sidebar : marque, navigation et état du backend.
      La marque reprend le titre/sous-titre de la page (déduits de la route par
      le script ci-dessus), donc sidebar et contenu racontent la même chose.
    -->
    <aside id="app-sidebar" class="sidebar" :class="{ 'sidebar--open': sidebarOpen }">
      <div class="brand">
        <div class="logo">{{ page.logo }}</div>
        <div>
          <h1>{{ page.title }}</h1>
          <small>{{ header.subtitle }}</small>
        </div>
      </div>

      <nav class="nav" aria-label="Navigation principale">
        <p class="nav__section">Navigation</p>
        <NuxtLink
          v-for="item in navItems"
          :key="item.to"
          :to="item.to"
          class="nav__item"
          :class="{ 'nav__item--on': isActive(item.to) }"
          :aria-current="isActive(item.to) ? 'page' : undefined"
        >
          <span class="nav__icon">{{ item.icon }}</span>
          <span>{{ item.label }}</span>
        </NuxtLink>
      </nav>

      <div class="stats sidebar__foot">
        <span class="pill">
          <span class="dot" :class="connected ? 'live' : 'err'" /> {{ statusLabel }}
        </span>
        <span v-if="page.home" class="pill"><b>{{ header.count }}</b> fichiers</span>
      </div>
    </aside>

    <!-- Voile du tiroir (mobile) : un clic referme la sidebar. -->
    <div v-if="sidebarOpen" class="scrim" aria-hidden="true" @click="closeSidebar" />

    <div class="content">
      <!--
        Bouton ☰ : visible uniquement quand la sidebar devient un tiroir, et
        masqué tant que le tiroir est ouvert (le voile sert alors à le fermer)
        pour ne pas recouvrir le logo de la sidebar.
      -->
      <button
        v-show="!sidebarOpen"
        type="button"
        class="burger"
        aria-controls="app-sidebar"
        aria-label="Ouvrir la navigation"
        :aria-expanded="sidebarOpen"
        @click="sidebarOpen = true"
      >
        ☰
      </button>

      <!-- En-tête : recherche + tri, puis filtre par type. -->
      <header v-if="page.filter" class="header">
        <div class="toolbar">
          <div class="search">
            <span class="icon">🔍</span>
            <input v-model="query" type="text" :placeholder="page.placeholder" />
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
      </header>

      <main class="main">
        <slot />
      </main>
    </div>
  </div>
</template>
