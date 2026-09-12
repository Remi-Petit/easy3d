// https://nuxt.com/docs/api/configuration/nuxt-config

// Liste des langues : `frontend/i18n/locales.json` est la **seule** source
// (config Nuxt, test de couverture, ligne du README). Ajouter une langue =
// un fichier dans `i18n/locales/` + une entrée ici.
import i18nConfig from './i18n/locales.json'

export default defineNuxtConfig({
  modules: ['@nuxt/ui', '@nuxtjs/i18n'],
  devtools: { enabled: true },
  css: ['~/assets/css/main.css'],
  // L'app est en thème sombre permanent : pas de bascule clair/sombre (sinon
  // les composants Nuxt UI arriveraient en clair au premier rendu).
  ui: { colorMode: false },
  app: {
    head: {
      htmlAttrs: { class: 'dark' },
    },
  },
  // Internationalisation. La langue vit dans un cookie, pas dans l'URL
  // (`no_prefix`) : les liens internes, le partage et les routes
  // `/fichier/<rel>` restent inchangés en changeant de langue.
  i18n: {
    strategy: 'no_prefix',
    defaultLocale: i18nConfig.defaultLocale,
    // Messages dans `frontend/i18n/locales/` (résolus depuis `restructureDir`,
    // qui vaut `<rootDir>/i18n`). `language` sert au SEO et aux formats
    // (`n()`, `d()`, temps relatif), `name` au sélecteur de langue.
    locales: i18nConfig.locales,
    // Le cookie est lu par le serveur : la langue est donc déjà bonne dans le
    // HTML rendu par SSR, sans bascule visible à l'hydratation.
    detectBrowserLanguage: {
      useCookie: true,
      cookieKey: 'easy3d_lang',
      fallbackLocale: i18nConfig.defaultLocale,
      redirectOn: 'root',
      alwaysRedirect: false,
    },
  },
  runtimeConfig: {
    // URL du backend Rust (axum).
    // Priorité : NUXT_HPCCAT_API_BASE, sinon NUXT_HPCCAT_API_PORT,
    // sinon défaut 8090.
    hpccatApiBase:
      process.env.NUXT_HPCCAT_API_BASE ||
      `http://127.0.0.1:${process.env.NUXT_HPCCAT_API_PORT || '8090'}`,
    public: {
      // Base WebSocket vue par le navigateur. Vide par défaut : les connexions
      // visent l'origine qui sert l'interface, Nitro relayant `/ws` et
      // `/collab/*` vers le backend (voir `server/routes/`). À poser seulement
      // si le temps réel doit être joint sur un autre hôte que l'interface.
      hpccatWsBase: process.env.NUXT_PUBLIC_HPCCAT_WS_BASE || '',
    },
  },
  // Le temps réel (catalogue `/ws`, notes `/collab/*`) est relayé par Nitro
  // vers le backend (voir `server/routes/`) : sans ce drapeau, Nitro refuse
  // l'upgrade et les deux routes répondent 426.
  nitro: {
    experimental: {
      websocket: true,
    },
  },
  compatibilityDate: '2026-08-28',
})
