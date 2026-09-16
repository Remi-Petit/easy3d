import { describe, expect, it } from 'vitest'
import {
  childrenOf,
  depth,
  directFiles,
  directFolders,
  filesUnder,
  folderView,
  parentOf,
  segments,
} from '~/utils/folders'
import type { FileInfo, FolderInfo } from '~/composables/useModels'

/** Fichier minimal : seul `rel` compte pour la hiérarchie. */
const file = (rel: string): FileInfo => ({
  rel,
  path: `/models/${rel}`,
  created: null,
  modified: null,
})

/** Dossier de premier niveau minimal, sous-dossiers à plat comme le scan. */
function top(subRels: string[], fileRels: string[]): FolderInfo {
  return {
    name: 'Maison',
    count: fileRels.length,
    modified: 100,
    files: fileRels.map(file),
    subfolders: subRels.map((rel) => ({
      rel,
      name: rel.split('/').pop() ?? rel,
      count: 0,
      modified: 100,
    })),
  }
}

describe('segments', () => {
  it('découpe un chemin relatif', () => {
    expect(segments('Maison/sous')).toEqual(['Maison', 'sous'])
    expect(segments('Maison')).toEqual(['Maison'])
    // Barres obliques en trop, ou chemin vide.
    expect(segments('/Maison//sous/')).toEqual(['Maison', 'sous'])
    expect(segments('')).toEqual([])
  })
})

describe('parentOf', () => {
  it('donne le dossier parent', () => {
    expect(parentOf('Maison/sous/encore')).toBe('Maison/sous')
    expect(parentOf('Maison/sous')).toBe('Maison')
  })

  it('rend une chaîne vide pour un chemin de premier niveau', () => {
    // Convention : `''` désigne la racine du catalogue.
    expect(parentOf('Maison')).toBe('')
    expect(parentOf('')).toBe('')
  })
})

describe('depth', () => {
  it('compte les niveaux', () => {
    expect(depth('Maison')).toBe(1)
    expect(depth('Maison/sous/encore')).toBe(3)
    expect(depth('')).toBe(0)
  })
})

describe('childrenOf', () => {
  const items = [{ rel: 'Maison' }, { rel: 'Maison/sous' }, { rel: 'Maison/sous/encore' }, { rel: 'Autre' }]

  it('ne garde que les enfants directs', () => {
    expect(childrenOf(items, 'Maison').map((i) => i.rel)).toEqual(['Maison/sous'])
    expect(childrenOf(items, 'Maison/sous').map((i) => i.rel)).toEqual(['Maison/sous/encore'])
  })

  it('trouve les éléments de la racine', () => {
    expect(childrenOf(items, '').map((i) => i.rel)).toEqual(['Maison', 'Autre'])
  })
})

describe('directFiles et directFolders', () => {
  const files = ['Maison/a.stl', 'Maison/sous/b.stl', 'Maison/sous/encore/c.stl'].map(file)
  const subs = ['Maison/sous', 'Maison/sous/encore'].map((rel) => ({
    rel,
    name: rel.split('/').pop() ?? rel,
    count: 0,
    modified: null,
  }))

  it('sépare les fichiers directs de ceux des sous-dossiers', () => {
    expect(directFiles(files, 'Maison').map((f) => f.rel)).toEqual(['Maison/a.stl'])
    expect(directFiles(files, 'Maison/sous').map((f) => f.rel)).toEqual(['Maison/sous/b.stl'])
  })

  it('ne liste pas les petits-enfants comme sous-dossiers directs', () => {
    expect(directFolders(subs, 'Maison').map((s) => s.rel)).toEqual(['Maison/sous'])
    expect(directFolders(subs, 'Maison/sous').map((s) => s.rel)).toEqual(['Maison/sous/encore'])
  })
})

describe('filesUnder', () => {
  const files = ['Maison/a.stl', 'Maison/sous/b.stl', 'Maison/sous/encore/c.stl'].map(file)

  it('prend tout le sous-arbre', () => {
    expect(filesUnder(files, 'Maison').map((f) => f.rel)).toEqual([
      'Maison/a.stl',
      'Maison/sous/b.stl',
      'Maison/sous/encore/c.stl',
    ])
    expect(filesUnder(files, 'Maison/sous').map((f) => f.rel)).toEqual([
      'Maison/sous/b.stl',
      'Maison/sous/encore/c.stl',
    ])
  })

  it('ne confond pas un préfixe avec un dossier parent', () => {
    // `Maison/attic` ne doit pas être pris pour un sous-dossier de `Maison/a`.
    expect(filesUnder(['Maison/attic/x.stl'].map(file), 'Maison/a')).toEqual([])
  })
})

describe('folderView', () => {
  const maison = top(
    ['Maison/sous', 'Maison/sous/encore', 'Maison/vide'],
    ['Maison/a.stl', 'Maison/sous/b.stl', 'Maison/sous/encore/c.stl'],
  )

  it('décrit le dossier de premier niveau', () => {
    const vue = folderView(maison, 'Maison')!
    expect(vue.name).toBe('Maison')
    expect(vue.rel).toBe('Maison')
    expect(vue.count).toBe(3)
    // Tous les fichiers du dossier, sous-arbre compris : c'est ce que le scan
    // annonce, et ce que voit la recherche.
    expect(vue.files).toHaveLength(3)
    expect(vue.subfolders.map((s) => s.rel)).toEqual(['Maison/sous', 'Maison/vide'])
  })

  it('décrit un sous-dossier', () => {
    const vue = folderView(maison, 'Maison/sous')!
    expect(vue.name).toBe('sous')
    expect(vue.files.map((f) => f.rel)).toEqual(['Maison/sous/b.stl', 'Maison/sous/encore/c.stl'])
    expect(vue.subfolders.map((s) => s.rel)).toEqual(['Maison/sous/encore'])
  })

  it('décrit un sous-dossier vide (aucun fichier, aucun sous-dossier)', () => {
    const vue = folderView(maison, 'Maison/vide')!
    expect(vue.files).toEqual([])
    expect(vue.subfolders).toEqual([])
  })

  it('rend null pour un chemin inconnu', () => {
    expect(folderView(maison, 'Maison/inexistant')).toBeNull()
    expect(folderView(maison, 'Autre')).toBeNull()
  })
})
