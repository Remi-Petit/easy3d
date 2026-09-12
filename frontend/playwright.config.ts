import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: 'http://localhost:3100',
    trace: 'on-first-retry',
    // Langue du navigateur épinglée sur la langue de référence : sans ça, les
    // assertions sur du texte dépendraient de la langue du poste (l'app suit
    // `Accept-Language` au premier chargement).
    locale: 'fr-FR',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  // Démarre le backend Rust (8090) + le frontend Nuxt (3100) avant le test.
  webServer: [
    {
      command: 'cd ../backend && PORT=8090 cargo run',
      url: 'http://127.0.0.1:8090/health',
      reuseExistingServer: true,
      timeout: 120_000,
    },
    {
      command: 'npx nuxt dev --port 3100',
      url: 'http://localhost:3100',
      reuseExistingServer: true,
      timeout: 180_000,
    },
  ],
})
