import { expect, test, type Page } from '@playwright/test'

/**
 * Envoi de fichiers par l'interface.
 *
 * Les deux chemins sont couverts : le **dépôt** (la fenêtre ouverte par le bouton
 * « Ajouter ») et le **secours** clavier, qui passe par les champs masqués.
 *
 * Le backend des e2e tourne sur des données jetables (voir `start-backend.mjs`) :
 * ces tests écrivent sur le disque sans toucher aux vrais modèles.
 */

/** Noms uniques : aucune dépendance à ce qu'une exécution précédente a laissé. */
const DEPOSE = 'e2e-depose.txt'
const PARCOURU = 'e2e-parcouru.txt'

/**
 * Ouvre la fenêtre d'ajout.
 *
 * Le clic est réessayé : le serveur de dev compile la page à la première visite,
 * et un clic arrivé avant l'hydratation ne déclenche rien (le bouton est rendu
 * par le serveur, mais son gestionnaire n'existe pas encore).
 */
async function ouvrirFenetre(page: Page) {
  const zone = page.locator('.dropzone')
  await expect(async () => {
    if (!(await zone.isVisible())) {
      await page.getByRole('button', { name: 'Ajouter' }).click()
    }
    await expect(zone).toBeVisible({ timeout: 2000 })
  }).toPass({ timeout: 30_000 })
}

test('ajout : dépôt d’un fichier dans la fenêtre', async ({ page }) => {
  await page.goto('/')
  await ouvrirFenetre(page)

  // La fenêtre annonce ce qu'elle accepte : fichiers **et** dossiers.
  const zone = page.locator('.dropzone')
  await expect(zone).toBeVisible()
  await expect(zone).toContainText('Déposez vos fichiers ou dossiers ici')
  await expect(zone).toContainText('Fichiers et dossiers acceptés')

  // Un vrai glisser-déposer n'est pas scriptable : on fabrique le `DataTransfer`
  // dans la page et on déclenche l'événement, comme le fait le navigateur.
  await page.evaluate((nom) => {
    const dt = new DataTransfer()
    dt.items.add(new File(['contenu e2e'], nom, { type: 'text/plain' }))
    document
      .querySelector('.dropzone')!
      .dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: dt }))
  }, DEPOSE)

  // Bilan en notification, et la fenêtre se referme.
  await expect(page.locator('[aria-label*="Notification"]')).toContainText('1 fichier ajouté')
  await expect(page.locator('.dropzone')).toBeHidden()

  // Le watcher rediffuse la liste : la carte apparaît sans rechargement.
  await expect(page.locator('.model-card__name', { hasText: DEPOSE })).toBeVisible()
})

test('ajout : le secours clavier envoie aussi', async ({ page }) => {
  await page.goto('/')
  await ouvrirFenetre(page)

  // Le champ masqué que le lien « Fichiers… » ouvre.
  await page.setInputFiles('input.add__input[multiple]', {
    name: PARCOURU,
    mimeType: 'text/plain',
    buffer: Buffer.from('contenu e2e'),
  })

  await expect(page.locator('[aria-label*="Notification"]')).toContainText('1 fichier ajouté')
  await expect(page.locator('.model-card__name', { hasText: PARCOURU })).toBeVisible()
})

test('ajout : un fichier refusé est signalé', async ({ page }) => {
  await page.goto('/')
  await ouvrirFenetre(page)

  // Un fichier ne peut pas porter le nom d'un dossier existant : le backend
  // refuse, et le motif doit remonter jusqu'à l'utilisateur.
  await page.evaluate(() => {
    const dt = new DataTransfer()
    dt.items.add(new File(['x'], 'DemaAuto', { type: 'text/plain' }))
    document
      .querySelector('.dropzone')!
      .dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: dt }))
  })

  const bilan = page.locator('[aria-label*="Notification"]')
  await expect(bilan).toContainText('1 échec')
  await expect(bilan).toContainText('un dossier porte déjà ce nom')
})
