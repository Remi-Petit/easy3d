import { expect, test, type Page } from '@playwright/test'

/**
 * Recherche assistée (IA).
 *
 * Le backend des e2e travaille sur une configuration jetable : on peut y
 * enregistrer un fournisseur sans toucher à celle du dépôt.
 *
 * Aucun appel réseau réel n'a lieu : le fournisseur configuré est un Ollama
 * volontairement pointé vers une adresse fermée. On vérifie donc l'état de
 * l'interface et la **qualité du message d'échec**, pas une réponse de modèle.
 */

/**
 * Le sélecteur de fournisseur, **dans la carte IA**.
 *
 * Visé par son `id` et non par son libellé : l'administration porte désormais
 * aussi une case « Autoriser la connexion par le fournisseur d'identité », et
 * `getByLabel('Fournisseur')` — qui cherche par sous-chaîne — trouve alors deux
 * éléments et échoue en mode strict.
 */
const champFournisseur = (page: Page) => page.locator('#ai-provider')

/**
 * La carte « Recherche assistée » de l'administration.
 *
 * Délimitée par le champ qu'elle contient : depuis l'arrivée du SSO, une autre
 * carte porte elle aussi un bouton « Tester » (le contrôle du fournisseur
 * d'identité), et viser le bouton par son seul libellé devient ambigu.
 */
const carteIA = (page: Page) =>
  page.locator('.admin__card').filter({ has: page.locator('#ai-provider') })

/** Le bouton « Tester » **de la carte IA** (et non celui du SSO). */
const boutonTester = (page: Page) => carteIA(page).getByRole('button', { name: 'Tester' })

/** Carte « Recherche assistée » de l'administration. */
async function ouvrirCarteIA(page: Page) {
  await page.goto('/admin')
  // `/admin` compilée à la première visite du serveur de dev : délai large,
  // comme dans `admin.spec.ts`.
  await expect(champFournisseur(page)).toBeVisible({ timeout: 30_000 })
  // Le modèle n'est plus un champ de saisie : c'est une liste de choix, remplie
  // par ce que le fournisseur annonce.
  await expect(page.locator('select#ai-model')).toBeVisible()
}

/**
 * Enregistre la carte IA (bouton « Enregistrer ») et attend l'accusé du backend.
 *
 * Rien n'est écrit au changement de champ, contrairement aux réglages
 * d'affichage : la clé ne part qu'ici.
 */
async function enregistrerIA(page: Page) {
  await page.getByRole('button', { name: 'Enregistrer' }).first().click()
  await expect(page.locator('.admin__status--ok').first()).toBeVisible({ timeout: 10_000 })
}

/**
 * Choisit « Aucun » et l'enregistre : la recherche assistée redevient grisée.
 *
 * La relecture après rechargement est la preuve que la valeur vient du serveur —
 * naviguer juste après le clic annulerait la requête en vol.
 */
async function effacerFournisseur(page: Page) {
  await expect(async () => {
    await champFournisseur(page).selectOption('')
    await enregistrerIA(page)
    await page.reload()
    await expect(champFournisseur(page)).toHaveValue('', { timeout: 2000 })
  }).toPass({ timeout: 30_000 })
}

/**
 * Configure Ollama sur une adresse volontairement fermée.
 *
 * « Tester » échoue alors en expliquant la cause — c'est aussi lui qui remplit
 * la liste des modèles quand le fournisseur répond.
 */
async function configurerOllamaFerme(page: Page) {
  const fournisseur = champFournisseur(page)
  const adresse = page.getByLabel('Adresse de l’API')

  await expect(async () => {
    await fournisseur.selectOption('ollama')
    await adresse.fill('http://127.0.0.1:9/v1')

    await boutonTester(page).click()
    await expect(page.locator('.admin__status--err')).toContainText('impossible', {
      timeout: 20_000,
    })

    await enregistrerIA(page)
    await page.reload()
    await expect(fournisseur).toHaveValue('ollama', { timeout: 2000 })
    await expect(adresse).toHaveValue('http://127.0.0.1:9/v1', { timeout: 2000 })
  }).toPass({ timeout: 60_000 })
}

/**
 * Adresses connues du fournisseur : un clic remplit le champ.
 *
 * Rien n'est enregistré ici (la carte IA n'écrit qu'au clic sur « Enregistrer ») :
 * le test peut donc choisir un fournisseur sans toucher à la configuration
 * partagée par les autres tests.
 */
test('recherche IA : une adresse connue remplit le champ, qui reste libre', async ({ page }) => {
  await ouvrirCarteIA(page)
  await champFournisseur(page).selectOption('openai')

  // La liste vient du backend : l'interface n'a aucune adresse en dur.
  const puces = page.getByRole('group', { name: 'Adresses connues' })
  await expect(puces.getByRole('button', { name: 'DeepSeek' })).toBeVisible()
  await expect(puces.getByRole('button', { name: 'OpenRouter' })).toBeVisible()

  await puces.getByRole('button', { name: 'DeepSeek' }).click()

  const adresse = page.getByLabel('Adresse de l’API')
  await expect(adresse).toHaveValue('https://api.deepseek.com/v1')
  // La puce choisie est marquée, et elle seule.
  await expect(puces.locator('.admin__choice--on')).toHaveCount(1)

  // Le point d'entrée propose aussi son modèle : il sera remplacé par ce que le
  // fournisseur annonce au premier « Tester ». L'attendu est lu **à la source**
  // (`ai-presets.yml`, via l'API) : ce fichier est fait pour être modifié, un
  // test ne doit donc pas figer un nom de modèle.
  const suggere = await page.evaluate(async () => {
    const providers = await fetch('/api/ai/providers').then((r) => r.json())
    const openai = providers.find((p) => p.id === 'openai')
    return openai.presets.find((p) => p.label === 'DeepSeek')?.model ?? ''
  })
  expect(suggere).not.toBe('')
  await expect(page.locator('select#ai-model')).toHaveValue(suggere)

  // Le champ reste maître : une adresse intermédiaire n'appartient à aucune
  // puce, donc plus rien n'est marqué.
  await adresse.fill('https://proxy.interne/v1')
  await expect(puces.locator('.admin__choice--on')).toHaveCount(0)
})

