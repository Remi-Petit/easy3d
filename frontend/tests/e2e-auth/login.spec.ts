import { expect, test, type Page } from '@playwright/test'
import { ADMIN_EMAIL, ADMIN_PASSWORD, ADMIN_USERNAME, SSO_USERNAME } from './params'

/**
 * Authentification de bout en bout : navigateur → Nuxt (proxy) → backend Rust →
 * fournisseur d'identité fictif.
 *
 * C'est le **seul** endroit où la chaîne complète est exercée : les tests
 * unitaires du backend (`auth::oidc::tests`) simulent le transport, et ceux de
 * `api.rs` montent le routeur sans passer par le navigateur. Ici, une vraie
 * redirection, un vrai `POST` de jeton et une vraie page d'autorisation sont en
 * jeu — c'est exactement là que se logent les erreurs de câblage (adresse
 * publique, `redirect_uri`, cookie, provisionnement).
 *
 * Tous les réglages viennent de `params.ts` et de `playwright.auth.config.ts` :
 * rien n'est recopié ici, sinon les couples identifiant/mot de passe finiraient
 * par diverger du backend.
 */

/**
 * Saisit les identifiants, puis valide — en réessayant la saisie.
 *
 * Le serveur de développement sert d'abord le HTML du rendu serveur : une saisie
 * arrivée **avant** l'hydratation est écrasée quand Vue reprend la main, et le
 * bouton reste inerte (il est désactivé tant qu'un champ est vide). Le repère
 * fiable est donc le bouton lui-même : on resaisit jusqu'à ce qu'il s'active,
 * comme dans `admin.spec.ts`.
 */
async function saisirIdentifiants(page: Page, identifiant: string, motDePasse: string) {
  const bouton = page.getByRole('button', { name: 'Se connecter' })
  await expect(async () => {
    await page.getByLabel('Identifiant ou e-mail').fill(identifiant)
    await page.getByLabel('Mot de passe').fill(motDePasse)
    await expect(bouton).toBeEnabled({ timeout: 1000 })
  }).toPass({ timeout: 30_000 })
  await bouton.click()
}

/** Remplit le formulaire, puis attend que la session soit ouverte. */
async function seConnecter(page: Page): Promise<void> {
  await page.goto('/login')
  await saisirIdentifiants(page, ADMIN_EMAIL, ADMIN_PASSWORD)

  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
  await expect(page.locator('.sidebar-user__who')).toHaveText(ADMIN_USERNAME)
}

test('sans session, le catalogue mène à la connexion', async ({ page }) => {
  await page.goto('/')

  // La garde s'applique **avant** le rendu : le catalogue n'est même pas
  // aperçu, et la page demandée est retenue pour y revenir après connexion.
  await expect(page).toHaveURL((url) => url.pathname === '/login', { timeout: 30_000 })
  await expect(page.locator('h1')).toHaveText('Connexion')
  expect(new URL(page.url()).searchParams.get('redirect')).toBe('/')
})

test('sans session, l’administration mène aussi à la connexion', async ({ page }) => {
  await page.goto('/admin/audit')

  await expect(page).toHaveURL((url) => url.pathname === '/login', { timeout: 30_000 })
  // Le chemin demandé voyage avec la redirection : on repart d'où l'on venait.
  expect(new URL(page.url()).searchParams.get('redirect')).toBe('/admin/audit')
})

test('la connexion par mot de passe ouvre le catalogue, la déconnexion le referme', async ({
  page,
}) => {
  await seConnecter(page)

  // Le compte est bien celui que l'on a créé au démarrage, et le lien SSO est
  // proposé puisque le serveur annonce un fournisseur.
  await expect(page.locator('.sidebar-user__who')).toHaveAttribute('title', ADMIN_EMAIL)

  await page.getByRole('button', { name: 'Se déconnecter' }).click()

  await expect(page).toHaveURL((url) => url.pathname === '/login')
  // La session est réellement fermée côté serveur : revenir au catalogue
  // redemande une connexion.
  await page.goto('/')
  await expect(page).toHaveURL((url) => url.pathname === '/login')
})

test('un refus ne dit pas si le compte existe', async ({ page }) => {
  await page.goto('/login')
  await saisirIdentifiants(page, 'personne@e2e.test', 'mot-de-passe-invente')

  // Même message pour un compte inconnu et pour un mot de passe erroné : c'est
  // la règle, et elle est vérifiée ici sur l'interface.
  await expect(page.getByRole('alert')).toHaveText('Identifiant ou mot de passe incorrect.')
  await expect(page).toHaveURL((url) => url.pathname === '/login')
})

test('le SSO traverse le fournisseur et ouvre une session', async ({ page }) => {
  await page.goto('/login')

  // Le libellé porte le nom du fournisseur annoncé par le serveur : le cliquer
  // **quitte** l'application (c'est un lien, pas un appel `fetch`).
  await page.getByRole('link', { name: /Continuer avec/ }).click()

  // Le fournisseur fictif redirige immédiatement, sans mot de passe : on
  // revient donc sur le catalogue sans rien saisir.
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })

  // Le compte n'existait pas : c'est le provisionnement automatique qui l'a
  // créé, avec le rôle livré `lecteur` — d'où le catalogue accessible.
  await expect(page.locator('.sidebar-user__who')).toHaveText(SSO_USERNAME)
  await expect(page.locator('h1')).toContainText('Models')
})

