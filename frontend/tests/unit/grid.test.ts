import { describe, expect, it } from 'vitest'
import type { GridColumn } from '~/utils/grid'
import { sortRows, sortValueOf, visibleRange } from '~/utils/grid'

/**
 * Logique de la grille de données (voir `DataGrid.vue`).
 *
 * Ce qui est vérifié ici, c'est ce qui se casse en silence : l'ordre d'un tri,
 * l'immutabilité de la liste reçue, et la fenêtre de virtualisation (des index
 * négatifs ou un décalage d'une ligne ne se voient qu'à l'œil, à l'écran).
 */

type Ligne = { nom?: string; n?: number; role?: string; disabled?: boolean }

const colonneNom: GridColumn<Ligne> = { id: 'nom', header: 'Nom', width: 200, sortable: true }

describe('sortValueOf', () => {
  it('prend la propriété qui porte l’identifiant de la colonne', () => {
    expect(sortValueOf(colonneNom, { nom: 'Alice' })).toBe('Alice')
  })

  it('laisse les nombres en nombre', () => {
    // Sinon « 10 » se rangerait avant « 9 » : le tri serait faux, sans erreur.
    expect(sortValueOf<Ligne>({ id: 'n', header: 'n', width: 10 }, { n: 10 })).toBe(10)
  })

  it('rend une chaîne vide pour une valeur absente', () => {
    expect(sortValueOf(colonneNom, {})).toBe('')
  })

  it('donne la main à la colonne quand elle sait se trier', () => {
    const colonne: GridColumn<Ligne> = {
      id: 'statut',
      header: 'Statut',
      width: 100,
      sortValue: (row) => (row.disabled ? '0' : '1'),
    }
    expect(sortValueOf(colonne, { disabled: true })).toBe('0')
    expect(sortValueOf(colonne, {})).toBe('1')
  })
})

describe('sortRows', () => {
  const lignes: Ligne[] = [{ nom: 'Zoé' }, { nom: 'élodie' }, { nom: 'Alice' }]

  it('ne touche à rien sans tri', () => {
    // L'ordre reçu veut dire quelque chose (c'est celui du backend) : on le rend
    // tel quel, et sans copie inutile.
    expect(sortRows(lignes, [colonneNom], null)).toBe(lignes)
    expect(sortRows(lignes, [colonneNom], { id: 'nom', mode: 'none' })).toBe(lignes)
  })

  it('trie en texte, sans distinguer casse ni accents', () => {
    const ordre = sortRows(lignes, [colonneNom], { id: 'nom', mode: 'date-asc' })
    expect(ordre.map((ligne) => ligne.nom)).toEqual(['Alice', 'élodie', 'Zoé'])

    const inverse = sortRows(lignes, [colonneNom], { id: 'nom', mode: 'date-desc' })
    expect(inverse.map((ligne) => ligne.nom)).toEqual(['Zoé', 'élodie', 'Alice'])
  })

  it('ne modifie pas la liste reçue', () => {
    const ordonnees = sortRows(lignes, [colonneNom], { id: 'nom', mode: 'date-asc' })
    expect(ordonnees).not.toBe(lignes)
    expect(lignes.map((ligne) => ligne.nom)).toEqual(['Zoé', 'élodie', 'Alice'])
  })

  it('compare les nombres en nombre', () => {
    const nombres: Ligne[] = [{ n: 10 }, { n: 9 }, { n: 100 }]
    const colonne: GridColumn<Ligne> = { id: 'n', header: 'Nombre', width: 80, sortable: true }
    expect(sortRows(nombres, [colonne], { id: 'n', mode: 'date-asc' }).map((l) => l.n)).toEqual([
      9, 10, 100,
    ])
  })

  it('trie sur la clé de la colonne, pas sur la propriété', () => {
    // Une colonne peut afficher un libellé calculé (des rôles mis en mots) et se
    // trier sur autre chose.
    const colonne: GridColumn<Ligne> = {
      id: 'role',
      header: 'Rôles',
      width: 120,
      sortValue: (row) => row.role ?? '',
    }
    const lignesRoles: Ligne[] = [{ role: 'lecteur' }, { role: 'admin' }]
    expect(
      sortRows(lignesRoles, [colonne], { id: 'role', mode: 'date-asc' }).map((l) => l.role),
    ).toEqual(['admin', 'lecteur'])
  })

  it('rend l’ordre reçu si la colonne n’existe plus', () => {
    expect(sortRows(lignes, [colonneNom], { id: 'disparue', mode: 'date-asc' })).toBe(lignes)
  })
})

describe('visibleRange', () => {
  it('commence à la première ligne, avec la marge de sécurité', () => {
    // 340 px de haut, 34 px par ligne : 10 lignes visibles, + 4 en réserve.
    expect(visibleRange(100, 34, 0, 340)).toEqual({
      start: 0,
      end: 14,
      paddingTop: 0,
      paddingBottom: 86 * 34,
    })
  })

  it('suit le défilement, cales comprises', () => {
    const fenetre = visibleRange(100, 34, 340, 340)
    expect(fenetre.start).toBe(6)
    expect(fenetre.end).toBe(24)
    // Les cales valent exactement les lignes non rendues : la hauteur totale de
    // la table ne change pas, donc la barre de défilement ne saute pas.
    expect(fenetre.paddingTop).toBe(6 * 34)
    expect(fenetre.paddingBottom).toBe((100 - 24) * 34)
  })

  it('s’arrête à la dernière ligne', () => {
    const fenetre = visibleRange(100, 34, 340 * 9, 340)
    expect(fenetre.end).toBe(100)
    expect(fenetre.paddingBottom).toBe(0)
  })

  it('rend une fenêtre vide sans ligne', () => {
    expect(visibleRange(0, 34, 0, 340)).toEqual({
      start: 0,
      end: 0,
      paddingTop: 0,
      paddingBottom: 0,
    })
  })

  it('ne produit jamais d’index négatif', () => {
    // Rebond élastique du défilement (scrollTop négatif), ou conteneur pas encore
    // mesuré (hauteur nulle) : la fenêtre doit rester saine.
    const rebond = visibleRange(10, 34, -80, 100, 0)
    expect(rebond.start).toBe(0)
    expect(rebond.paddingTop).toBe(0)

    const nonMesure = visibleRange(10, 34, 0, 0, 4)
    expect(nonMesure).toEqual({ start: 0, end: 4, paddingTop: 0, paddingBottom: 6 * 34 })
  })

  it('ne rend jamais plus de lignes qu’il n’en existe', () => {
    expect(visibleRange(2, 34, 0, 340).end).toBe(2)
  })

  it('ne rend jamais une hauteur de ligne nulle', () => {
    // Une hauteur de 0 diviserait par zéro : on la borne à 1 px.
    expect(visibleRange(5, 0, 0, 10, 0).end).toBe(5)
  })
})