test('recherche IA : grisée tant qu’aucun fournisseur n’est configuré', async ({ page }) => {
  // La configuration peut garder un fournisseur d'une exécution précédente :
  // on part explicitement de « aucun ».
  await ouvrirCarteIA(page)
  await effacerFournisseur(page)

  await page.goto('/')
  const bouton = page.getByRole('button', { name: /IA/ })
  await expect(bouton).toBeDisabled()

  // L'explication vit dans une infobulle maison : masquée au repos, révélée au
  // survol, et rattachée au bouton pour les lecteurs d'écran.
  const bulle = page.locator('#ai-btn-help')
  await expect(bulle).toHaveText(/administration/)
  await expect(bouton).toHaveAttribute('aria-describedby', 'ai-btn-help')
  await expect(bulle).toHaveCSS('opacity', '0')
  await page.locator('.ai-toggle').hover()
  await expect(bulle).toHaveCSS('opacity', '1')

  // Le catalogue est bien là : pas de panneau de recherche assistée.
  await expect(page.locator('.ai')).toHaveCount(0)
  await expect(page.locator('.file-grid').first()).toBeVisible()
})

test('recherche IA : configurée, elle s’ouvre et explique l’échec du fournisseur', async ({
  page,
}) => {
  await ouvrirCarteIA(page)
  await configurerOllamaFerme(page)

  // Le bouton de la barre de recherche suit la configuration enregistrée.
  await page.goto('/')
  await expect(page.getByRole('button', { name: /IA/ })).toBeEnabled()

  // « Tester » remonte la cause exacte, pas un statut nu.
  await ouvrirCarteIA(page)
  await boutonTester(page).click()
  await expect(page.locator('.admin__status--err')).toContainText('impossible', {
    timeout: 30_000,
  })

  // La recherche assistée ouvre son panneau à la place du catalogue.
  await page.goto('/')
  await page.getByRole('button', { name: /IA/ }).click()
  const panneau = page.locator('.ai')
  await expect(panneau).toBeVisible()
  await expect(panneau).toContainText('le modèle fouille les noms')

  const champ = page.getByPlaceholder(/Décrivez ce que vous cherchez/)
  await champ.fill('une pièce en PETG')
  await champ.press('Enter')
  await expect(page.locator('.ai__error')).toContainText('impossible', { timeout: 30_000 })

  // Quitter la recherche assistée rend le catalogue.
  await page.getByRole('button', { name: /IA/ }).click()
  await expect(page.locator('.ai')).toHaveCount(0)
  await expect(page.locator('.file-grid').first()).toBeVisible()
})

test('recherche IA : changer de page referme le panneau', async ({ page }) => {
  await ouvrirCarteIA(page)
  await configurerOllamaFerme(page)

  // Panneau ouvert sur le catalogue : il recouvre la page **sans la démonter** —
  // `v-show`, et non `v-if`, dans `layouts/default.vue` — parce que démonter la
  // page gelait la navigation (Nuxt n'y rafraîchit la route du layout qu'au
  // rendu de la page).
  await page.goto('/')
  await page.getByRole('button', { name: /IA/ }).click()
  await expect(page.locator('.ai')).toBeVisible()
  await expect(page.locator('.page')).toBeHidden()

  // La barre latérale est le seul chemin qui reste cliquable pendant que le
  // panneau recouvre la liste : c'est par elle qu'on change de page. Le mode est
  // un état partagé (voir `useAiSearch`) : sans garde, le panneau suivait la
  // navigation et masquait la page demandée.
  await page.getByRole('link', { name: 'Administration' }).click()
  await expect(page.locator('.page')).toBeVisible()
  await expect(page.locator('.ai')).toHaveCount(0)
  await expect(page.getByRole('heading', { name: 'Administration', level: 1 })).toBeVisible()

  // Même chose depuis un dossier, où la barre du catalogue est toujours là
  // (le dossier s'appelle `name`, seul un fichier occupe `rel`) : c'est là qu'on
  // poserait une question avant de revenir au catalogue.
  await page.getByRole('link', { name: 'Modèles' }).click()
  const dossier = page.locator('a[href*="/folders/"]').first()
  await expect(dossier).toBeVisible()
  await dossier.click()
  await expect(page.locator('a[href*="/files/"]').first()).toBeVisible()

  await page.getByRole('button', { name: /IA/ }).click()
  await expect(page.locator('.ai')).toBeVisible()
  await page.getByRole('link', { name: 'Modèles' }).click()
  await expect(page.locator('a[href*="/folders/"]').first()).toBeVisible()
  await expect(page.locator('.ai')).toHaveCount(0)
})
