import { describe, expect, it } from 'vitest'
import { basename, ext, fileDownloadUrl, fileUrl, relativeTime, toDate } from '~/utils/format'

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

// Le rendu est traduit (composable `useTimeAgo`) : ici on ne teste que le
// découpage en unité + valeur, qui est la partie qui peut se tromper.
describe('relativeTime', () => {
  const base = 1_700_000_000_000 // ms
  const now = new Date(base)

  it('signale « maintenant » sous 5s', () => {
    expect(relativeTime(new Date(base - 2_000), now)).toEqual({ key: 'now', count: 0 })
  })
  it('compte les secondes', () => {
    expect(relativeTime(new Date(base - 20_000), now)).toEqual({ key: 'seconds', count: 20 })
  })
  it('compte les minutes', () => {
    expect(relativeTime(new Date(base - 5 * 60_000), now)).toEqual({ key: 'minutes', count: 5 })
  })
  it('compte les heures', () => {
    expect(relativeTime(new Date(base - 3 * 3_600_000), now)).toEqual({ key: 'hours', count: 3 })
  })
  it('compte les jours', () => {
    expect(relativeTime(new Date(base - 2 * 86_400_000), now)).toEqual({ key: 'days', count: 2 })
  })
  it('renvoie null pour une date inconnue', () => {
    expect(relativeTime(null, now)).toBeNull()
  })
})
