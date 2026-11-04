import { defineConfig, devices } from '@playwright/test'
import {
  ADMIN_EMAIL,
  ADMIN_PASSWORD,
  ADMIN_USERNAME,
  API_PORT,
  KC_CLIENT_ID,
  KC_CLIENT_SECRET,
  KC_ISSUER,
  WEB_ORIGIN,
  WEB_PORT,
} from './tests/e2e-sso/params'

/**
 * Campagne e2e « SSO réel » : la même application, **comptes allumés** et SSO
 * branché sur un **vrai** fournisseur OIDC (Keycloak, en conteneur).
 *
 * # Trois campagnes, trois questions
 *
 * | `playwright.config.ts`      | le catalogue, sans comptes (le défaut du backend) |
 * | `playwright.auth.config.ts` | la chaîne complète, fournisseur **fictif**      |
 * | ce fichier                  | la chaîne complète, fournisseur **réel**         |
 *
 * Les deux dernières ne diffèrent que par le fournisseur : c'est volontaire,
 * pour que la différence entre « le câblage est bon » et « easy3d parle à un
 * vrai OIDC » se lise dans la configuration, pas dans les tests.
 *
 * # Ce qui tourne où
 *
 * - Keycloak : conteneur Docker, **projet Compose à part** (`easy3d-sso`),
 *   démarré par `global-setup.ts` — voir `docker/keycloak/realm.json` ;
 * - backend Rust et Nuxt : sur l'hôte, comme dans la campagne `e2e-auth` (base
 *   de comptes jetable, ports à part, recompilation locale).
 *
 *   bun run test:e2e:sso
 */
export default defineConfig({
  testDir: './tests/e2e-sso',
  // Un seul worker : les tests partagent la base de comptes du backend et
  // l'ordre des entrées du journal. Le parallélisme n'apporterait rien (une
  // dizaine de cas) et rendrait les assertions dépendantes du hasard.
  fullyParallel: false,
  workers: 1,
  // Plus large que la campagne `e2e-auth` : le premier passage démarre Keycloak
  // (30-60 s) et peut compiler le backend dans `backend/target/e2e`.
  timeout: 120_000,
  retries: 0,
  expect: { timeout: 10_000 },
  globalSetup: './tests/e2e-sso/global-setup.ts',
  globalTeardown: './tests/e2e-sso/global-teardown.ts',
  use: {
    baseURL: WEB_ORIGIN,
    trace: 'on-first-retry',
    locale: 'fr-FR',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    {
      command: 'node tests/e2e/start-backend.mjs',
      url: `http://127.0.0.1:${API_PORT}/health`,
      reuseExistingServer: true,
      timeout: 300_000,
      env: {
        ...process.env,
        E2E_API_PORT: String(API_PORT),
        // ── Comptes ─────────────────────────────────────────────────────────
        EASY3D_AUTH: 'on',
        EASY3D_ADMIN_USERNAME: ADMIN_USERNAME,
        // Même adresse que le compte `admin` de Keycloak : la connexion SSO
        // doit **rattacher** cet administrateur, pas créer un second compte.
        EASY3D_ADMIN_EMAIL: ADMIN_EMAIL,
        EASY3D_ADMIN_PASSWORD: ADMIN_PASSWORD,
        // Adresse publique : elle construit l'adresse de retour du SSO
        // (`<PUBLIC_URL>/api/auth/oidc/callback`, à déclarer chez le
        // fournisseur) et décide du drapeau `Secure` du cookie — HTTP, donc
        // sans `Secure`.
        EASY3D_PUBLIC_URL: WEB_ORIGIN,
        // ── SSO (Keycloak) ──────────────────────────────────────────────────
        // L'émetteur doit être **exactement** celui que la découverte annonce,
        // sinon le backend refuse le document (garde-fou contre une redirection
        // DNS). C'est pourquoi `KC_HOSTNAME` est passé à Compose par
        // `global-setup.ts`.
        EASY3D_OIDC_ISSUER: KC_ISSUER,
        EASY3D_OIDC_CLIENT_ID: KC_CLIENT_ID,
        EASY3D_OIDC_CLIENT_SECRET: KC_CLIENT_SECRET,
        // `auto` : une première connexion SSO inconnue crée le compte et lui
        // donne le rôle livré `lecteur`.
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
