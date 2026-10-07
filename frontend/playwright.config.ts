import { defineConfig, devices } from '@playwright/test'
import { port } from './tests/ports'

/**
 * Ports de la campagne **catalogue**.
 *
 * Réglables par l'environnement (`tests/ports.ts`) : sur une machine où Windows
 * a réservé la plage, le backend de test ne peut pas ouvrir son port
 * (`os error 10013`) et la campagne s'arrête avant le premier test — un
 * `frontend/.env` suffit alors à en choisir d'autres.
 */
const API_PORT = port('E2E_API_PORT', 8091)
const WEB_PORT = port('E2E_WEB_PORT', 3200)
const WEB_ORIGIN = `http://localhost:${WEB_PORT}`

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  // **Un seul worker**, et ce n'est pas un choix de confort.
  //
  // Le serveur de dev Nuxt tourne dans un processus **forké** qui se tue au
  // **premier rejet de promesse non capturé** — le parent le relance alors, et
  // le port reste fermé pendant la recompilation. Or, sous plusieurs workers,
  // les navigateurs réinitialisent leurs sockets WebSocket en pleine écriture
  // (onglet fermé) et le `ws` embarqué par crossws rejette dans un chemin que
  // rien, côté application, ne peut intercepter (`NodePeer.send()` est
  // synchrone). Résultat : environ deux exécutions sur trois voyaient le serveur
  // de dev mourir en pleine campagne, et tous les tests suivants échouaient en
  // `ERR_CONNECTION_REFUSED`.
  //
  // Ce n'est pas un défaut du code testé : le conteneur sert un build de
  // production, sans fork ni surveillance, et n'est pas concerné. En série, la
  // campagne est passée 16/16 à chaque essai.
  workers: 1,
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  // Le serveur de dev **compile à la demande** : la première visite d'une page
  // prend quelques secondes, d'où un délai d'assertion plus large que le défaut.
  expect: { timeout: 10_000 },
  use: {
    // Port propre aux e2e : **3100 est occupé par le conteneur** (voir
    // `docker-compose.yml`), et le réutiliser ferait tester le conteneur au lieu
    // du code en cours.
    baseURL: WEB_ORIGIN,
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
      url: `http://127.0.0.1:${API_PORT}/health`,
      reuseExistingServer: true,
      // Large : le premier lancement compile le backend dans son propre
      // répertoire de build (`backend/target/e2e`, voir le script).
      timeout: 300_000,
      // Les scripts lisent ces variables pour eux-mêmes : sans ça, le port
      // attendu par Playwright et celui écouté par le backend divergeraient.
      env: { ...process.env, E2E_API_PORT: String(API_PORT) },
    },
    {
      command: 'node tests/e2e/start-frontend.mjs',
      url: WEB_ORIGIN,
      reuseExistingServer: true,
      timeout: 180_000,
      env: {
        ...process.env,
        E2E_WEB_PORT: String(WEB_PORT),
        E2E_API_BASE: `http://127.0.0.1:${API_PORT}`,
      },
    },
  ],
})
