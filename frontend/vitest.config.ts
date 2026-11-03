import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    // Équivalents Nuxt : `~/` et `@/` pointent vers `app/`.
    alias: {
      '~': fileURLToPath(new URL('./app', import.meta.url)),
      '@': fileURLToPath(new URL('./app', import.meta.url)),
      // Alias fourni par Nuxt/Nitro à l'exécution, absent des tests : les
      // utilitaires de `server/` (relais WebSocket) ne lisent que la
      // configuration, la doublure s'en charge (voir `tests/stubs/imports.ts`).
      '#imports': fileURLToPath(new URL('./tests/stubs/imports.ts', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
    // Couverture : périmètre volontairement limité à la **logique** que les
    // tests unitaires exercent. Les composants et pages `.vue` sont couverts
    // par les e2e Playwright : les compter ici afficherait 0 % pour du code
    // testé autrement (et vitest ne compile pas les SFC sans le plugin Vue).
    coverage: {
      provider: 'v8',
      reporter: ['text-summary', 'json-summary'],
      include: ['app/utils/**', 'app/composables/**', 'app/workers/**', 'server/**', 'i18n/**'],
      exclude: ['**/*.d.ts'],
    },
  },
})
