import { describe, expect, it } from 'vitest'
import { createI18n } from 'vue-i18n'
import { pluralizationRules } from '../../i18n/plural'

/** Les formes d'un message valent 0, 1 ou 2 : c'est l'index que la règle rend. */
const formes = (regle: (choice: number) => number, nombres: number[]) =>
  nombres.map((n) => regle(n))

describe('règles de pluriel', () => {
  it('le russe distingue une, quelques et beaucoup', () => {
    const ru = pluralizationRules.ru
    // « 1 файл », mais « 11 файлов » : le « 11 » est le piège classique.
    expect(formes(ru, [1, 21, 31, 101])).toEqual([0, 0, 0, 0])
    // « 2 файла », mais « 12 файлов ».
    expect(formes(ru, [2, 3, 4, 22, 23, 24])).toEqual([1, 1, 1, 1, 1, 1])
    expect(formes(ru, [0, 5, 11, 12, 13, 14, 25, 100])).toEqual([2, 2, 2, 2, 2, 2, 2, 2])
  })

  it('l’ukrainien et le croate suivent le russe', () => {
    for (const code of ['uk', 'hr']) {
      expect(formes(pluralizationRules[code], [1, 2, 5, 11, 21, 22])).toEqual([0, 1, 2, 2, 0, 1])
    }
  })

  it('le polonais ne met « 1 » à part que pour un seul', () => {
    const pl = pluralizationRules.pl
    expect(formes(pl, [1])).toEqual([0])
    expect(formes(pl, [2, 3, 4, 22, 23, 24])).toEqual([1, 1, 1, 1, 1, 1])
    expect(formes(pl, [0, 5, 12, 13, 14, 25, 100])).toEqual([2, 2, 2, 2, 2, 2, 2])
  })

  it('le tchèque et le slovaque passent à « beaucoup » dès cinq', () => {
    for (const code of ['cs', 'sk']) {
      expect(formes(pluralizationRules[code], [1, 2, 3, 4, 0, 5, 100])).toEqual([0, 1, 1, 1, 2, 2, 2])
    }
  })

  it('le roumain ajoute « de » au-delà de dix-neuf', () => {
    const ro = pluralizationRules.ro
    // « 1 fișier » — et 101 reste sur la forme courante (« 101 fișiere »).
    expect(formes(ro, [1])).toEqual([0])
    expect(formes(ro, [0, 2, 19, 101])).toEqual([1, 1, 1, 1])
    // De vingt à cent dix-neuf, la forme est « de + nom ».
    expect(formes(ro, [20, 99, 100, 120])).toEqual([2, 2, 2, 2])
  })

  it('ne règle que les langues concernées', () => {
    // Une langue absente garde le défaut de vue-i18n (une | plusieurs).
    expect(pluralizationRules.fr).toBeUndefined()
    expect(pluralizationRules.en).toBeUndefined()
    expect(Object.keys(pluralizationRules).sort()).toEqual(['cs', 'hr', 'pl', 'ro', 'ru', 'sk', 'uk'])
  })
})

/**
 * Ce que vue-i18n fait vraiment des deux formes de message que l'on écrit :
 *  - deux formes (« une | plusieurs ») → le défaut de vue-i18n suffit pour les langues
 *    à deux formes CLDR (tr, el, fi, hu, ca, bg…) ;
 *  - une seule forme (zh, ja, ko, id, vi…) → le message n'est pas un pluriel du tout,
 *    le nombre s'affiche par interpolation. Le vérifier évite de croire que ces
 *    langues affichent quelque chose de vide.
 */
describe('rendu du pluriel par vue-i18n', () => {
  const t = (locale: string, message: string, n: number) => {
    const i18n = createI18n({
      legacy: false,
      locale,
      messages: { [locale]: { m: message } },
      pluralRules: pluralizationRules,
    })
    return i18n.global.t('m', n)
  }

  it('deux formes : une seule pour 1, l’autre au-delà', () => {
    const message = '{count} fichier | {count} fichiers'
    expect(t('tr', message, 1)).toBe('1 fichier')
    expect(t('tr', message, 2)).toBe('2 fichiers')
    expect(t('tr', message, 5)).toBe('5 fichiers')
    // 0 prend la forme du pluriel dans ces langues, comme le veut CLDR.
    expect(t('fi', message, 0)).toBe('0 fichiers')
  })

  it('une seule forme : le nombre s’affiche quand même', () => {
    const message = '{count} 个文件'
    expect(t('zh', message, 1)).toBe('1 个文件')
    expect(t('zh', message, 2)).toBe('2 个文件')
    expect(t('zh', message, 5)).toBe('5 个文件')
  })
})
