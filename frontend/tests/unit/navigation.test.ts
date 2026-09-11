import { describe, expect, it } from 'vitest'
import { parentPath } from '~/utils/navigation'

describe('parentPath', () => {
  it('remonte un fichier de dossier vers son dossier', () => {
    expect(parentPath('/fichier/DemaAuto%2Fboitier.stl')).toBe('/dossiers/DemaAuto')
    // Les espaces et accents sont ré-encodés pour l'URL du dossier.
    expect(parentPath('/fichier/DemaAuto%2FBoitier%20d%C3%A9ma%20auto.stl')).toBe(
      '/dossiers/DemaAuto',
    )
  })

  it('rattache un fichier imbriqué au dossier de premier niveau', () => {
    // Le backend regroupe les fichiers sous leur dossier de premier niveau :
    // c'est donc là que le fichier est réellement listé.
    expect(parentPath('/fichier/DemaAuto%2Fsous%2Fpiece.stl')).toBe('/dossiers/DemaAuto')
  })

  it('reencode le nom du dossier', () => {
    expect(parentPath('/fichier/Mes%20pi%C3%A8ces%2Fx.stl')).toBe(
      '/dossiers/Mes%20pi%C3%A8ces',
    )
  })

  it('renvoie l’accueil pour un fichier à la racine', () => {
    expect(parentPath('/fichier/Capuchon%20LMB.stl')).toBe('/')
    expect(parentPath('/fichier/x.gcode')).toBe('/')
  })

  it('renvoie l’accueil depuis une page dossier', () => {
    expect(parentPath('/dossiers/DemaAuto')).toBe('/')
    expect(parentPath('/dossiers/Mes%20pi%C3%A8ces')).toBe('/')
  })

  it('renvoie l’accueil depuis l’accueil ou un chemin inconnu', () => {
    expect(parentPath('/')).toBe('/')
    expect(parentPath('')).toBe('/')
    expect(parentPath('/inconnu')).toBe('/')
  })
})
