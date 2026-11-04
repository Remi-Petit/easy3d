import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { devLoginEnabled, parseRealm } from '../../server/utils/devRealm'

/**
 * Page de choix des comptes de **développement** (`/dev/login`) : ce qu'on
 * vérifie ici est la **lecture** du royaume, sans disque ni serveur. La page
 * elle-même est couverte par la campagne `e2e-sso`.
 */
describe('parseRealm', () => {
  it('extrait les comptes et les rôles que Keycloak leur donne', () => {
    const royaume = parseRealm({
      realm: 'demo',
      users: [
        {
          username: 'alice',
          email: 'alice@exemple.fr',
          credentials: [{ value: 'secret-alice' }],
          realmRoles: ['admin'],
        },
        { username: 'bob', email: 'bob@exemple.fr', credentials: [{ value: 'secret-bob' }] },
      ],
    })

    expect(royaume).toEqual({
      realm: 'demo',
      accounts: [
        { login: 'alice', email: 'alice@exemple.fr', password: 'secret-alice', roles: ['admin'] },
        { login: 'bob', email: 'bob@exemple.fr', password: 'secret-bob', roles: [] },
      ],
    })
  })

  it('écarte ce qui ne peut pas servir à se connecter', () => {
    const royaume = parseRealm({
      realm: 'demo',
      users: [
        { username: '   ', email: 'sans-nom@exemple.fr', credentials: [{ value: 'x' }] },
        { username: 'sans-mot-de-passe', email: 'a@b.c' },
        { username: 'mot-de-passe-vide', credentials: [{ value: '' }] },
      ],
    })
    // Un compte sans identifiant ou sans mot de passe ne serait qu'une ligne
    // inerte : la page n'a rien à en faire, et l'afficher ferait croire à un
    // compte utilisable.
    expect(royaume.accounts).toEqual([])
  })

  it('accepte un fichier incomplet sans lever', () => {
    expect(parseRealm(null)).toEqual({ realm: '', accounts: [] })
    expect(parseRealm({})).toEqual({ realm: '', accounts: [] })
    expect(parseRealm({ realm: 42, users: 'pas un tableau' })).toEqual({ realm: '', accounts: [] })
  })

  it('lit le royaume réellement importé par le dépôt', () => {
    // Le fichier est la **source unique** (il est importé par Keycloak) : s'il
    // change de forme, la page cesserait de proposer des comptes — d'où ce
    // garde-fou sur le fichier du dépôt, pas sur une copie.
    const fichier = readFileSync(
      new URL('../../../docker/keycloak/realm.json', import.meta.url),
      'utf8',
    )
    const royaume = parseRealm(JSON.parse(fichier))

    expect(royaume.realm).toBe('easy3d')
    expect(royaume.accounts.length).toBeGreaterThanOrEqual(5)
    expect(royaume.accounts.map((compte) => compte.login)).toEqual(
      expect.arrayContaining(['admin', 'user', 'imprimeuse', 'observatrice', 'superuser']),
    )
    expect(royaume.accounts.find((compte) => compte.login === 'imprimeuse')?.roles).toEqual([
      'imprimeur',
    ])
    // Chaque compte proposé a de quoi se connecter : la page affiche le mot de
    // passe et le fait saisir chez le fournisseur.
    for (const compte of royaume.accounts) expect(compte.password).not.toBe('')
  })
})

describe('devLoginEnabled', () => {
  it('n’est vrai que si la configuration le demande', () => {
    expect(devLoginEnabled({ public: { devLogin: true } })).toBe(true)
    expect(devLoginEnabled({ public: { devLogin: false } })).toBe(false)
    expect(devLoginEnabled({ public: {} })).toBe(false)
    expect(devLoginEnabled({})).toBe(false)
  })

  it('accepte ce que Nuxt fabrique à partir de l’environnement', () => {
    // `NUXT_PUBLIC_DEV_LOGIN=1` passe par `destr` : c'est le **nombre** `1` qui
    // arrive ici, pas `true`. Le drapeau doit marcher par variable
    // d'environnement, c'est même son seul usage.
    expect(devLoginEnabled({ public: { devLogin: 1 } })).toBe(true)
    expect(devLoginEnabled({ public: { devLogin: '1' } })).toBe(true)
    expect(devLoginEnabled({ public: { devLogin: 0 } })).toBe(false)
    expect(devLoginEnabled({ public: { devLogin: '' } })).toBe(false)
  })
})
