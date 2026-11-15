import { describe, expect, it } from 'vitest'
import { isNewer } from '~/utils/version'

/**
 * Le piège que ce module existe pour éviter : `0.10.0` sort **après** `0.9.0`,
 * alors qu'en comparaison de chaînes il serait « plus petit ». Une alerte ratée
 * à cause d'un `parseInt` oublié est le genre de bug qui ne se voit qu'une fois
 * par an, sur la version qui compte.
 */
describe('isNewer', () => {
  it('compare les segments comme des nombres', () => {
    expect(isNewer('0.10.0', '0.9.0')).toBe(true)
    expect(isNewer('0.9.0', '0.10.0')).toBe(false)
    expect(isNewer('1.0.0', '0.99.99')).toBe(true)
    expect(isNewer('0.4.10', '0.4.9')).toBe(true)
  })

  it('accepte le `v` des tags de release', () => {
    expect(isNewer('v0.5.0', '0.4.5')).toBe(true)
    expect(isNewer('V0.5.0', '0.4.5')).toBe(true)
  })

  it('ne signale rien quand la version est la même', () => {
    expect(isNewer('0.4.5', '0.4.5')).toBe(false)
    expect(isNewer('v0.4.5', '0.4.5')).toBe(false)
    // Un segment absent vaut zéro : `0.5` et `0.5.0` sont la même version.
    expect(isNewer('0.5', '0.5.0')).toBe(false)
  })

  it('ignore les suffixes, et ne prend pas une pré-version pour une mise à jour', () => {
    expect(isNewer('0.5.0-rc1', '0.5.0')).toBe(false)
    expect(isNewer('0.5.0+20260101', '0.4.5')).toBe(true)
  })

  it('reste muet sur une valeur qui n’est pas un numéro', () => {
    expect(isNewer(null, '0.4.5')).toBe(false)
    expect(isNewer('', '0.4.5')).toBe(false)
    expect(isNewer('latest', '0.4.5')).toBe(false)
  })
})
