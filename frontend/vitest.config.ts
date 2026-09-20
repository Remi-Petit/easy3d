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
  },
})
