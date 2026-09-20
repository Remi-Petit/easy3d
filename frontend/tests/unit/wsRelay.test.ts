import { afterEach, describe, expect, it, vi } from 'vitest'
import { runtimeConfig } from '../stubs/imports'
import {
  backendHttpUrl,
  backendWsUrl,
  peerCookie,
  ticketFor,
  withParam,
  type BrowserPeer,
} from '../../server/utils/wsRelay'

/** Pair minimal : seuls la requête et les en-têtes nous intéressent ici. */
function peer(headers?: unknown): BrowserPeer {
  return {
    request: { url: '/ws', headers },
    send: () => {},
    close: () => {},
  }
}

describe('peerCookie', () => {
  it('lit le cookie des en-têtes de Node', () => {
    expect(peerCookie(peer({ cookie: 'autre=1; easy3d_session=abc' }))).toBe(
      'autre=1; easy3d_session=abc',
    )
  })

  /** Certains adaptateurs donnent un tableau quand l'en-tête arrive en double. */
  it('rassemble un en-tête répété', () => {
    expect(peerCookie(peer({ cookie: ['a=1', 'b=2'] }))).toBe('a=1; b=2')
  })

  it('lit aussi les `Headers` standards', () => {
    expect(peerCookie(peer(new Headers({ cookie: 'easy3d_session=abc' })))).toBe(
      'easy3d_session=abc',
    )
  })

  it('ne trouve rien quand il n’y a pas d’en-têtes', () => {
    expect(peerCookie(peer())).toBe('')
    expect(peerCookie(peer({}))).toBe('')
    expect(peerCookie(peer({ 'user-agent': 'x' }))).toBe('')
  })
})

describe('withParam', () => {
  it('ajoute le paramètre en tenant compte de l’existant', () => {
    expect(withParam('ws://h/ws', 'ticket', 'abc')).toBe('ws://h/ws?ticket=abc')
    expect(withParam('ws://h/ws?x=1', 'ticket', 'abc')).toBe('ws://h/ws?x=1&ticket=abc')
  })

  it('encode la valeur (un ticket hexadécimal ne change pas)', () => {
    expect(withParam('ws://h/ws', 'ticket', 'a b/c')).toBe('ws://h/ws?ticket=a%20b%2Fc')
  })
})

describe('adresses du backend', () => {
  afterEach(() => {
    runtimeConfig.hpccatApiBase = 'http://127.0.0.1:8090'
  })

  it('déduit le WebSocket de l’adresse HTTP', () => {
    expect(backendHttpUrl()).toBe('http://127.0.0.1:8090')
    expect(backendWsUrl('/ws')).toBe('ws://127.0.0.1:8090/ws')
  })

  it('tolère un slash final', () => {
    runtimeConfig.hpccatApiBase = 'http://api:8090//'
    expect(backendHttpUrl()).toBe('http://api:8090')
    expect(backendWsUrl('/collab/Maison')).toBe('ws://api:8090/collab/Maison')
  })
})

describe('ticketFor', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function reponse(status: number, body: unknown = {}): void {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        status,
        ok: status >= 200 && status < 300,
        json: async () => body,
      })),
    )
  }

  it('demande le ticket en transmettant le cookie', async () => {
    const fetchMock = vi.fn(async () => ({
      status: 200,
      ok: true,
      json: async () => ({ ticket: 'abc' }),
    }))
    vi.stubGlobal('fetch', fetchMock)

    expect(await ticketFor('easy3d_session=jeton')).toEqual({ accepted: true, ticket: 'abc' })
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:8090/auth/ws-ticket', {
      method: 'POST',
      headers: { cookie: 'easy3d_session=jeton' },
    })
  })

  /** Authentification éteinte : aucun ticket, la connexion se fait comme avant. */
  it('accepte l’absence de ticket', async () => {
    reponse(200, { ticket: null })
    expect(await ticketFor('')).toEqual({ accepted: true, ticket: null })
  })

  /** Session refusée : le relais doit fermer, pas se connecter anonymement. */
  it('refuse quand le backend refuse la session', async () => {
    reponse(401)
    expect(await ticketFor('easy3d_session=perime')).toEqual({ accepted: false, ticket: null })
  })

  /** Backend plus ancien (route absente) : on ne casse pas l'existant. */
  it('se connecte sans ticket si la route n’existe pas', async () => {
    reponse(404)
    expect(await ticketFor('easy3d_session=jeton')).toEqual({ accepted: true, ticket: null })
  })

  it('se connecte sans ticket si le backend est injoignable', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('ECONNREFUSED')
      }),
    )
    expect(await ticketFor('easy3d_session=jeton')).toEqual({ accepted: true, ticket: null })
  })
})
