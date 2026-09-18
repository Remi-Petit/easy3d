import { expect, test, type Page } from '@playwright/test'

/**
 * Suppression par le menu contextuel : clic droit → « Supprimer » → confirmation.
 *
 * Contrairement au renommage, rien n'est à remettre en place : le sujet du test
 * est le fichier supprimé. Il est fabriqué par le test (envoi par l'API), ce qui
 * garantit qu'il ne touche à aucun des éléments partagés par les autres tests.
 */

const FICHIER = 'e2e-supprime.txt'
const DOSSIER = 'e2e-supprime-dossier'

/** Clic droit sur la carte portant ce nom, puis « Supprimer » dans le menu. */
async function ouvrirSuppression(page: Page, nom: string) {
  const carte = page.locator('.model-card', { hasText: nom })
  await expect(carte).toBeVisible()
  await carte.click({ button: 'right' })
  await page.locator('.card-menu').getByRole('menuitem', { name: 'Supprimer' }).click()
  await expect(page.locator('.confirm__confirm')).toBeVisible()
}

/** Dépose un fichier dans le dossier des modèles, via l'API. */
async function deposer(page: Page, rel: string) {
  const status = await page.evaluate(async (chemin) => {
    const res = await fetch(`/api/upload?path=${encodeURIComponent(chemin)}`, {
      method: 'POST',
      body: 'sujet du test de suppression',
    })
    return res.status
  }, rel)
  expect(status).toBe(201)
}

test('carte : le clic droit propose de supprimer un fichier', async ({ page }) => {
  await page.goto('/')
  await deposer(page, FICHIER)

  await ouvrirSuppression(page, FICHIER)

  // L'avertissement dit ce qui part, et « Annuler » ne supprime rien.
  await expect(page.locator('.confirm__warning')).toContainText(FICHIER)
  await page.locator('.confirm__cancel').click()
  await expect(page.locator('.model-card', { hasText: FICHIER })).toBeVisible()

  // Confirmation : la carte disparaît **sans rechargement** (rediffusion du
  // backend).
  await ouvrirSuppression(page, FICHIER)
  await page.locator('.confirm__confirm').click()
  await expect(page.locator('.model-card', { hasText: FICHIER })).toHaveCount(0)
})

test('carte dossier : la suppression annonce ce qu’elle emporte', async ({ page }) => {
  await page.goto('/')
  await deposer(page, `${DOSSIER}/piece.stl`)
  await deposer(page, `${DOSSIER}/vis.stl`)

  await ouvrirSuppression(page, DOSSIER)

  // Le dossier part avec ses fichiers : c'est annoncé avant de confirmer.
  await expect(page.locator('.confirm__warning')).toContainText('2 fichiers')

  await page.locator('.confirm__confirm').click()

  // La carte du dossier disparaît, et le dossier lui-même est bien parti.
  await expect(page.locator('.model-card', { hasText: DOSSIER })).toHaveCount(0)
  const status = await page.evaluate(async (nom) => {
    const res = await fetch(`/api/models`)
    const body = await res.json()
    return Object.keys(body.folders ?? {}).includes(nom) ? 200 : 404
  }, DOSSIER)
  expect(status).toBe(404)
})
