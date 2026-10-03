import { defineConfig, devices } from '@playwright/test'
import {
  ADMIN_EMAIL,
  ADMIN_PASSWORD,
  ADMIN_USERNAME,
  API_PORT,
  IDP_CLIENT_ID,
  IDP_CLIENT_SECRET,
  IDP_ISSUER,
  IDP_PORT,
  SSO_EMAIL,
  SSO_SUBJECT,
  SSO_USERNAME,
  WEB_ORIGIN,
  WEB_PORT,
} from './tests/e2e-auth/params'

/**
 * Campagne e2e « authentification » : la même application, **comptes allumés** et
 * SSO branché sur le fournisseur fictif (`docker/fake-oidc/server.mjs`).
 *
 * # Pourquoi une seconde configuration
 *
 * `playwright.config.ts` teste le catalogue **sans** comptes — c'est le
 * comportement par défaut du backend, et il doit le rester. Ici, tout est fermé
 * sans session : les deux campagnes n'ont donc ni les mêmes réglages, ni la même
 * base de comptes, ni les mêmes ports. Les mélanger obligerait chaque test à
 * choisir son mode, et le moindre oubli ferait tester l'un en croyant tester
 * l'autre.
 *
 * # Le fournisseur d'identité
 *
 * C'est le **même** script que celui du compose de développement, lancé ici
 * directement par Node (il n'a aucune dépendance) : ce que l'on teste est donc
 * exactement ce que l'on utilise à la main, sur un port à part.
 *
 *   bun run test:e2e:auth
 */
export default defineConfig({
  testDir: './tests/e2e-auth',
  // Séquentiel : les tests partagent une base de comptes (celle du backend), et
  // l'ordre des événements du journal en dépend.
  fullyParallel: false,
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  expect: { timeout: 10_000 },
  use: {
    baseURL: WEB_ORIGIN,
    trace: 'on-first-retry',
    // Langue de référence, comme dans la campagne du catalogue : les assertions
    // portent sur du texte traduit.
    locale: 'fr-FR',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    {
      // Le port du serveur fictif est publié **tel quel** (`base`), et l'adresse
      // annoncée au navigateur est celle de `PUBLIC_URL` : ici, les deux sont le
      // même hôte — l'hôte, pas le réseau Compose.
      command: 'node ../docker/fake-oidc/server.mjs',
      url: `${IDP_ISSUER}/health`,
      reuseExistingServer: true,
      timeout: 30_000,
      env: {
        ...process.env,
        PORT: String(IDP_PORT),
        PUBLIC_URL: IDP_ISSUER,
        CLIENT_ID: IDP_CLIENT_ID,
        CLIENT_SECRET: IDP_CLIENT_SECRET,
        SUBJECT: SSO_SUBJECT,
        EMAIL: SSO_EMAIL,
        // `USERNAME` **est déjà définie sous Windows** (le compte du poste) : on
        // l'écrase explicitement, sinon le compte simulé porterait le nom de la
        // session et l'assertion dépendrait de la machine.
        USERNAME: SSO_USERNAME,
        NAME: 'Compte SSO e2e',
      },
    },
    {
      command: 'node tests/e2e/start-backend.mjs',
      url: `http://127.0.0.1:${API_PORT}/health`,
      reuseExistingServer: true,
      // Large : le premier lancement compile le backend dans `backend/target/e2e`.
      timeout: 300_000,
      env: {
        ...process.env,
        E2E_API_PORT: String(API_PORT),
        // ── Comptes ─────────────────────────────────────────────────────────
        EASY3D_AUTH: 'on',
        EASY3D_ADMIN_USERNAME: ADMIN_USERNAME,
        EASY3D_ADMIN_EMAIL: ADMIN_EMAIL,
        EASY3D_ADMIN_PASSWORD: ADMIN_PASSWORD,
        // Adresse publique : elle construit l'adresse de retour du SSO
        // (`<PUBLIC_URL>/api/auth/oidc/callback`) et décide du drapeau `Secure`
        // du cookie — HTTP ici, donc sans `Secure`.
        EASY3D_PUBLIC_URL: WEB_ORIGIN,
        // ── SSO ─────────────────────────────────────────────────────────────
        EASY3D_OIDC_ISSUER: IDP_ISSUER,
        EASY3D_OIDC_CLIENT_ID: IDP_CLIENT_ID,
        EASY3D_OIDC_CLIENT_SECRET: IDP_CLIENT_SECRET,
        // `auto` : une première connexion SSO crée le compte et lui donne le
        // rôle livré `lecteur` (rôle par défaut d'une base neuve).
        EASY3D_OIDC_PROVISIONING: 'auto',
      },
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
