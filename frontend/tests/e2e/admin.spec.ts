import { expect, test } from '@playwright/test'

/**
 * Page d'administration : les réglages sont écrits dans `config.yml` (via
 * `PUT /api/config`) et le backend les applique à chaud.
 *
 * Le backend des e2e travaille sur une configuration jetable (voir
 * `start-backend.mjs`) : ces tests modifient des réglages sans toucher à ceux du
 * dépôt.
 */

test('réglages : le re-scan périodique se règle, s’applique et se garde', async ({ page }) => {
  await page.goto('/admin')
  // Le `h1` est le titre de la page (le `document.title` reste vide, voir
  // `layouts/default.vue`).
  await expect(page.locator('h1')).toHaveText('Administration')

  const champ = page.getByPlaceholder('auto')
  // `/admin` n'est visitée par aucun autre test : sa première visite la fait
  // compiler par le serveur de dev, ce qui peut dépasser le délai d'assertion
  // courant (même parade que `upload.spec.ts`).
  await expect(champ).toBeVisible({ timeout: 30_000 })

  // Le backend explique sa recommandation : ici un dossier temporaire, sur un
  // disque local, donc aucun re-scan nécessaire.
  await expect(page.locator('.admin__hint--rec')).toContainText('temps réel')
  await expect(page.getByText('aucun re-scan (temps réel)')).toBeVisible()

  // 15 s : écrit dans le YAML, puis appliqué (la page suit le WebSocket).
  //
  // La saisie est réessayée : le serveur de dev compile la page à la première
  // visite, et une saisie arrivée avant l'hydratation ne déclenche rien.
  await expect(async () => {
    await champ.fill('15')
    await champ.press('Tab')
    await expect(page.getByText('re-scan toutes les 15 s')).toBeVisible({ timeout: 2000 })
  }).toPass({ timeout: 30_000 })

  // Un rechargement retrouve la valeur : elle vient du fichier de config.
  await page.reload()
  await expect(page.getByPlaceholder('auto')).toHaveValue('15')
  await expect(page.getByText('re-scan toutes les 15 s')).toBeVisible()

  // Une valeur choisie se retire par un bouton : le champ vide *est* le mode
  // automatique, il faut donc pouvoir y revenir sans le savoir.
  const auto = page.getByRole('button', { name: 'Automatique' })
  await expect(auto).toBeVisible()

  await auto.click()
  await expect(page.getByPlaceholder('auto')).toHaveValue('')
  await expect(page.getByText('aucun re-scan (temps réel)')).toBeVisible()
  // En automatique, il n'y a plus rien à retirer : le bouton disparaît.
  await expect(auto).toHaveCount(0)
})
