import { expect, test, type Page } from '@playwright/test'
import {
  ADMIN_EMAIL,
  ADMIN_PASSWORD,
  ADMIN_USERNAME,
  KC_ADMIN_LOGIN,
  KC_ADMIN_PASSWORD,
  KC_IMPRIMEUSE,
  KC_OBSERVATRICE,
  KC_SUPERUSER,
  KC_USER_EMAIL,
  KC_USER_LOGIN,
  KC_USER_PASSWORD,
  KC_USERNAME,
} from './params'

/**
 * SSO de bout en bout contre un **vrai** fournisseur OIDC (Keycloak).
 *
 * `tests/e2e-auth/` éprouve déjà la chaîne complète, mais contre
 * `docker/fake-oidc` : un fournisseur qui redirige sans rien demander. Ici, la
 * page de connexion, les mots de passe, la session du fournisseur et les
 * revendications de `userinfo` sont **réels** — c'est le cas concret, et c'est
 * là que se cachent les différences entre la spécification et un produit
 * (nom des champs, adresse annoncée, contenu de `userinfo`).
 *
 * Les tests s'exécutent **dans l'ordre** (une base de comptes partagée : le
 * journal et le décompte des comptes en dépendent). Chacun part d'un navigateur
 * neuf — donc sans cookie de session, ni côté easy3d ni côté Keycloak.
 */

/** Remplit la page de connexion **de Keycloak**, puis valide. */
async function saisirChezKeycloak(page: Page, identifiant: string, motDePasse: string) {
  // Les sélecteurs `name=` survivent au thème : ils font partie du formulaire
  // OIDC, pas de sa mise en page (les identifiants `#username`/`#kc-login` ont
  // déjà changé une fois entre deux thèmes de Keycloak).
  await expect(page.locator('input[name="username"]')).toBeVisible()
  await page.locator('input[name="username"]').fill(identifiant)
  await page.locator('input[name="password"]').fill(motDePasse)
  await page.locator('button[type="submit"], input[type="submit"]').first().click()
}

/** Clique le bouton SSO de la page de connexion et vérifie qu'on est chez Keycloak. */
async function ouvrirKeycloak(page: Page) {
  await page.goto('/login')
  await page.getByRole('link', { name: /Continuer avec/ }).click()
  await expect(page).toHaveURL(/\/realms\/easy3d\/protocol\/openid-connect\/auth/, {
    timeout: 30_000,
  })
}

/** Connexion complète par le fournisseur, jusqu'au retour dans l'application. */
async function seConnecterParSso(page: Page, identifiant: string, motDePasse: string) {
  await ouvrirKeycloak(page)
  await saisirChezKeycloak(page, identifiant, motDePasse)
}

test('le bouton SSO mène à la page de connexion de Keycloak', async ({ page }) => {
  await page.goto('/login')

  // Le libellé porte l'hôte du fournisseur, annoncé par `/auth/me`.
  const bouton = page.getByRole('link', { name: /Continuer avec/ })
  await expect(bouton).toContainText('127.0.0.1')
  await bouton.click()

  // On a **quitté** l'application : c'est Keycloak qui sert la page, et elle
  // demande un mot de passe (ce que le fournisseur fictif ne fait jamais).
  await expect(page).toHaveURL(/\/realms\/easy3d\/protocol\/openid-connect\/auth/, {
    timeout: 30_000,
  })
  await expect(page.locator('input[name="username"]')).toBeVisible()
  await expect(page.locator('input[name="password"]')).toBeVisible()
  await expect(page.locator('button[type="submit"], input[type="submit"]')).toHaveCount(1)
})

