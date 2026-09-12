import { describe, expect, it } from 'vitest'
import { collabBase, collabRoom, collabUrl, editorIdFor, wsBase } from '~/utils/collab'

describe('wsBase', () => {
  const here = { protocol: 'https:', host: 'easy3d.exemple.test' }

  it('utilise la base WebSocket quand elle est fournie', () => {
    expect(wsBase({ hpccatWsBase: 'ws://127.0.0.1:8090' }, here)).toBe('ws://127.0.0.1:8090')
  })

  it('retire les slashes finales', () => {
    expect(wsBase({ hpccatWsBase: 'ws://127.0.0.1:8090//' }, here)).toBe('ws://127.0.0.1:8090')
  })

  /**
   * Sans configuration, le navigateur vise l'origine qui sert l'interface :
   * Nitro relaie `/ws` et `/collab` vers le backend. C'est ce qui évite de figer
   * une adresse — un `localhost` figé désigne la machine du **visiteur**.
   */
  it('se rabat sur l’origine de la page', () => {
    expect(wsBase({}, here)).toBe('wss://easy3d.exemple.test')
    expect(wsBase(undefined, { protocol: 'http:', host: 'localhost:3000' })).toBe(
      'ws://localhost:3000',
    )
  })

  it('reste vide hors du navigateur (rendu serveur)', () => {
    expect(wsBase()).toBe('')
    expect(wsBase({})).toBe('')
  })
})

describe('collabRoom', () => {
  it('laisse un nom de dossier intact', () => {
    expect(collabRoom('DemaAuto')).toBe('DemaAuto')
  })

  it('garde les segments séparés par des slashes', () => {
    expect(collabRoom('DemaAuto/boitier.stl')).toBe('DemaAuto/boitier.stl')
  })

  it('échappe ce qui perturberait l’URL', () => {
    expect(collabRoom('DemaAuto/Boitier dema auto.gcode')).toBe(
      'DemaAuto/Boitier%20dema%20auto.gcode',
    )
    expect(collabRoom('Bouclier #2.stl')).toBe('Bouclier%20%232.stl')
    expect(collabRoom('a?b.stl')).toBe('a%3Fb.stl')
    expect(collabRoom('100%.stl')).toBe('100%25.stl')
  })

  it('ignore les slashes superflus', () => {
    expect(collabRoom('/DemaAuto/')).toBe('DemaAuto')
    expect(collabRoom('DemaAuto//boitier.stl')).toBe('DemaAuto/boitier.stl')
  })
})

describe('collabUrl', () => {
  it('compose l’adresse du document d’une note', () => {
    const pub = { hpccatWsBase: 'ws://127.0.0.1:8090/' }
    expect(collabBase(pub)).toBe('ws://127.0.0.1:8090/collab')
    expect(collabUrl(pub, 'DemaAuto/Boitier dema auto.gcode')).toBe(
      'ws://127.0.0.1:8090/collab/DemaAuto/Boitier%20dema%20auto.gcode',
    )
  })
})

describe('editorIdFor', () => {
  it('donne un identifiant valide en CSS', () => {
    // md-editor-v3 construit `#<id> .cm-scroller` : points et slashes interdits.
    expect(editorIdFor('_collab-demo.stl')).toBe('note-_collab-demo-stl')
    expect(editorIdFor('DemaAuto/Boitier dema auto.gcode')).toBe(
      'note-DemaAuto-Boitier-dema-auto-gcode',
    )
  })

  it('reste unique et non vide', () => {
    expect(editorIdFor('a.stl')).not.toBe(editorIdFor('b.stl'))
    expect(editorIdFor('///')).toMatch(/^note-[\w-]+$/)
  })
})
