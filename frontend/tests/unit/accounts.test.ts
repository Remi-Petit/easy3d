import { describe, expect, it } from 'vitest'
import { errorCodeOf } from '~/composables/useAccounts'

/**
 * Le backend répond en **texte brut** avec un code (`last_admin`, `role_frozen`,
 * `name_taken`…) : c'est ce code que la page traduit. Un refus de droit, lui, ne
 * vient pas du handler mais de la couche de contrôle — il arrive en 403 sans
 * corps, et il faut alors le reconnaître au code d'état.
 */
describe('errorCodeOf', () => {
  it('rend le code du corps tel quel', () => {
    expect(errorCodeOf({ data: 'last_admin' })).toBe('last_admin')
    expect(errorCodeOf({ data: '  role_frozen  ' })).toBe('role_frozen')
  })

  it('déduit les refus de la couche de contrôle du code d’état', () => {
    expect(errorCodeOf({ statusCode: 403 })).toBe('forbidden')
    expect(errorCodeOf({ statusCode: 401 })).toBe('unauthenticated')
    expect(errorCodeOf({ statusCode: 404 })).toBe('not_found')
  })

  it('accepte la forme d’un `FetchError` (`response.status`)', () => {
    expect(errorCodeOf({ response: { status: 403 } })).toBe('forbidden')
    expect(errorCodeOf({ statusCode: 500 })).toBe('http_500')
  })

  it('retombe sur `unknown` plutôt que sur une chaîne vide', () => {
    expect(errorCodeOf({})).toBe('unknown')
    expect(errorCodeOf(undefined)).toBe('unknown')
    expect(errorCodeOf({ data: '   ' })).toBe('unknown')
    expect(errorCodeOf(new Error('boum'))).toBe('unknown')
  })
})
