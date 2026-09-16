import { describe, expect, it } from 'vitest'
import { folderHref, parentPath } from '~/utils/navigation'

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

  it('remonte un sous-dossier à son parent', () => {
    // Le paramètre porte le chemin complet, encodé : le parent est le chemin
    // moins son dernier segment.
    expect(parentPath('/dossiers/Maison%2FMaison')).toBe('/dossiers/Maison')
    expect(parentPath('/dossiers/Maison%2Fsous%2Fencore')).toBe('/dossiers/Maison%2Fsous')
  })

  it('renvoie l’accueil depuis l’accueil ou un chemin inconnu', () => {
    expect(parentPath('/')).toBe('/')
    expect(parentPath('')).toBe('/')
    expect(parentPath('/inconnu')).toBe('/')
  })
})

describe('folderHref', () => {
  it('encode le chemin du dossier', () => {
    expect(folderHref('DemaAuto')).toBe('/dossiers/DemaAuto')
    expect(folderHref('Mes pièces')).toBe('/dossiers/Mes%20pi%C3%A8ces')
  })

  it('garde un sous-dossier en un seul segment d’URL', () => {
    // Même convention que `/fichier/[rel]` : la barre oblique est encodée, la
    // route reste `[name]`.
    expect(folderHref('Maison/sous')).toBe('/dossiers/Maison%2Fsous')
  })
})