test('un compte local est rattaché par son e-mail, sans doublon', async ({ page }) => {
  await seConnecterParSso(page, KC_ADMIN_LOGIN, KC_ADMIN_PASSWORD)

  // Le retour du fournisseur rouvre la page demandée (`/`) avec une **session
  // locale** : le compte est celui créé au démarrage, pas un second compte.
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
  await expect(page.locator('.sidebar-user__who')).toHaveText(ADMIN_USERNAME)
  await expect(page.locator('.sidebar-user__who')).toHaveAttribute('title', ADMIN_EMAIL)

  // Vue de l'administrateur : l'adresse n'apparaît qu'une fois, et la ligne
  // porte la pastille « SSO ». Deux comptes pour un même e-mail signifieraient
  // que le rattachement n'a pas eu lieu.
  await page.goto('/admin/accounts')
  await expect(page.locator('h1')).toHaveText('Comptes', { timeout: 30_000 })
  const ligne = page.locator('.data-grid__row', { hasText: ADMIN_EMAIL })
  await expect(ligne).toHaveCount(1)
  await expect(ligne.first()).toContainText('SSO')

  // Le journal dit **comment** ce compte a été obtenu : un compte local déjà
  // présent, rattaché à l'identité du fournisseur par son adresse (mode
  // `manual`), et non une création.
  await page.goto('/admin/audit')
  await expect(page.locator('h1')).toHaveText('Journal', { timeout: 30_000 })
  const journal = page.locator('.audit__table tbody tr').first()
  await expect(journal).toContainText('Connexion SSO')
  await expect(journal).toContainText('compte préparé')
})

test('un compte inconnu est créé d’office, avec le rôle livré', async ({ page }) => {
  await seConnecterParSso(page, KC_USER_LOGIN, KC_USER_PASSWORD)

  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
  // Le nom vient de `preferred_username` chez Keycloak.
  await expect(page.locator('.sidebar-user__who')).toHaveText(KC_USERNAME)

  // Le rôle livré `lecteur` ouvre le catalogue, pas l'administration : la
  // garde renvoie au catalogue.
  await page.goto('/admin/accounts')
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })

  // Et « mon compte » sait que ce compte se gère chez le fournisseur : les
  // trois champs de mot de passe sont inertes.
  await page.goto('/account')
  await expect(page.locator('h1')).toHaveText('Mon compte')
  await expect(page.locator('.admin__applied')).toContainText(KC_USER_EMAIL)
  const champs = page.locator('input[type="password"]')
  await expect(champs).toHaveCount(3)
  for (const champ of await champs.all()) await expect(champ).toBeDisabled()
})

test('un mot de passe erroné chez le fournisseur ne crée aucune session', async ({ page }) => {
  await ouvrirKeycloak(page)
  await saisirChezKeycloak(page, KC_USER_LOGIN, 'ce-n-est-pas-le-bon')

  // Keycloak refuse **chez lui** : on reste sur sa page, avec son message (dans
  // le thème actuel, l'erreur est portée par le champ de l'identifiant).
  const erreur = page
    .locator('#input-error-username, [role="alert"], .pf-v5-c-alert')
    .first()
  await expect(erreur).toBeVisible({ timeout: 20_000 })
  await expect(erreur).toContainText(/invalid|incorrect|identifiant|mot de passe/i)
  // On reste **chez le fournisseur** : la page de connexion renvoie sur sa
  // propre action (`login-actions/authenticate`), jamais sur l'application.
  await expect(page).toHaveURL(
    /\/realms\/easy3d\/(?:protocol\/openid-connect\/auth|login-actions\/authenticate)/,
  )

  // Aucun retour dans l'application, aucun cookie de session : le catalogue
  // redemande la connexion.
  await page.goto('/')
  await expect(page).toHaveURL((url) => url.pathname === '/login', { timeout: 30_000 })
})

