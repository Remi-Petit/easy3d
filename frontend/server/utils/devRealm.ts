import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/**
 * Fournisseur d'identité **de développement** : les comptes du royaume Keycloak.
 *
 * La page `/dev/login` propose de choisir un compte puis démarre un vrai flux
 * SSO (`login_hint` pré-remplit l'identifiant chez le fournisseur). Elle existe
 * pour une raison précise : les droits d'easy3d sont **locaux** (voir
 * `README.md`), donc pour éprouver « un administrateur », « un imprimeur » ou
 * « un lecteur » il faut autant de sessions — et retaper un mot de passe à
 * chaque fois décourage cet essai.
 *
 * # Ce que ce module ne fait pas
 *
 * Il ne **connecte personne** : il lit une liste et la rend. L'authentification
 * reste chez le fournisseur (code + `userinfo`), et les mots de passe affichés
 * sont ceux d'un royaume de développement déjà versionné
 * (`docker/keycloak/realm.json`). Rien de tout cela n'existe hors de ce dépôt.
 *
 * # Source unique
 *
 * La liste vient de `docker/keycloak/realm.json` — **le** fichier importé par
 * Keycloak — et non d'une copie. Un compte ajouté au royaume apparaît donc ici,
 * et une faute de frappe dans un mot de passe se voit tout de suite.
 */

/** Un compte tel que la page de choix le présente. */
export interface DevAccount {
  login: string
  email: string
  password: string
  /** Rôles portés **chez Keycloak** (easy3d ne les lit pas). */
  roles: string[]
}

export interface DevRealm {
  realm: string
  accounts: DevAccount[]
}

/** Forme brute du fichier importé (ce qu'on lit, sans le typage complet de Keycloak). */
interface RealmFile {
  realm?: unknown
  users?: unknown
}

/**
 * Lit un royaume importé et en extrait ce que la page affiche.
 *
 * Fonction **pure** (aucun accès disque) : c'est elle qu'on teste. Un fichier
 * incomplet ne fait pas échouer la page : un compte sans mot de passe ou sans
 * nom est simplement ignoré, et une liste vide veut dire « rien à proposer ».
 */
export function parseRealm(raw: unknown): DevRealm {
  const fichier = (raw ?? {}) as RealmFile
  const realm = typeof fichier.realm === 'string' ? fichier.realm : ''
  const users = Array.isArray(fichier.users) ? fichier.users : []
  const accounts: DevAccount[] = []
  for (const entree of users as Array<Record<string, unknown>>) {
    const login = typeof entree.username === 'string' ? entree.username.trim() : ''
    if (!login) continue
    const credentials = Array.isArray(entree.credentials) ? entree.credentials : []
    const password = credentials
      .map((credential) =>
        typeof (credential as Record<string, unknown>)?.value === 'string'
          ? ((credential as Record<string, unknown>).value as string)
          : '',
      )
      .find((valeur) => valeur.length > 0)
    if (!password) continue
    accounts.push({
      login,
      email: typeof entree.email === 'string' ? entree.email : '',
      password,
      roles: Array.isArray(entree.realmRoles)
        ? entree.realmRoles.filter((role): role is string => typeof role === 'string')
        : [],
    })
  }
  return { realm, accounts }
}

/**
 * `true` quand la page de développement est ouverte.
 *
 * Un seul interrupteur, **côté configuration** (`NUXT_PUBLIC_DEV_LOGIN=1`) : il
 * agit à la fois sur le lien de la page de connexion, sur cette page et sur la
 * route qui livre les comptes. Une installation ordinaire ne le pose pas, et
 * `/dev/login` répond alors 404 — pas « caché mais accessible ».
 *
 * ⚠️ Nuxt réécrit la valeur depuis l'environnement en la passant par `destr` :
 * `NUXT_PUBLIC_DEV_LOGIN=1` donne le **nombre** `1`, pas le booléen `true`. On
 * teste donc ce qui vaut vrai, et pas l'identité à `true` — sinon le drapeau ne
 * marcherait que s'il était écrit en dur dans `nuxt.config.ts`.
 */
export function devLoginEnabled(config: { public?: { devLogin?: unknown } }): boolean {
  const valeur = config.public?.devLogin
  return valeur === true || valeur === 1 || valeur === '1' || valeur === 'true'
}

/**
 * Fichier du royaume à lire.
 *
 * Par défaut celui du dépôt, relativement au dossier de travail de Nuxt (le
 * serveur de dev tourne dans `frontend/`). Dans le conteneur, le compose monte
 * `docker/keycloak/` et pose `NUXT_DEV_REALM` : l'image ne contient pas les
 * sources du dépôt, donc le chemin ne peut pas y être relatif.
 */
export function devRealmPath(): string {
  return process.env.NUXT_DEV_REALM || resolve(process.cwd(), '../docker/keycloak/realm.json')
}

/** Lit le royaume ; lève une erreur explicite si le fichier manque. */
export function readDevRealm(): DevRealm {
  const chemin = devRealmPath()
  let contenu: string
  try {
    contenu = readFileSync(chemin, 'utf8')
  } catch {
    throw new Error(
      `royaume de développement introuvable : ${chemin} ` +
        '(posée par NUXT_DEV_REALM ; dans le conteneur, voir le montage du docker-compose)',
    )
  }
  return parseRealm(JSON.parse(contenu))
}
