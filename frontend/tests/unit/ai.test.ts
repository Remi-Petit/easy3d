import { describe, expect, it } from 'vitest'
import { aiConfigured, hitHref, modelOptions, type AiHit } from '~/utils/ai'

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

describe('modelOptions', () => {
  it('propose ce que le fournisseur annonce, dans son ordre', () => {
    expect(modelOptions(['gpt-4o', 'o3-mini'], '')).toEqual(['gpt-4o', 'o3-mini'])
  })

  it('garde le modèle retenu même s’il n’est pas annoncé', () => {
    // Au chargement de la page, rien n'a encore été interrogé : sans cette
    // règle, le champ apparaîtrait vide et l'enregistrement suivant effacerait
    // le modèle.
    expect(modelOptions([], 'deepseek-v4-flash')).toEqual(['deepseek-v4-flash'])
    expect(modelOptions(['gpt-4o'], 'deepseek-v4-flash')).toEqual(['deepseek-v4-flash', 'gpt-4o'])
  })

  it('ne répète pas un modèle déjà proposé et ignore les valeurs vides', () => {
    expect(modelOptions(['gpt-4o'], 'gpt-4o')).toEqual(['gpt-4o'])
    expect(modelOptions(['', 'gpt-4o'], '   ')).toEqual(['gpt-4o'])
  })

  it('dédoublonne les deux sources', () => {
    // La liste vient du fournisseur **et** de la configuration : la même entrée
    // ne doit pas apparaître deux fois (c'est exactement ce qui arrivait quand
    // le modèle enregistré était ajouté à une liste qui le contenait déjà).
    expect(modelOptions(['fake-small', 'fake-large', 'fake-small'], 'fake-small')).toEqual([
      'fake-small',
      'fake-large',
    ])
    expect(modelOptions(['a', ' a '], 'a')).toEqual(['a'])
  })
})
