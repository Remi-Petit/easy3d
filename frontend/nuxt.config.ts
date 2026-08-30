// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  devtools: { enabled: true },
  css: ['~/assets/css/main.css'],
  runtimeConfig: {
    // URL du backend Rust (axum).
    // Priorité : NUXT_HPCCAT_API_BASE, sinon NUXT_HPCCAT_API_PORT,
    // sinon défaut 8090.
    hpccatApiBase:
      process.env.NUXT_HPCCAT_API_BASE ||
      `http://127.0.0.1:${process.env.NUXT_HPCCAT_API_PORT || '8090'}`,
    public: {
      // Base WebSocket du backend (exposée au navigateur pour la connexion WS).
      // Priorité : NUXT_HPCCAT_WS_BASE, sinon dérivée du port API.
      hpccatWsBase:
        process.env.NUXT_HPCCAT_WS_BASE ||
        `ws://127.0.0.1:${process.env.NUXT_HPCCAT_API_PORT || '8090'}`,
    },
  },
  compatibilityDate: '2026-08-28',
})
