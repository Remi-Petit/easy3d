import { describe, expect, it } from 'vitest'
import {
  CHANGE_FIELDS,
  DETAIL_CODES,
  EVENT_FAMILIES,
  EVENT_FAMILY_LIST,
  EVENT_ICONS,
  EVENT_KINDS,
  describeDetail,
  eventFamily,
  eventKey,
  familyCounts,
  filterEvents,
  sortEvents,
  type JournalEvent,
} from '~/utils/journal'

/**
 * Le journal est ce qu'on relit le jour où un accès surprend : ce qui se teste
 * ici, c'est que chaque forme de `detail` est reconnue pour ce qu'elle est. Une
 * durée de jeton présentée comme un code d'erreur (ou l'inverse) rendrait le
 * journal trompeur — exactement ce qu'on ne veut pas d'un témoin.
 */
describe('eventKey', () => {
  it('nomme chaque type d’événement du backend', () => {
    for (const kind of EVENT_KINDS) {
      expect(eventKey(kind)).toBe(`journal.kind.${kind}`)
    }
  })
})

describe('describeDetail', () => {
  it('ne dit rien quand il n’y a rien à dire', () => {
    expect(describeDetail('login_ok', '')).toEqual({ kind: 'none' })
    expect(describeDetail('logout', '   ')).toEqual({ kind: 'none' })
  })

  it('lit la durée d’un jeton créé', () => {
    expect(describeDetail('token_created', '30')).toEqual({ kind: 'days', days: 30 })
    // 0 = sans expiration, et c'est la page qui le dit.
    expect(describeDetail('token_created', '0')).toEqual({ kind: 'days', days: 0 })
  })

  it('lit la liste des champs modifiés d’un compte', () => {
    expect(describeDetail('account_updated', 'disabled')).toEqual({
      kind: 'changes',
      fields: ['disabled'],
    })
    expect(describeDetail('account_updated', 'identity,roles,permissions')).toEqual({
      kind: 'changes',
      fields: ['identity', 'roles', 'permissions'],
    })
    // Un champ inconnu (backend plus récent) reste visible tel quel.
    expect(describeDetail('account_updated', 'chose')).toEqual({
      kind: 'changes',
      fields: ['chose'],
    })
  })

  it('reconnaît les codes connus', () => {
    for (const code of DETAIL_CODES) {
      expect(describeDetail('login_ok', code)).toEqual({ kind: 'code', code })
    }
    expect(describeDetail('sso_refused', 'oidc_not_provisioned')).toEqual({
      kind: 'code',
      code: 'oidc_not_provisioned',
    })
  })

  it('rend le reste tel quel : un UUID vaut mieux qu’une traduction inventée', () => {
    const uuid = '01a0bf2a-f244-7191-a194-d640bdae34e2'
    expect(describeDetail('token_revoked', uuid)).toEqual({ kind: 'raw', value: uuid })
    expect(describeDetail('account_created', uuid)).toEqual({ kind: 'raw', value: uuid })
  })

  it('ne confond pas un nombre avec une durée pour les autres types', () => {
    // « 30 » sans le type `token_created` n'est pas une durée : on ne l'invente pas.
    expect(describeDetail('account_created', '30')).toEqual({ kind: 'raw', value: '30' })
  })
})

describe('champs modifiables', () => {
  it('sont ceux que le backend écrit', () => {
    expect([...CHANGE_FIELDS]).toEqual([
      'identity',
      'password',
      'password_revoked',
      'disabled',
      'enabled',
      'roles',
      'permissions',
    ])
  })
})

/**
 * Les puces de filtre ne doivent **jamais** cacher un type d'événement : un type
 * qu'aucune famille ne reprend disparaît dès qu'un filtre est posé. C'est ce
 * qu'un journal ne peut pas se permettre — on filtre pour trouver, pas pour
 * perdre.
 */
describe('EVENT_FAMILIES', () => {
  it('reprend tous les types du backend, et une seule fois chacun', () => {
    const repris = EVENT_FAMILY_LIST.flatMap((famille) => [...EVENT_FAMILIES[famille]])
    expect([...repris].sort()).toEqual([...EVENT_KINDS].sort())
    expect(new Set(repris).size).toBe(repris.length)
  })

  it('donne une icône à chaque famille', () => {
    for (const famille of EVENT_FAMILY_LIST) {
      expect(EVENT_ICONS[famille], famille).toMatch(/^i-lucide-/)
    }
  })
})

