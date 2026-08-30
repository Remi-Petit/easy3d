import { expect, test } from '@playwright/test'

test('accueil : dossiers et fichiers racine listés', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'easy3d' })).toBeVisible()
  // Au moins un dossier (DemaAuto / Maison) est affiché.
  await expect(page.locator('a[href*="/dossiers/"]').first()).toBeVisible()
  // Au moins un fichier racine est listé.
  await expect(page.locator('a[href*="/fichier/"]').first()).toBeVisible()
})

test('dossier : la navigation vers le détail liste ses fichiers', async ({ page }) => {
  await page.goto('/')
  const folderLink = page.locator('a[href*="/dossiers/"]').first()
  await expect(folderLink).toBeVisible()
  await folderLink.click()
  // Le titre devient le nom du dossier (plus "easy3d").
  await expect(page.locator('h1')).not.toHaveText('easy3d')
  // Les fichiers du dossier sont listés et cliquables.
  await expect(page.locator('a[href*="/fichier/"]').first()).toBeVisible()
})

test('fichier : le détail charge le viewer et la table de méta', async ({ page }) => {
  await page.goto('/')
  const fileLink = page.locator('a[href*="/fichier/"]').first()
  await expect(fileLink).toBeVisible()
  await fileLink.click()
  await expect(page.locator('.viewer-card')).toBeVisible()
  await expect(page.locator('.meta-table')).toBeVisible()
})
