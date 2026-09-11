<script setup lang="ts">
// Bouton de retour des pages de détail. Rendu par la page (donc sous l'en-tête
// du layout, en dehors de celui-ci).
const route = useRoute()
const router = useRouter()

/** Parent logique : le dossier du fichier, ou l'accueil. */
const fallback = computed(() => parentPath(route.path))

/**
 * Revient à la **position précédente** si elle est interne à l'application
 * (`router.back()`), sinon au parent logique.
 *
 * Le repli couvre l'arrivée par lien direct ou après rechargement, où il n'y a
 * pas d'historique interne — et évite surtout de sortir du site.
 */
function goBack() {
  const previous = router.options.history.state.back
  if (typeof previous === 'string' && previous.startsWith('/') && previous !== route.path) {
    router.back()
    return
  }
  router.push(fallback.value)
}
</script>

<template>
  <button type="button" class="back" aria-label="Revenir en arrière" @click="goBack">
    ← Retour
  </button>
</template>
