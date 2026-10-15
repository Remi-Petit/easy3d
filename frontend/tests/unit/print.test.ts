import { describe, expect, it } from 'vitest'
import { formatBytes, formatDuration, hasPrintFacts } from '~/utils/print'

describe('formatDuration', () => {
  it('reste lisible de la minute à la journée de travail', () => {
    expect(formatDuration(0)).toBe('0 min')
    expect(formatDuration(43 * 60)).toBe('43 min')
    // Arrondi à la minute : les slicers annoncent rarement des secondes rondes.
    expect(formatDuration(43 * 60 + 29)).toBe('43 min')
    expect(formatDuration(3600)).toBe('1 h')
    expect(formatDuration(7260)).toBe('2 h 01')
    expect(formatDuration(31 * 3600 + 20 * 60)).toBe('31 h 20')
  })
})

describe('formatBytes', () => {
  it('suit la langue de l’interface', () => {
    // Le séparateur décimal et l'unité viennent d'`Intl`, pas d'un suffixe écrit
    // à la main : c'est ce qui distingue « 4,1 Mo » de « 4.1 MB ».
    expect(formatBytes(4_100_000, 'fr')).toMatch(/^4,1\s/)
    expect(formatBytes(4_100_000, 'en')).toMatch(/^4\.1\s/)
    expect(formatBytes(900_000, 'fr')).toMatch(/^900\s/)
  })
})

describe('hasPrintFacts', () => {
  it('ignore la seule taille du fichier', () => {
    expect(hasPrintFacts(null)).toBe(false)
    expect(hasPrintFacts(undefined)).toBe(false)
    expect(hasPrintFacts({})).toBe(false)
    // Un STL ne dit que son poids en octets : pas de quoi remplir une carte.
    expect(hasPrintFacts({ bytes: 1234 })).toBe(false)
    expect(hasPrintFacts({ material: 'PLA' })).toBe(true)
    expect(hasPrintFacts({ size_mm: [120, 80, 30] })).toBe(true)
    expect(hasPrintFacts({ support: false })).toBe(true)
  })
})