describe('eventFamily', () => {
  it('range chaque type du backend dans sa famille', () => {
    for (const kind of EVENT_KINDS) {
      const famille = eventFamily(kind)
      expect(famille, `type sans famille : ${kind}`).not.toBeNull()
      if (famille) {
        expect(EVENT_FAMILIES[famille] as readonly string[], kind).toContain(kind)
      }
    }
  })

  it('rend `null` pour un type inconnu, plutôt qu’une famille inventée', () => {
    // Un backend plus récent peut écrire un type que cette version ne connaît
    // pas : la page l'affiche tel quel, sans icône ni couleur.
    expect(eventFamily('login_removed')).toBeNull()
    expect(eventFamily('')).toBeNull()
  })
})

/** Un événement minimal : on ne teste ici que ce qui sert au filtrage. */
function evenement(kind: string, at: number, extra: Partial<JournalEvent> = {}): JournalEvent {
  return { at, kind, actor: null, subject: '', detail: '', ip: '', user_agent: '', ...extra }
}

describe('filterEvents', () => {
  const events = [
    evenement('login_ok', 1_000, { actor: 'remi', ip: '10.0.0.1' }),
    evenement('login_failed', 2_000, { subject: 'remi', detail: 'invalid_credentials' }),
    evenement('token_created', 3_000, { subject: 'CI', detail: '30' }),
    evenement('role_deleted', 4_000, { subject: 'invite' }),
  ]
  /** Aucun texte affiché : la recherche ne retient rien. */
  const muet = () => ''

  it('rend tout quand rien n’est demandé', () => {
    expect(filterEvents(events, {}, muet)).toHaveLength(4)
  })

  it('garde ce qui est plus récent que la période', () => {
    const recents = filterEvents(events, { since: 2_500 }, muet)
    expect(recents.map((event) => event.kind)).toEqual(['token_created', 'role_deleted'])
    // 0 = aucune borne : tout l'historique.
    expect(filterEvents(events, { since: 0 }, muet)).toHaveLength(4)
  })

  it('réunit les familles choisies — un « ou », pas un « et »', () => {
    const choix = filterEvents(events, { families: ['refusals', 'tokens'] }, muet)
    expect(choix.map((event) => event.kind)).toEqual(['login_failed', 'token_created'])
  })

  it('cherche dans le texte affiché, sans tenir compte de la casse', () => {
    const affiche = (event: JournalEvent) => `${event.actor ?? ''} ${event.subject} ${event.ip}`
    expect(filterEvents(events, { text: 'REMI' }, affiche).map((event) => event.kind)).toEqual([
      'login_ok',
      'login_failed',
    ])
    // Les espaces autour ne comptent pas : un copier-coller passe.
    expect(filterEvents(events, { text: '  10.0.0.1 ' }, affiche)).toHaveLength(1)
  })

  it('croise les trois filtres', () => {
    const affiche = (event: JournalEvent) => event.subject
    const choisis = filterEvents(
      events,
      { text: 'ci', families: ['refusals', 'tokens'], since: 2_500 },
      affiche,
    )
    expect(choisis.map((event) => event.kind)).toEqual(['token_created'])
  })
})

describe('familyCounts', () => {
  const events = [
    evenement('login_ok', 1_000),
    evenement('logout', 1_100),
    evenement('login_blocked', 2_000),
    evenement('token_revoked', 2_100),
  ]

  it('compte par famille, sur la période demandée', () => {
    expect(familyCounts(events)).toEqual({
      sessions: 2,
      refusals: 1,
      tokens: 1,
      accounts: 0,
      roles: 0,
    })
    expect(familyCounts(events, 1_500)).toEqual({
      sessions: 0,
      refusals: 1,
      tokens: 1,
      accounts: 0,
      roles: 0,
    })
  })
})

describe('sortEvents', () => {
  const events = [
    evenement('login_ok', 300),
    evenement('logout', 100),
    evenement('token_created', 200),
  ]

  it('laisse le journal dans son ordre naturel quand rien n’est demandé', () => {
    // Le serveur rend déjà du plus récent au plus ancien : « normal » n'est pas
    // un tri, c'est l'ordre du journal — comme le catalogue livré dans son ordre.
    expect(sortEvents(events, 'none').map((event) => event.at)).toEqual([300, 100, 200])
  })

  it('range par date, dans les deux sens', () => {
    expect(sortEvents(events, 'date-desc').map((event) => event.at)).toEqual([300, 200, 100])
    expect(sortEvents(events, 'date-asc').map((event) => event.at)).toEqual([100, 200, 300])
  })

  it('ne modifie pas la liste reçue', () => {
    const avant = events.map((event) => event.at)
    sortEvents(events, 'date-asc')
    expect(events.map((event) => event.at)).toEqual(avant)
  })
})
