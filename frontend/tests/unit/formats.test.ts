import { describe, expect, it } from 'vitest'
import {
  DEFAULT_FORMATS,
  fileIcon,
  formatFor,
  hasPreview,
  showsViewer,
  viewerOf,
} from '~/utils/formats'

// Les formats viennent du backend (`GET /models` → `formats`) : ces tests
// vérifient les règles d'affichage appliquées à cette liste, pas la liste
// elle-même (elle est testée côté Rust, dans `formats/mod.rs`).

describe('formatFor', () => {
  it('reconnaît un fichier d’après son extension', () => {
    expect(formatFor(DEFAULT_FORMATS, 'a.stl')?.name).toBe('STL')
    expect(formatFor(DEFAULT_FORMATS, 'DemaAuto/x.3MF')?.name).toBe('3MF')
  })

  it('accepte les extensions multiples d’un même format', () => {
    expect(formatFor(DEFAULT_FORMATS, 'piece.gco')?.name).toBe('G-code')
  })

  it('renvoie null pour un format inconnu', () => {
    expect(formatFor(DEFAULT_FORMATS, 'notes.md')).toBeNull()
    expect(formatFor(DEFAULT_FORMATS, 'sans_extension')).toBeNull()
  })

  it('suit la liste annoncée par le backend', () => {
    // Un format ajouté côté Rust est pris en compte sans rien coder ici.
    const custom = [{ name: 'PLY', extensions: ['ply'], preview: false, viewer: 'mesh' as const }]
    expect(formatFor(custom, 'x.ply')?.name).toBe('PLY')
    expect(formatFor(custom, 'x.stl')).toBeNull()
  })
})

describe('viewerOf', () => {
  it('distingue maillage, G-code et format sans rendu', () => {
    expect(viewerOf(DEFAULT_FORMATS, 'a.stl')).toBe('mesh')
    expect(viewerOf(DEFAULT_FORMATS, 'a.gcode')).toBe('gcode')
    expect(viewerOf(DEFAULT_FORMATS, 'a.txt')).toBe('none')
  })
})

describe('hasPreview', () => {
  it('suit le drapeau annoncé par le backend', () => {
    expect(hasPreview(DEFAULT_FORMATS, 'a.stl')).toBe(true)
    expect(hasPreview(DEFAULT_FORMATS, 'notes.md')).toBe(false)
  })
})

describe('showsViewer', () => {
  it('affiche toujours un maillage, quel que soit le mode', () => {
    expect(showsViewer(DEFAULT_FORMATS, 'a.obj', '3d')).toBe(true)
    expect(showsViewer(DEFAULT_FORMATS, 'a.obj', 'image')).toBe(true)
  })

  it('ne rend un G-code qu’en mode 3d', () => {
    expect(showsViewer(DEFAULT_FORMATS, 'a.gcode', '3d')).toBe(true)
    expect(showsViewer(DEFAULT_FORMATS, 'a.gcode', 'image')).toBe(false)
  })

  it('ne rend rien pour un format sans visionneuse', () => {
    expect(showsViewer(DEFAULT_FORMATS, 'notes.md', '3d')).toBe(false)
  })
})

describe('fileIcon', () => {
  it('choisit l’emoji d’après la visionneuse du format', () => {
    expect(fileIcon(DEFAULT_FORMATS, 'a.stl')).toBe('🧊')
    expect(fileIcon(DEFAULT_FORMATS, 'a.gco')).toBe('🖨')
    expect(fileIcon(DEFAULT_FORMATS, 'a.txt')).toBe('📄')
  })
})
