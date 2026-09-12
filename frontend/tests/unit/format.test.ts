import { describe, expect, it } from 'vitest'
import { basename, ext, fileDownloadUrl, fileUrl, toDate, timeAgo } from '~/utils/format'

describe('basename', () => {
  it('extrait le nom final du chemin', () => {
    expect(basename('/a/b/c.stl')).toBe('c.stl')
    expect(basename('DemaAuto/boitier.stl')).toBe('boitier.stl')
    expect(basename('fiche.gcode')).toBe('fiche.gcode')
    expect(basename('no_sep')).toBe('no_sep')
  })
})

describe('ext', () => {
  it("retourne l'extension en minuscules", () => {
    expect(ext('c.stl')).toBe('stl')
    expect(ext('DemaAuto/x.STL')).toBe('stl')
    expect(ext('a.gcode')).toBe('gcode')
    expect(ext('sans_extension')).toBe('')
    expect(ext('archive.gco')).toBe('gco')
  })
})

describe('fileDownloadUrl', () => {
  it('ajoute le drapeau download à l’URL du fichier', () => {
    expect(fileDownloadUrl('a.stl')).toBe(`${fileUrl('a.stl')}&download=1`)
  })
  it('encode les chemins et les accents', () => {
    expect(fileDownloadUrl('DemaAuto/Boitier déma auto.gcode')).toBe(
      '/api/file?path=DemaAuto%2FBoitier%20d%C3%A9ma%20auto.gcode&download=1',
    )
  })
})

describe('toDate', () => {
  it('convertit un timestamp unix (secondes) en Date', () => {
    const d = toDate(1_700_000_000)
    expect(d).toBeInstanceOf(Date)
    expect(d?.getTime()).toBe(1_700_000_000 * 1000)
  })
  it('renvoie null pour null ou 0', () => {
    expect(toDate(null)).toBeNull()
    expect(toDate(0)).toBeNull()
  })
})

describe('timeAgo', () => {
  const base = 1_700_000_000_000 // ms
  const now = new Date(base)

  it("affiche « à l’instant » sous 5s", () => {
    expect(timeAgo(new Date(base - 2_000), now)).toBe('à l’instant')
  })
  it('affiche les secondes', () => {
    expect(timeAgo(new Date(base - 20_000), now)).toBe('il y a 20s')
  })
  it('affiche les minutes', () => {
    expect(timeAgo(new Date(base - 5 * 60_000), now)).toBe('il y a 5 min')
  })
  it('affiche les heures', () => {
    expect(timeAgo(new Date(base - 3 * 3_600_000), now)).toBe('il y a 3h')
  })
  it('affiche les jours', () => {
    expect(timeAgo(new Date(base - 2 * 86_400_000), now)).toBe('il y a 2j')
  })
  it('renvoie « — » pour null', () => {
    expect(timeAgo(null)).toBe('—')
  })
})
