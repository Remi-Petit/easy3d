<script setup lang="ts">
// Layout global : conteneur + en-tête communs à toutes les pages.
//
// Le logo, le titre et le lien de retour sont déduits de la **route** : ils sont
// identiques côté serveur et client (pas de « hydration mismatch » ni de flash).
// Les valeurs dynamiques (sous-titre, compteur, connexion) viennent de
// `usePageHeader()`, rempli par la page après l'hydratation.
const route = useRoute()
const header = usePageHeaderState()

const page = computed(() => {
  // /dossiers/[name]
  if (route.params.name) {
    return {
      logo: '📁',
      title: decodeURIComponent(String(route.params.name)),
      back: { to: '/', label: '← Retour' },
      home: false,
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
    }
  }
  // Accueil
  return { logo: '3D', title: 'easy3d', back: null, home: true }
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

    <slot />
  </div>
</template>
