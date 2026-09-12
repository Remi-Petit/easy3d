<script setup lang="ts">
import { fr } from '@nuxt/ui/locale'
</script>

<template>
  <!-- `UApp` fournit le contexte des composants Nuxt UI (locale, toasts…). -->
  <UApp :locale="fr">
    <NuxtLayout>
      <NuxtPage />
    </NuxtLayout>
  </UApp>
</template>
