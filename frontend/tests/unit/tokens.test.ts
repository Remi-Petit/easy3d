import { describe, expect, it } from 'vitest'
import {
  SOON_DAYS,
  TOKEN_DEFAULT_DAYS,
  TOKEN_DURATIONS,
  durationKey,
  expiryKind,
} from '~/utils/tokens'

const JOUR = 86_400
const MAINTENANT = 1_700_000_000

/**
 * La durée de vie d'un jeton est décidée par le backend (`expiry_from_days`),
 * mais c'est ici qu'on décide ce que l'interface en **dit** — et une date mal
 * interprétée ferait afficher « sans expiration » sur un jeton expiré, ce qui
 * est exactement le mensonge qu'on veut éviter.
 */
describe('expiryKind', () => {
  it('reconnaît un jeton sans expiration', () => {
    expect(expiryKind(null, MAINTENANT)).toBe('never')
    expect(expiryKind(undefined, MAINTENANT)).toBe('never')
  })

  it('reconnaît un jeton expiré, y compris à l’instant même', () => {
    expect(expiryKind(MAINTENANT - JOUR, MAINTENANT)).toBe('expired')
    // La borne est inclusive côté serveur (`fin <= maintenant`) : l'interface
    // doit dire la même chose au même moment.
    expect(expiryKind(MAINTENANT, MAINTENANT)).toBe('expired')
  })

  it('distingue « bientôt » de « plus tard », en jours entiers', () => {
    expect(expiryKind(MAINTENANT + JOUR, MAINTENANT)).toBe('soon')
    expect(expiryKind(MAINTENANT + SOON_DAYS * JOUR, MAINTENANT)).toBe('soon')
    expect(expiryKind(MAINTENANT + (SOON_DAYS + 1) * JOUR, MAINTENANT)).toBe('later')
    // 6 jours et 23 h = 7 jours entiers à venir : annoncé « bientôt », pas
    // « plus tard » (l'arrondi se fait vers le haut).
    expect(expiryKind(MAINTENANT + 7 * JOUR - 3_600, MAINTENANT)).toBe('soon')
  })

  it('accepte une fenêtre « bientôt » différente', () => {
    expect(expiryKind(MAINTENANT + 20 * JOUR, MAINTENANT, 30)).toBe('soon')
    expect(expiryKind(MAINTENANT + 20 * JOUR, MAINTENANT, 10)).toBe('later')
  })
})

describe('durées proposées', () => {
  it('commence par « sans expiration », qui est le défaut', () => {
    expect(TOKEN_DURATIONS[0]).toBe(0)
    expect(TOKEN_DEFAULT_DAYS).toBe(0)
    expect(TOKEN_DURATIONS).toContain(30)
    expect(TOKEN_DURATIONS).toContain(365)
  })

  it('nomme chaque durée par une clé i18n stable', () => {
    expect(durationKey(0)).toBe('account.days0')
    expect(durationKey(90)).toBe('account.days90')
    // Une durée hors liste garde sa clé : la page retombe alors sur le libellé
    // générique `account.daysMany` (voir `compte.vue`).
    expect(durationKey(1000)).toBe('account.days1000')
  })
})
