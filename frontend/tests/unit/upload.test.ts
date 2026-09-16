import { describe, expect, it } from 'vitest'
import { dropPicks, inputPicks, uploadPlan, uploadRel } from '~/utils/upload'
import type { DropEntry } from '~/utils/upload'

/**
 * Faux `File` : seuls `name` et `webkitRelativePath` sont lus par le calcul de
 * chemin, et jsdom ne construit pas un `File` portant un chemin relatif.
 */
function fakeFile(name: string, relativePath = ''): File {
  return { name, webkitRelativePath: relativePath } as File
}

/** Fausse entrée de fichier déposé (`FileSystemFileEntry`). */
function fileEntry(name: string, fullPath: string): DropEntry {
  return {
    isFile: true,
    fullPath,
    file: (resolve) => resolve(fakeFile(name)),
  }
}

/**
 * Faux dossier déposé (`FileSystemDirectoryEntry`).
 *
 * `batchSize` sert à vérifier la lecture par lots : l'API du navigateur rend au
 * plus 100 entrées par appel.
 */
function dirEntry(fullPath: string, children: DropEntry[], batchSize = 100): DropEntry {
  let cursor = 0
  return {
    isFile: false,
    fullPath,
    createReader: () => ({
      readEntries: (resolve) => {
        const batch = children.slice(cursor, cursor + batchSize)
        cursor += batch.length
        resolve(batch)
      },
    }),
  }
}

/** Faux `DataTransfer` : seuls ses `items` sont lus. */
function fakeDrop(entries: Array<DropEntry | File>) {
  return {
    items: entries.map((entry) =>
      'fullPath' in entry
        ? { kind: 'file', webkitGetAsEntry: () => entry as DropEntry }
        : { kind: 'file', getAsFile: () => entry as File },
    ),
  }
}

describe('uploadRel', () => {
  it('place un fichier choisi à la racine', () => {
    expect(uploadRel('piece.stl')).toBe('piece.stl')
  })

  it('place un fichier dans le dossier d’accueil', () => {
    expect(uploadRel('piece.stl', 'DemaAuto')).toBe('DemaAuto/piece.stl')
  })

  it('conserve l’arborescence d’un dossier', () => {
    expect(uploadRel('Boitiers/sous/piece.stl')).toBe('Boitiers/sous/piece.stl')
  })

  it('combine dossier d’accueil et arborescence', () => {
    expect(uploadRel('Lot/sous/piece.stl', 'DemaAuto')).toBe('DemaAuto/Lot/sous/piece.stl')
  })

  it('normalise les séparateurs Windows', () => {
    expect(uploadRel('Lot\\sous\\piece.stl')).toBe('Lot/sous/piece.stl')
  })

  it('refuse les chemins que le backend rejetterait', () => {
    // Composant caché : réservé au backend (aperçus, notes).
    expect(uploadRel('.cache')).toBeNull()
    // Remontée hors de la racine.
    expect(uploadRel('Lot/../piece.stl')).toBeNull()
    // Dossier d'accueil douteux.
    expect(uploadRel('piece.stl', '..')).toBeNull()
    // Chemin vide.
    expect(uploadRel('')).toBeNull()
  })
})

describe('inputPicks', () => {
  it('lit le nom seul pour une sélection de fichiers', () => {
    expect(inputPicks([fakeFile('piece.stl')])).toEqual([
      { file: expect.anything(), path: 'piece.stl' },
    ])
  })

  it('lit le chemin relatif pour une sélection de dossier', () => {
    const picks = inputPicks([fakeFile('piece.stl', 'Lot/sous/piece.stl')])
    expect(picks[0].path).toBe('Lot/sous/piece.stl')
  })
})

describe('dropPicks', () => {
  it('collecte un fichier déposé, avec son chemin', async () => {
    const picks = await dropPicks(fakeDrop([fileEntry('piece.stl', '/piece.stl')]))
    expect(picks.map((p) => p.path)).toEqual(['piece.stl'])
  })

  it('parcourt un dossier déposé, sous-dossiers compris', async () => {
    const lot = dirEntry('/Lot', [
      fileEntry('a.stl', '/Lot/a.stl'),
      dirEntry('/Lot/sous', [fileEntry('b.stl', '/Lot/sous/b.stl')]),
    ])
    const picks = await dropPicks(fakeDrop([lot]))
    expect(picks.map((p) => p.path)).toEqual(['Lot/a.stl', 'Lot/sous/b.stl'])
  })

  it('lit un dossier par lots (au-delà de 100 entrées)', async () => {
    // 250 enfants : `readEntries` doit être rappelé jusqu'au lot vide.
    const many = Array.from({ length: 250 }, (_, i) => fileEntry(`${i}.stl`, `/Gros/${i}.stl`))
    const picks = await dropPicks(fakeDrop([dirEntry('/Gros', many, 100)]))
    expect(picks).toHaveLength(250)
    expect(picks[249].path).toBe('Gros/249.stl')
  })

  it('retombe sur le nom du fichier sans API d’entrées', async () => {
    const picks = await dropPicks(fakeDrop([fakeFile('piece.stl')]))
    expect(picks.map((p) => p.path)).toEqual(['piece.stl'])
  })
})

describe('uploadPlan', () => {
  it('garde l’ordre de la sélection et écarte les chemins refusés', () => {
    const picks = inputPicks([fakeFile('b.stl'), fakeFile('.caché'), fakeFile('a.stl')])
    expect(uploadPlan(picks, 'DemaAuto').map((task) => task.rel)).toEqual([
      'DemaAuto/b.stl',
      'DemaAuto/a.stl',
    ])
  })

  it('associe chaque tâche à son fichier', () => {
    const file = fakeFile('piece.stl')
    const [task] = uploadPlan(inputPicks([file]))
    expect(task.file).toBe(file)
    expect(task.rel).toBe('piece.stl')
  })
})

