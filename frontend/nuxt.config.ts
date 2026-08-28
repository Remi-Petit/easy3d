// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  devtools: { enabled: true },
  css: ['~/assets/css/main.css'],
  runtimeConfig: {
    // URL du backend Rust (axum). Surcharge via NUXT_HPCCAT_API_BASE.
    hpccatApiBase: process.env.NUXT_HPCCAT_API_BASE || 'http://127.0.0.1:8090',
  },
  compatibilityDate: '2026-08-28',
})
