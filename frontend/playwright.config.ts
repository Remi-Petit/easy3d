import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  // Le serveur de dev **compile à la demande** : la première visite d'une page
  // prend quelques secondes, d'où un délai d'assertion plus large que le défaut.
  expect: { timeout: 10_000 },
  use: {
    // Port propre aux e2e : **3100 est occupé par le conteneur** (voir
    // `docker-compose.yml`), et le réutiliser ferait tester le conteneur au lieu
    // du code en cours.
    baseURL: 'http://localhost:3200',
    trace: 'on-first-retry',
    // Langue du navigateur épinglée sur la langue de référence : sans ça, les
    // assertions sur du texte dépendraient de la langue du poste (l'app suit
    // `Accept-Language` au premier chargement).
    locale: 'fr-FR',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  // Démarre le backend Rust (8091) + le frontend Nuxt (3200) avant les tests.
  //
  // Aucun des deux ne prend les ports du développement courant : 3100 est au
  // conteneur, 8090 au backend de l'hôte. Les commandes passent par Node (voir
  // `tests/e2e/`) : la syntaxe d'environnement du shell POSIX (`PORT=8090 cargo
  // run`) ne passe pas sous Windows, et le backend a besoin d'un dossier de
  // données **jetable**.
  webServer: [
    {
      command: 'node tests/e2e/start-backend.mjs',
      url: 'http://127.0.0.1:8091/health',
      reuseExistingServer: true,
      // Large : le premier lancement compile le backend dans son propre
      // répertoire de build (`backend/target/e2e`, voir le script).
      timeout: 300_000,
    },
    {
      command: 'node tests/e2e/start-frontend.mjs',
      url: 'http://localhost:3200',
      reuseExistingServer: true,
      timeout: 180_000,
    },
  ],
})
