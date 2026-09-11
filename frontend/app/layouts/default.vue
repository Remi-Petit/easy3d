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
      back: { to: '/', label: '← Retour' },
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
      back: { to: '/', label: '← Retour' },
      home: false,
      filter: false,
      placeholder: '',
    }
  }
  // Accueil
  return {
    logo: '3D',
    title: 'easy3d',
    back: null,
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
</script>

<template>
  <div class="container">
    <NuxtLink v-if="page.back" :to="page.back.to" class="back">{{ page.back.label }}</NuxtLink>

    <header class="header">
      <div class="brand">
        <div class="logo">{{ page.logo }}</div>
        <div>
          <h1>{{ page.title }}</h1>
          <small>{{ header.subtitle }}</small>
        </div>
      </div>

      <div class="stats">
        <span class="pill">
          <span class="dot" :class="connected ? 'live' : 'err'" /> {{ statusLabel }}
        </span>
        <span v-if="page.home" class="pill"><b>{{ header.count }}</b> fichiers</span>
      </div>
    </header>

    <!-- Barre de filtre globale : recherche + tri par date. -->
    <div v-if="page.filter" class="toolbar">
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
    <div v-if="page.filter && availableTypes.length > 1" class="types">
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

    <slot />
  </div>
</template>
