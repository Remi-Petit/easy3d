import { describe, expect, it } from 'vitest'
import { aiConfigured, hitHref, type AiHit } from '~/utils/ai'

/**
 * Recherche assistée : la seule logique pure côté interface.
 *
 * Le reste (appels, état partagé) vit dans `useAiSearch` et se vérifie de bout
 * en bout dans les tests e2e.
 */

describe('aiConfigured', () => {
  it('exige un fournisseur, et rien d’autre', () => {
    expect(aiConfigured(null)).toBe(false)
    expect(aiConfigured(undefined)).toBe(false)
    expect(aiConfigured({})).toBe(false)
    expect(aiConfigured({ provider: null })).toBe(false)
    // Un champ vidé à la main vaut « pas de fournisseur ».
    expect(aiConfigured({ provider: '   ' })).toBe(false)

    expect(aiConfigured({ provider: 'ollama' })).toBe(true)
    // Sans clé : c'est le backend qui refuse (Ollama n'en demande pas), pas
    // l'interface — sinon on griserait le bouton pour un fournisseur local.
    expect(aiConfigured({ provider: 'openai' })).toBe(true)
  })
})

function hit(kind: AiHit['kind'], rel: string): AiHit {
  return { rel, kind, reason: 'peu importe' }
}

describe('hitHref', () => {
  it('dirige vers la page du fichier ou du dossier', () => {
    expect(hitHref(hit('file', 'DemaAuto/boitier.stl'))).toBe('/fichier/DemaAuto%2Fboitier.stl')
    expect(hitHref(hit('folder', 'DemaAuto'))).toBe('/dossiers/DemaAuto')
  })

  it('encode les chemins tels que les pages les attendent', () => {
    // Les noms du catalogue contiennent des espaces et des accents : le chemin
    // doit être encodé segment par segment, comme `FileItem`/`FolderCard`.
    expect(hitHref(hit('file', 'Boitier dema auto.stl'))).toBe(
      '/fichier/Boitier%20dema%20auto.stl',
    )
    expect(hitHref(hit('folder', 'Maison/étage 2'))).toBe('/dossiers/Maison%2F%C3%A9tage%202')
  })
})