test('après connexion, on revient sur la page demandée', async ({ page }) => {
  await page.goto('/admin/audit')
  await expect(page).toHaveURL((url) => url.pathname === '/login', { timeout: 30_000 })
  // Le chemin demandé voyage dans l'URL de la page de connexion, puis dans
  // l'état de la connexion : c'est ce que le fournisseur ne voit jamais.
  expect(new URL(page.url()).searchParams.get('redirect')).toBe('/admin/audit')

  await page.getByRole('link', { name: /Continuer avec/ }).click()
  await saisirChezKeycloak(page, KC_ADMIN_LOGIN, KC_ADMIN_PASSWORD)

  await expect(page).toHaveURL((url) => url.pathname === '/admin/audit', { timeout: 30_000 })
  await expect(page.locator('h1')).toHaveText('Journal', { timeout: 30_000 })
})

test('une reconnexion SSO ne se lit pas comme une création', async ({ page }) => {
  // Le compte `admin` est déjà rattaché (test 2) : la trace doit dire la
  // connexion, et non recopier le mode de provisionnement configuré. C'est un
  // vrai défaut trouvé par cette campagne — chaque connexion s'affichait
  // « création automatique », y compris quand rien n'avait été créé.
  await seConnecterParSso(page, KC_ADMIN_LOGIN, KC_ADMIN_PASSWORD)

  await page.goto('/admin/audit')
  await expect(page.locator('h1')).toHaveText('Journal', { timeout: 30_000 })
  const premiere = page.locator('.audit__table tbody tr').first()
  await expect(premiere).toContainText('Connexion SSO')
  await expect(premiere).not.toContainText('création automatique')
})

test('la session reste locale : un cookie HttpOnly, pas un jeton du fournisseur', async ({
  page,
  context,
}) => {
  await seConnecterParSso(page, KC_ADMIN_LOGIN, KC_ADMIN_PASSWORD)
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })

  const cookies = await context.cookies()
  // Le cookie d'easy3d : opaque, inaccessible au script, `SameSite=Lax` (c'est
  // lui qui protège du CSRF, le backend ne compare pas les `Origin`).
  const session = cookies.filter((cookie) => cookie.name === 'easy3d_session')
  expect(session).toHaveLength(1)
  expect(session[0].httpOnly).toBe(true)
  expect(session[0].sameSite).toBe('Lax')

  // Les cookies de Keycloak vivent sur **son** origine, pas sur celle de
  // l'application : le navigateur ne les envoie jamais à easy3d.
  const fournisseur = cookies.filter((cookie) => cookie.name.startsWith('KEYCLOAK_'))
  expect(fournisseur.length).toBeGreaterThan(0)
  for (const cookie of fournisseur) expect(cookie.domain).not.toBe('localhost')
})

test('le mot de passe local reste valable après un rattachement SSO (porte de secours admin)', async ({
  page,
}) => {
  // L'administrateur local a été rattaché au compte Keycloak par le test 2. Un
  // administrateur rattaché garde l'entrée par mot de passe : c'est la porte de
  // secours de l'installation si le fournisseur tombe.
  await page.goto('/login')
  // Même course à l'hydratation que dans la campagne `e2e-auth` : on resaisit
  // jusqu'à ce que le bouton s'active (il est désactivé tant qu'un champ est vide).
  const bouton = page.getByRole('button', { name: 'Se connecter' })
  await expect(async () => {
    await page.getByLabel('Identifiant ou e-mail').fill(ADMIN_EMAIL)
    await page.getByLabel('Mot de passe').fill(ADMIN_PASSWORD)
    await expect(bouton).toBeEnabled({ timeout: 1000 })
  }).toPass({ timeout: 30_000 })
  await bouton.click()

  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
  await expect(page.locator('.sidebar-user__who')).toHaveText(ADMIN_USERNAME)
})

test('le compte SSO n’ouvre pas les jetons d’API d’un autre compte', async ({ page }) => {
  // Le compte `user` est propre au fournisseur : aucune session locale ne
  // traîne, et son onglet « API » ne montre que ses propres jetons.
  await seConnecterParSso(page, KC_USER_LOGIN, KC_USER_PASSWORD)
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })

  await page.goto('/account/api')
  await expect(page.locator('h1')).toHaveText('Mon compte')
  await expect(page.locator('.admin__count').first()).toHaveText('0')
})

