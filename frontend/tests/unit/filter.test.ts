import { describe, expect, it } from 'vitest'
import {
  SORT_ICONS,
  SORT_LABELS,
  collectTypes,
  matchesQuery,
  matchesTypes,
  nextSortMode,
  pruneTypes,
  sortFilesByDate,
  toggleType,
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

describe('collectTypes', () => {
  it('recense les formats avec leur nombre, triés par nom', () => {
    const files = [
      file('a.stl'),
      file('b.stl'),
      file('DemaAuto/c.gcode'),
      file('d.3mf'),
    ]
    expect(collectTypes(files)).toEqual([
      { type: '3mf', count: 1 },
      { type: 'gcode', count: 1 },
      { type: 'stl', count: 2 },
    ])
  })

  it('ignore les fichiers sans extension', () => {
    expect(collectTypes([file('LISEZMOI'), file('a.stl')])).toEqual([
      { type: 'stl', count: 1 },
    ])
  })

  it('normalise la casse de l’extension', () => {
    // `a.STL` et `b.stl` sont le même format.
    expect(collectTypes([file('a.STL'), file('b.stl')])).toEqual([
      { type: 'stl', count: 2 },
    ])
  })

  it('renvoie une liste vide sans fichier', () => {
    expect(collectTypes([])).toEqual([])
  })
})

describe('matchesTypes', () => {
  const stl = file('DemaAuto/x.stl')
  const gcode = file('y.gcode')

  it('accepte tout quand aucun type n’est sélectionné', () => {
    expect(matchesTypes(stl, [])).toBe(true)
    expect(matchesTypes(gcode, [])).toBe(true)
  })

  it('accepte un fichier dont le type est sélectionné', () => {
    expect(matchesTypes(stl, ['stl'])).toBe(true)
    expect(matchesTypes(stl, ['stl', '3mf'])).toBe(true)
  })

  it('refuse un fichier dont le type ne l’est pas', () => {
    expect(matchesTypes(stl, ['gcode'])).toBe(false)
    expect(matchesTypes(gcode, ['stl', '3mf'])).toBe(false)
  })
})

describe('toggleType', () => {
  it('ajoute un type absent', () => {
    expect(toggleType([], 'stl')).toEqual(['stl'])
    expect(toggleType(['stl'], 'gcode')).toEqual(['stl', 'gcode'])
  })

  it('retire un type déjà sélectionné', () => {
    expect(toggleType(['stl', 'gcode'], 'stl')).toEqual(['gcode'])
    expect(toggleType(['stl'], 'stl')).toEqual([])
  })

  it('ne modifie pas la liste d’entrée', () => {
    const selection = ['stl']
    toggleType(selection, 'gcode')
    expect(selection).toEqual(['stl'])
  })
})

describe('pruneTypes', () => {
  it('retire les types devenus indisponibles', () => {
    // Ex. : on quitte l'accueil (3MF présent) pour un dossier qui n'en a pas.
    expect(pruneTypes(['stl', '3mf'], ['stl', 'gcode'])).toEqual(['stl'])
  })

  it('conserve l’ordre et tout ce qui reste disponible', () => {
    expect(pruneTypes(['gcode', 'stl'], ['stl', 'gcode', '3mf'])).toEqual(['gcode', 'stl'])
  })

  it('renvoie la même référence si rien ne change', () => {
    // Évite de relancer la réactivité inutilement.
    const selection = ['stl']
    expect(pruneTypes(selection, ['stl', 'gcode'])).toBe(selection)
  })

  it('vide la sélection si plus rien n’est disponible', () => {
    expect(pruneTypes(['stl'], [])).toEqual([])
  })
})
