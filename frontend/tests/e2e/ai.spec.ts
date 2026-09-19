import { expect, test } from '@playwright/test'

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

/** Carte « Recherche assistée » de l'administration. */
async function ouvrirCarteIA(page: import('@playwright/test').Page) {
  await page.goto('/admin')
  // `/admin` compilée à la première visite du serveur de dev : délai large,
  // comme dans `admin.spec.ts`.
  await expect(page.getByLabel('Fournisseur')).toBeVisible({ timeout: 30_000 })
}

/**
 * Choisit « Aucun » et attend que le backend l'ait **enregistré**.
 *
 * L'enregistrement part au changement du champ ; naviguer juste après
 * annulerait la requête en vol. La relecture après rechargement est la preuve
 * que la valeur vient bien du serveur.
 */
async function effacerFournisseur(page: import('@playwright/test').Page) {
  await expect(async () => {
    await page.getByLabel('Fournisseur').selectOption('')
    await page.reload()
    await expect(page.getByLabel('Fournisseur')).toHaveValue('', { timeout: 2000 })
  }).toPass({ timeout: 30_000 })
}

/**
 * Configure Ollama sur une adresse volontairement fermée.
 *
 * Aucun appel réseau réel n'a lieu : on vérifie le chemin d'échec et la qualité
 * du message, pas une réponse de modèle.
 */
async function configurerOllamaFerme(page: import('@playwright/test').Page) {
  const fournisseur = page.getByLabel('Fournisseur')
  const adresse = page.getByLabel('Adresse de l’API')

  await expect(async () => {
    await fournisseur.selectOption('ollama')
    await adresse.fill('http://127.0.0.1:9/v1')
    await adresse.press('Tab')
    await page.reload()
    await expect(fournisseur).toHaveValue('ollama', { timeout: 2000 })
    await expect(adresse).toHaveValue('http://127.0.0.1:9/v1', { timeout: 2000 })
  }).toPass({ timeout: 30_000 })
}

test('recherche IA : grisée tant qu’aucun fournisseur n’est configuré', async ({ page }) => {
  // La configuration peut garder un fournisseur d'une exécution précédente :
  // on part explicitement de « aucun ».
  await ouvrirCarteIA(page)
  await effacerFournisseur(page)

  await page.goto('/')
  const bouton = page.getByRole('button', { name: /IA/ })
  await expect(bouton).toBeDisabled()
  // L'infobulle dit où aller, et le conteneur la porte aussi (un bouton
  // désactivé ne déclenche pas toujours son propre `title`).
  await expect(bouton).toHaveAttribute('title', /administration/)
  await expect(page.locator('.ai-toggle')).toHaveAttribute('title', /administration/)

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
  await page.getByRole('button', { name: 'Tester' }).click()
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