test('le refus du fournisseur est traduit pour l’utilisateur', async ({ page }) => {
  // `?error=` posé par le retour du fournisseur : le backend n'ouvre aucune
  // session et la page de connexion affiche un code traduit (i18n), jamais une
  // phrase anglaise venue de Keycloak.
  await page.goto('/login?error=oidc_denied')
  await expect(page.getByRole('alert')).toContainText(
    'La connexion a été refusée chez le fournisseur d’identité.',
  )
})

// ── Plusieurs comptes, plusieurs droits ──────────────────────────────────
//
// Le SSO ne donne **que l'identité** : les droits restent dans le RBAC local
// (voir `README.md`). Ces trois tests le vérifient dans les deux sens — un rôle
// porté chez Keycloak ne donne rien ici, et un rôle posé ici s'applique
// immédiatement à une session ouverte par le fournisseur.

/** Rôle **local** créé par le test : catalogue + envoi, pas la suppression. */
const ROLE_IMPRIMEUR = 'imprimeur-e2e'

/** Dépose un fichier dans le catalogue par l'API (même geste que les e2e du catalogue). */
async function deposer(page: Page, rel: string): Promise<number> {
  return page.evaluate(async (chemin) => {
    const res = await fetch(`/api/upload?path=${encodeURIComponent(chemin)}`, {
      method: 'POST',
      body: 'contenu du test SSO',
    })
    return res.status
  }, rel)
}

/** Rôles locaux du compte connecté, tels que le serveur les annonce. */
async function rolesDe(page: Page): Promise<string[]> {
  const me = (await (await page.request.get('/api/auth/me')).json()) as {
    user?: { roles?: string[] }
  }
  return me.user?.roles ?? []
}

test('des droits différents selon le compte, appliqués sans reconnexion', async ({ browser }) => {
  // 1. Deux comptes Keycloak entrent : *imprimeuse* et *observatrice* n'existent
  //    pas côté easy3d, le provisionnement automatique les crée avec le rôle
  //    livré `lecteur` — donc **les mêmes droits** au départ.
  const sessionImpr = await browser.newContext()
  const pageImpr = await sessionImpr.newPage()
  await seConnecterParSso(pageImpr, KC_IMPRIMEUSE.login, KC_IMPRIMEUSE.motDePasse)
  const sessionObs = await browser.newContext()
  const pageObs = await sessionObs.newPage()
  await seConnecterParSso(pageObs, KC_OBSERVATRICE.login, KC_OBSERVATRICE.motDePasse)

  expect(await rolesDe(pageImpr)).toEqual(['lecteur'])
  expect(await rolesDe(pageObs)).toEqual(['lecteur'])
  // Envoyer un fichier demande `model.upload` : refusé pour l'instant.
  expect(await deposer(pageImpr, 'e2e-avant-role.txt')).toBe(403)

  // 2. L'administrateur crée un rôle métier et le pose sur **la seule**
  //    imprimeuse. (Le rôle porte les droits que le fournisseur n'a pas à
  //    connaître.)
  const sessionAdmin = await browser.newContext()
  const pageAdmin = await sessionAdmin.newPage()
  await seConnecterParSso(pageAdmin, KC_ADMIN_LOGIN, KC_ADMIN_PASSWORD)

  const creation = await pageAdmin.request.post('/api/roles', {
    data: {
      name: ROLE_IMPRIMEUR,
      description: 'Catalogue et envoi de fichiers (rôle de démonstration SSO)',
      permissions: ['catalog.read', 'model.upload'],
    },
  })
  expect([201, 409], `création du rôle (${creation.status()})`).toContain(creation.status())

  // ⚠️ `PUT /users/{uuid}` attend des **UUID** de rôles (c'est ce que
  // l'interface coche), pas des noms : on relit la liste plutôt que de deviner.
  const roles = (await (await pageAdmin.request.get('/api/roles')).json()) as {
    roles: Array<{ uuid: string; name: string; permissions: string[] }>
  }
  const role = roles.roles.find((candidat) => candidat.name === ROLE_IMPRIMEUR)
  expect(role, 'rôle créé').toBeTruthy()
  expect(role!.permissions.sort()).toEqual(['catalog.read', 'model.upload'])

  const comptes = (await (await pageAdmin.request.get('/api/users')).json()) as {
    users: Array<{ uuid: string; username: string }>
  }
  const cible = comptes.users.find((compte) => compte.username === KC_IMPRIMEUSE.login)
  expect(cible, 'le compte provisionné apparaît dans Administration → Comptes').toBeTruthy()
  const pose = await pageAdmin.request.put(`/api/users/${cible!.uuid}`, {
    data: { roles: [role!.uuid] },
  })
  expect(pose.status()).toBe(200)

  // 3. **Sans se reconnecter** : les droits sont relus à chaque requête, donc la
  //    session déjà ouverte par Keycloak en profite tout de suite. L'autre
  //    compte, lui, ne gagne rien.
  expect(await rolesDe(pageImpr)).toEqual([ROLE_IMPRIMEUR])
  expect(await deposer(pageImpr, 'e2e-apres-role.txt')).toBe(201)
  expect(await deposer(pageObs, 'e2e-refus-observatrice.txt')).toBe(403)

  // Le rôle ne donne pas `model.delete` : la suppression reste refusée.
  const suppression = await pageImpr.request.post('/api/delete', {
    data: { path: 'e2e-apres-role.txt' },
  })
  expect(suppression.status()).toBe(403)

  // Et l'administration reste fermée aux deux (pas de `users.write`).
  await pageImpr.goto('/admin/accounts')
  await expect(pageImpr).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
})

