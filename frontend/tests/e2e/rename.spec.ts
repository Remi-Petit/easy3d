import { expect, test, type Page } from '@playwright/test'

/**
 * Menu contextuel des cartes : clic droit → « Renommer ».
 *
 * Le sujet du test est **fabriqué par le test** (envoi d'un petit fichier par
 * l'API, comme le fait la fenêtre d'ajout) puis remis sous son nom d'origine :
 * les autres tests partagent le même dossier de modèles jetable et ne doivent
 * pas voir passer ces fichiers.
 *
 * Les refus (nom déjà pris, nom impossible) sont couverts par les tests du
 * backend : ici, ce qui nous intéresse est le chemin complet — carte, menu,
 * fenêtre, et mise à jour de la liste sans rechargement.
 */

const DEPART = 'e2e-renomme.txt'
const ARRIVEE = 'e2e-renomme-2.txt'

/** Clic droit sur la carte portant ce nom, puis « Renommer » dans le menu. */
async function ouvrirRenommage(page: Page, nom: string) {
  const carte = page.locator('.model-card', { hasText: nom })
  await expect(carte).toBeVisible()
  await carte.click({ button: 'right' })
  await page.locator('.card-menu').getByRole('menuitem', { name: 'Renommer' }).click()
  await expect(page.locator('.rename__input')).toBeVisible()
}

/** Saisit un nouveau nom et valide la fenêtre. */
async function renommer(page: Page, nom: string) {
  await page.locator('.rename__input').fill(nom)
  await page.locator('.rename__confirm').click()
}

test('carte : le clic droit propose de renommer', async ({ page }) => {
  await page.goto('/')

  // Le sujet : un fichier à la racine, envoyé par l'API (aucun événement pour
  // le watcher ici — c'est le backend qui rediffuse après son écriture).
  const status = await page.evaluate(async (nom) => {
    const res = await fetch(`/api/upload?path=${encodeURIComponent(nom)}`, {
      method: 'POST',
      body: 'sujet du test de renommage',
    })
    return res.status
  }, DEPART)
  expect(status).toBe(201)

  await ouvrirRenommage(page, DEPART)

  // Le champ propose le nom actuel : valider tel quel ne changerait rien.
  await expect(page.locator('.rename__input')).toHaveValue(DEPART)

  await renommer(page, ARRIVEE)

  // La carte change de nom **sans rechargement** : la liste vient du WebSocket.
  await expect(page.locator('.model-card', { hasText: ARRIVEE })).toBeVisible()
  await expect(page.locator('.model-card', { hasText: DEPART })).toHaveCount(0)

  // Remise en place : le dossier des modèles est partagé par les autres tests.
  await ouvrirRenommage(page, ARRIVEE)
  await renommer(page, DEPART)
  await expect(page.locator('.model-card', { hasText: DEPART })).toBeVisible()

  // Échap referme le menu, sans rien renommer.
  await page.locator('.model-card', { hasText: DEPART }).click({ button: 'right' })
  await expect(page.locator('.card-menu')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.card-menu')).toHaveCount(0)
})

/**
 * Le clic droit vaut aussi pour un dossier, et la fenêtre reçoit le **chemin
 * relatif** — pas seulement le nom, sinon un sous-dossier renommé depuis une
 * page serait cherché à la racine.
 *
 * Le renommage lui-même n'est pas fait ici : les dossiers du dossier de modèles
 * des e2e servent aux autres tests. On annule.
 */
test('carte dossier : le clic droit propose de renommer', async ({ page }) => {
  await page.goto('/dossiers/DemaAuto')

  // Un sous-dossier : son nom seul (`sous-structure`) ne suffit pas à le
  // retrouver, d'où le chemin complet attendu dans la fenêtre.
  const carte = page.locator('.model-card', { hasText: 'sous-structure' })
  await expect(carte).toBeVisible()
  await carte.click({ button: 'right' })
  await page.locator('.card-menu').getByRole('menuitem', { name: 'Renommer' }).click()

  await expect(page.locator('.rename__input')).toHaveValue('sous-structure')
  await expect(page.getByText('DemaAuto/sous-structure')).toBeVisible()

  await page.locator('.rename__cancel').click()
  await expect(page.locator('.rename__input')).toHaveCount(0)
  // Rien n'a bougé : le dossier est toujours à sa place.
  await expect(carte).toBeVisible()
})
