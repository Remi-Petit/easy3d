import { describe, expect, it } from 'vitest'
import { reconcileRemote } from '~/utils/note'

describe('reconcileRemote', () => {
  it('ne fait rien si le serveur confirme ce que l’on sait déjà', () => {
    const r = reconcileRemote('texte', 'texte', 'texte')
    expect(r).toEqual({ content: 'texte', saved: 'texte', conflict: false })
  })

  it('suit le serveur quand il n’y a pas de saisie en cours', () => {
    // Cas courant : l'autre session modifie, la nôtre ne fait que regarder.
    const r = reconcileRemote('ancien', 'ancien', 'nouveau')
    expect(r).toEqual({ content: 'nouveau', saved: 'nouveau', conflict: false })
  })

  it('suit le serveur quand la note passe de vide à remplie (et inversement)', () => {
    expect(reconcileRemote('', '', '# note')).toEqual({
      content: '# note',
      saved: '# note',
      conflict: false,
    })
    expect(reconcileRemote('# note', '# note', '')).toEqual({
      content: '',
      saved: '',
      conflict: false,
    })
  })

  it('préserve une saisie locale non enregistrée et signale le conflit', () => {
    // La référence serveur est « ancien », on a tapé « en cours de frappe ».
    const r = reconcileRemote('en cours de frappe', 'ancien', 'venu d’ailleurs')
    expect(r).toEqual({
      content: 'en cours de frappe',
      saved: 'ancien',
      conflict: true,
    })
  })

  it('ne signale pas de conflit quand le serveur renvoie notre propre valeur', () => {
    // Après notre enregistrement, le serveur diffuse le même contenu.
    const r = reconcileRemote('ma note', 'ma note', 'ma note')
    expect(r.conflict).toBe(false)
    expect(r.content).toBe('ma note')
  })

  it('ne signale pas de conflit si la valeur distante est déjà notre référence', () => {
    // On a tapé par-dessus, mais le serveur n'a pas bougé.
    const r = reconcileRemote('brouillon', 'ancien', 'ancien')
    expect(r).toEqual({ content: 'brouillon', saved: 'ancien', conflict: false })
  })
})