test('un compte au rôle livré ne peut que consulter', async ({ page }) => {
  // *observatrice* est restée au rôle par défaut : catalogue, et rien d'autre.
  await seConnecterParSso(page, KC_OBSERVATRICE.login, KC_OBSERVATRICE.motDePasse)
  await expect(page.locator('h1')).toContainText('Models')

  expect(await deposer(page, 'e2e-refus-lecteur.txt')).toBe(403)
  expect((await page.request.post('/api/delete', { data: { path: 'piece.stl' } })).status()).toBe(403)
  expect((await page.request.post('/api/rename', { data: { path: 'piece.stl', name: 'x.stl' } })).status()).toBe(403)
  expect((await page.request.get('/api/users')).status()).toBe(403)
  expect((await page.request.get('/api/journal')).status()).toBe(403)

  // Le fichier existe toujours : un refus ne doit rien avoir entamé.
  const liste = (await (await page.request.get('/api/models')).json()) as {
    files?: Array<{ name?: string; rel?: string }>
  }
  expect(JSON.stringify(liste)).toContain('piece')
})

test('un rôle porté chez Keycloak ne donne aucun droit dans easy3d', async ({ page }) => {
  // *superuser* porte le rôle **admin chez Keycloak** : c'est l'annuaire qui le
  // dit, et easy3d ne le lit pas. La connexion réussit (l'identité est bonne),
  // mais le compte local naît avec le rôle livré — aucune remontée de
  // privilège par le fournisseur, même quand celui-ci s'appelle Keycloak.
  await seConnecterParSso(page, KC_SUPERUSER.login, KC_SUPERUSER.motDePasse)
  await expect(page.locator('.sidebar-user__who')).toHaveText(KC_SUPERUSER.login)

  expect(KC_SUPERUSER.rolesKeycloak).toContain('admin')
  expect(await rolesDe(page)).toEqual(['lecteur'])
  expect((await page.request.get('/api/users')).status()).toBe(403)
  expect((await page.request.get('/api/roles')).status()).toBe(403)
  await page.goto('/admin/accounts')
  await expect(page).toHaveURL((url) => url.pathname === '/', { timeout: 30_000 })
})
