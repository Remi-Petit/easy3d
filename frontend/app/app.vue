<script setup lang="ts">
import { de, en, es, fr } from '@nuxt/ui/locale'

/**
 * Langue des composants Nuxt UI (libellés internes : pagination, menus, dates…).
 * Elle suit celle de l'app, choisie via `@nuxtjs/i18n` — sans quoi la sidebar
 * changerait de langue mais pas les composants qui l'habitent.
 */
const { locale } = useI18n()
const UI_LOCALES: Record<string, typeof fr> = { fr, en, de, es }
const uiLocale = computed(() => UI_LOCALES[locale.value] ?? fr)

// Langue du document (lecteurs d'écran, césure, traduction automatique).
// Le module la pose dans la plupart des cas, mais nos `htmlAttrs` statiques de
// `nuxt.config` la laissent vide : on l'écrit donc explicitement.
useHead({ htmlAttrs: { lang: computed(() => locale.value) } })
</script>

<template>
  <!-- `UApp` fournit le contexte des composants Nuxt UI (locale, toasts…). -->
  <UApp :locale="uiLocale">
    <NuxtLayout>
      <NuxtPage />
    </NuxtLayout>
  </UApp>
</template>