test('l’administration liste les comptes et le journal garde la connexion', async ({ page }) => {
  await seConnecter(page)

  await page.goto('/admin/accounts')
  await expect(page.locator('h1')).toHaveText('Comptes', { timeout: 30_000 })
  // L'administrateur créé au démarrage figure dans la liste.
  await expect(page.locator('.admin__item', { hasText: ADMIN_USERNAME }).first()).toBeVisible()

  await page.goto('/admin/audit')
  await expect(page.locator('h1')).toHaveText('Journal', { timeout: 30_000 })
  // La connexion qui vient d'avoir lieu est journalisée : c'est un témoin, pas
  // une navigation dans le catalogue (qui, elle, n'y figure pas).
  await expect(page.locator('.audit__table tbody tr').first()).toContainText('Connexion')

  // Les puces de famille filtrent le journal ; le compte se cherche au clavier.
  await expect(page.locator('.audit .types__item')).toHaveCount(5)
  await page.locator('.audit .types__item', { hasText: 'Sessions' }).click()
  await expect(page.locator('.audit__table tbody tr').first()).toContainText('Connexion')
  // L'en-tête du tableau est collant (`sticky` de Nuxt UI) : la page se lit en
  // descendant, la ligne des titres reste.
  await expect(page.locator('.audit .audit__head')).toHaveCSS('position', 'sticky')
  await page.locator('.audit .toolbar .search input').fill('personne-de-ce-nom')
  await expect(page.locator('.admin__empty')).toBeVisible()
})

/**
 * Les comptes ont deux onglets — utilisateurs et rôles — sur le même système que
 * « mon compte » : c'est l'**URL** qui porte la vue, et un rôle neuf se crée
 * depuis l'onglet des rôles.
 */
test('les comptes ont deux onglets, et un rôle s’y crée', async ({ page }) => {
  await seConnecter(page)

  await page.goto('/admin/accounts')
  await expect(page.locator('h1')).toHaveText('Comptes', { timeout: 30_000 })

  const onglets = page.locator('.panel-tabs [role="tab"]')
  await expect(onglets).toHaveCount(2)
  await expect(onglets.first()).toHaveText('Utilisateurs')
  await expect(onglets.first()).toHaveAttribute('aria-selected', 'true')
  // L'administrateur créé au démarrage figure dans la liste des utilisateurs,
  // et non dans celle des rôles.
  await expect(page.locator('.admin__item', { hasText: ADMIN_USERNAME }).first()).toBeVisible()

  await onglets.nth(1).click()
  await expect(page).toHaveURL(/\/admin\/accounts\/roles$/)
  await expect(onglets.nth(1)).toHaveAttribute('aria-selected', 'true')
  await expect(page.locator('.admin__item', { hasText: 'admin' }).first()).toBeVisible()

  // Un rôle neuf se crée ici. Ses droits se cochent ensuite dans l'éditeur, que
  // le clic sur la ligne ouvre.
  const nom = 'role-e2e'
  await page.getByLabel('Nom du rôle').last().fill(nom)
  await page.getByRole('button', { name: 'Créer' }).click()
  const ligne = page.locator('.admin__item', { hasText: nom }).first()
  await expect(ligne).toBeVisible()

  // Le test repart propre : ce qu'il a créé, il le supprime (deux clics, le
  // second confirme).
  await ligne.click()
  await page.getByRole('button', { name: 'Supprimer', exact: true }).click()
  await page.locator('.admin__action--danger').click()
  await expect(page.locator('.admin__item', { hasText: nom })).toHaveCount(0)

  // L'adresse suffit à ouvrir la vue : c'est elle qui fait foi.
  await page.goto('/admin/accounts/roles')
  await expect(page.locator('.panel-tabs [role="tab"]').nth(1)).toHaveAttribute(
    'aria-selected',
    'true',
  )
})

test('la barre du catalogue ne suit pas sur « mon compte »', async ({ page }) => {
  await seConnecter(page)

  // Le catalogue, lui, a la sienne.
  await page.goto('/')
  await expect(page.locator('.topbar .search input')).toBeVisible()

  // « Mon compte » ne filtre pas des modèles : ni recherche, ni tri, ni envoi de
  // fichiers, ni puces de formats. Sans garde, sa route héritait de la barre
  // entière — branchée sur le catalogue, donc sans effet ici.
  for (const onglet of ['/account', '/account/api', '/account/mcp']) {
    await page.goto(onglet)
    await expect(page.locator('.panel-tabs')).toBeVisible()
    await expect(page.locator('.topbar')).toHaveCount(0)
    await expect(page.locator('.ai-btn')).toHaveCount(0)
  }

  // L'administration a la sienne (le journal), et pas celle du catalogue.
  await page.goto('/admin/audit')
  await expect(page.locator('.audit .toolbar')).toBeVisible()
})

test('les onglets de « mon compte » occupent toute la largeur', async ({ page }) => {
  await seConnecter(page)

  // Le contenu d'un onglet ne décide pas de la largeur de la page : les deux vues
  // n'ont pas le même contenu (identité et mot de passe d'un côté, la liste des
  // jetons de l'autre), et un panneau qui suit son contenu — ou qui se centre —
  // fait sauter la page au changement d'onglet.
  for (const onglet of ['/account', '/account/api', '/account/mcp']) {
    await page.goto(onglet)
    await expect(page.locator('[role="tabpanel"]')).toBeVisible()

    const ecart = await page.evaluate(() => {
      const conteneur = document.querySelector('.tabbed-page')
      const panel = document.querySelector('[role="tabpanel"]')
      if (!conteneur || !panel) return -1
      return Math.abs(conteneur.getBoundingClientRect().width - panel.getBoundingClientRect().width)
    })
    expect(ecart, `onglet ${onglet}`).toBeLessThanOrEqual(1)
  }
})
