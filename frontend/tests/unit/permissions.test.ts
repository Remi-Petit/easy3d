import { readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import { PERM, byGroup, granted, groupKey, permissionKey, permissionLabel } from '~/utils/permissions'

describe('byGroup', () => {
  it('regroupe les droits en conservant l’ordre du backend', () => {
    const groups = byGroup([
      { id: 'catalog.read', group: 'catalog' },
      { id: 'model.delete', group: 'catalog' },
      { id: 'ai.use', group: 'ai' },
      { id: 'model.upload', group: 'catalog' },
    ])

    // L'ordre compte : c'est celui du catalogue côté serveur (catalogue, IA,
    // réglages, comptes), et il fait la lecture des cases à cocher.
    expect(groups.map((group) => group.group)).toEqual(['catalog', 'ai'])
    expect(groups[0].permissions.map((permission) => permission.id)).toEqual([
      'catalog.read',
      'model.delete',
      'model.upload',
    ])
  })

  it('ne perd rien, et accepte une liste vide', () => {
    expect(byGroup([])).toEqual([])

    const many = Array.from({ length: 13 }, (_, index) => ({ id: `x.${index}`, group: 'x' }))
    expect(byGroup(many)[0].permissions).toHaveLength(13)
  })
})

describe('libellés', () => {
  it('dérive les clés i18n', () => {
    expect(permissionKey('model.delete')).toBe('perm.model.delete')
    expect(groupKey('accounts')).toBe('permGroup.accounts')
  })

  it('traduit un droit connu', () => {
    const translate = (key: string) => `[${key}]`
    expect(permissionLabel('model.delete', translate, () => true)).toBe('[perm.model.delete]')
  })

  /**
   * Un backend plus récent peut publier un droit que l'interface ignore : mieux
   * vaut afficher `model.purge` que la clé brute `perm.model.purge`.
   */
  it('retombe sur l’identifiant quand la traduction manque', () => {
    const translate = (key: string) => `[${key}]`
    expect(permissionLabel('model.purge', translate, () => false)).toBe('model.purge')
  })
})

describe('granted', () => {
  it('dit si un droit est accordé', () => {
    expect(granted(['catalog.read'], 'catalog.read')).toBe(true)
    expect(granted(['catalog.read'], 'model.delete')).toBe(false)
    expect(granted(undefined, 'catalog.read')).toBe(false)
    expect(granted([], 'catalog.read')).toBe(false)
  })
})

describe('PERM', () => {
  it('ne contient que des identifiants bien formés, sans doublon', () => {
    const ids = Object.values(PERM)
    for (const id of ids) {
      expect(id).toMatch(/^[a-z]+\.[a-z]+$/)
    }
    // Deux constantes qui pointeraient le même droit seraient une faute de
    // recopie silencieuse : un bouton masqué chez qui devrait le voir.
    expect(new Set(ids).size).toBe(ids.length)
  })
})

const racine = resolve(dirname(fileURLToPath(import.meta.url)), '../..')

/** Tous les fichiers `.vue` et `.ts` de `app/`, récursivement. */
function fichiersApplication(dossier = join(racine, 'app')): string[] {
  return readdirSync(dossier).flatMap((entree) => {
    const chemin = join(dossier, entree)
    if (statSync(chemin).isDirectory()) return fichiersApplication(chemin)
    return /\.(vue|ts)$/.test(chemin) ? [chemin] : []
  })
}

/**
 * `can()` n'est **pas** une auto-import de Nuxt : c'est une fonction rendue par
 * `useAuth()`. Un composant qui l'appelle sans `const { can } = useAuth()`
 * compile sans broncher, puis échoue **dans le navigateur** au premier rendu
 * (`ReferenceError: can is not defined`) — le rendu serveur, lui, ne dit rien.
 *
 * Ce garde-fou a déjà attrapé trois fichiers lors de la vérification du RBAC :
 * c'est exactement le genre d'erreur qu'aucun test unitaire ne voit, puisqu'elle
 * n'existe qu'à l'exécution côté client.
 */
describe('can()', () => {
  it('n’est utilisé que dans des fichiers qui appellent useAuth()', () => {
    const coupables = fichiersApplication()
      .filter((chemin) => /\bcan\(/.test(readFileSync(chemin, 'utf8')))
      .filter((chemin) => !/useAuth\(/.test(readFileSync(chemin, 'utf8')))
      .map((chemin) => relative(racine, chemin))

    expect(coupables).toEqual([])
  })
})
