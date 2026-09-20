import { describe, expect, it } from 'vitest'
import { CHANGE_FIELDS, DETAIL_CODES, EVENT_KINDS, describeDetail, eventKey } from '~/utils/journal'

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
      'disabled',
      'enabled',
      'roles',
      'permissions',
    ])
  })
})
