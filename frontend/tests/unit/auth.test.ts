import { describe, expect, it } from 'vitest'
import { AUTH_ERROR_CODES, LOGIN_PATH, authErrorKey, safeRedirect } from '~/utils/auth'

describe('authErrorKey', () => {
  it('traduit chaque code du backend', () => {
    for (const code of AUTH_ERROR_CODES) {
      expect(authErrorKey(code)).toBe(`auth.errors.${code}`)
    }
  })

  /**
   * Un backend plus récent que l'interface peut renvoyer un code que celle-ci ne
   * connaît pas : mieux vaut un message générique qu'une clé brute affichée à
   * l'utilisateur (`auth.errors.quelque_chose`).
   */
  it('retombe sur un message générique pour un code inconnu', () => {
    expect(authErrorKey('quelque_chose_de_nouveau')).toBe('auth.errors.unknown')
    expect(authErrorKey('')).toBe('auth.errors.unknown')
    expect(authErrorKey(undefined)).toBe('auth.errors.unknown')
    expect(authErrorKey({ statusCode: 500 })).toBe('auth.errors.unknown')
  })
})

describe('safeRedirect', () => {
  it('accepte un chemin interne', () => {
    expect(safeRedirect('/dossiers/Maison')).toBe('/dossiers/Maison')
    expect(safeRedirect('  /admin  ')).toBe('/admin')
  })

  /**
   * `//exemple.fr` est une URL absolue déguisée : le navigateur la suivrait. La
   * page de connexion deviendrait alors un tremplin vers un autre site, à partir
   * d'une adresse familière.
   */
  it('refuse tout ce qui sortirait du site', () => {
    expect(safeRedirect('//exemple.fr')).toBe('/')
    expect(safeRedirect('https://exemple.fr')).toBe('/')
    expect(safeRedirect('exemple.fr')).toBe('/')
    expect(safeRedirect('')).toBe('/')
    expect(safeRedirect(undefined)).toBe('/')
    expect(safeRedirect(['/a', '/b'])).toBe('/')
  })

  it('utilise la destination de repli fournie', () => {
    expect(safeRedirect('https://exemple.fr', '/catalogue')).toBe('/catalogue')
  })
})

describe('LOGIN_PATH', () => {
  it('est un chemin absolu, utilisable tel quel dans une redirection', () => {
    expect(LOGIN_PATH.startsWith('/')).toBe(true)
    expect(safeRedirect(LOGIN_PATH)).toBe(LOGIN_PATH)
  })
})
