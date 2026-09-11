import { describe, expect, it } from 'vitest'
import {
  SORT_ICONS,
  SORT_LABELS,
  matchesQuery,
  nextSortMode,
  sortFilesByDate,
  type SortMode,
} from '~/utils/filter'

/** Fichier minimal : seuls `path` et `modified` comptent pour le filtre. */
function file(path: string, modified: number | null = null) {
  return { path, rel: path, created: null, modified }
}

describe('nextSortMode', () => {
  it('boucle normal → récent → ancien → normal', () => {
    expect(nextSortMode('none')).toBe('date-desc')
    expect(nextSortMode('date-desc')).toBe('date-asc')
    expect(nextSortMode('date-asc')).toBe('none')
  })

  it('revient au point de départ après trois clics', () => {
    let mode: SortMode = 'none'
    for (let i = 0; i < 3; i++) mode = nextSortMode(mode)
    expect(mode).toBe('none')
  })

  it('a un libellé et une flèche pour chaque mode', () => {
    for (const mode of ['none', 'date-asc', 'date-desc'] as SortMode[]) {
      expect(SORT_LABELS[mode]).toBeTruthy()
      expect(SORT_ICONS[mode]).toBeTruthy()
    }
    // ↕ = normal, ↑ = ancien → récent, ↓ = récent → ancien.
    expect(SORT_ICONS.none).toBe('↕')
    expect(SORT_ICONS['date-asc']).toBe('↑')
    expect(SORT_ICONS['date-desc']).toBe('↓')
  })
})

describe('matchesQuery', () => {
  it('accepte tout quand la recherche est vide ou blanche', () => {
    const f = file('DemaAuto/boitier.stl')
    expect(matchesQuery(f, '')).toBe(true)
    expect(matchesQuery(f, '   ')).toBe(true)
  })

  it('compare le nom de fichier, pas le dossier parent', () => {
    const f = file('DemaAuto/boitier.stl')
    // Le nom correspond…
    expect(matchesQuery(f, 'boitier')).toBe(true)
    expect(matchesQuery(f, 'BOITIER')).toBe(true)
    expect(matchesQuery(f, 'tier.st')).toBe(true)
    // …mais pas le dossier (géré séparément par la page).
    expect(matchesQuery(f, 'demaauto')).toBe(false)
  })

  it('renvoie false quand rien ne correspond', () => {
    expect(matchesQuery(file('a.stl'), 'zzz')).toBe(false)
  })
})

describe('sortFilesByDate', () => {
  const a = file('a.stl', 100)
  const b = file('b.stl', 300)
  const c = file('c.stl', 200)

  it('ne change rien en mode normal', () => {
    const input = [a, b, c]
    const out = sortFilesByDate(input, 'none')
    expect(out).toBe(input)
    expect(out.map((f) => f.path)).toEqual(['a.stl', 'b.stl', 'c.stl'])
  })

  it('trie du plus ancien au plus récent', () => {
    expect(sortFilesByDate([b, a, c], 'date-asc').map((f) => f.path)).toEqual([
      'a.stl',
      'c.stl',
      'b.stl',
    ])
  })

  it('trie du plus récent au plus ancien', () => {
    expect(sortFilesByDate([a, b, c], 'date-desc').map((f) => f.path)).toEqual([
      'b.stl',
      'c.stl',
      'a.stl',
    ])
  })

  it('ne modifie pas la liste d’entrée', () => {
    const input = [b, a]
    sortFilesByDate(input, 'date-asc')
    expect(input.map((f) => f.path)).toEqual(['b.stl', 'a.stl'])
  })

  it('renvoie les fichiers sans date en fin de liste, dans les deux sens', () => {
    const sansDate = file('z.stl', null)
    for (const mode of ['date-asc', 'date-desc'] as SortMode[]) {
      const paths = sortFilesByDate([sansDate, a, b], mode).map((f) => f.path)
      expect(paths[2]).toBe('z.stl')
    }
  })

  it('garde l’ordre relatif des fichiers sans date', () => {
    const n1 = file('n1.stl', null)
    const n2 = file('n2.stl', null)
    expect(sortFilesByDate([n1, n2, a], 'date-asc').map((f) => f.path)).toEqual([
      'a.stl',
      'n1.stl',
      'n2.stl',
    ])
  })

  it('accepte une liste vide', () => {
    expect(sortFilesByDate([], 'date-desc')).toEqual([])
  })
})
