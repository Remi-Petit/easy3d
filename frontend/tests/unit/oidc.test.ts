import { describe, expect, it } from 'vitest'
import { oidcStartUrl } from '~/utils/auth'
import { PROVISIONING_MODES, emptyOidc, oidcBody, oidcUsable, redirectUri } from '~/utils/oidc'

/**
 * Ces fonctions décident de ce qui part vers `config.yml` : elles sont pures,
 * donc testables sans serveur — et c'est voulu, parce qu'une valeur mal
 * préparée ici ne se voit qu'à la lecture du fichier (ou au prochain login).
 */
describe('oidcStartUrl', () => {
  it('transmet la page à rejoindre, encodée', () => {
    expect(oidcStartUrl()).toBe('/api/auth/oidc/start')
    expect(oidcStartUrl('/dossiers/Maison')).toBe(
      '/api/auth/oidc/start?redirect=%2Fdossiers%2FMaison',
    )
    // Une espace ou un `&` ne doit pas casser la requête.
    expect(oidcStartUrl('/a&b c')).toBe('/api/auth/oidc/start?redirect=%2Fa%26b%20c')
    // Vide = pas de paramètre : le backend retombe alors sur l'accueil.
    expect(oidcStartUrl('')).toBe('/api/auth/oidc/start')
  })
})

describe('redirectUri', () => {
  it('pointe sur la route relayée par l’interface', () => {
    expect(redirectUri('https://catalogue.exemple.fr')).toBe(
      'https://catalogue.exemple.fr/api/auth/oidc/callback',
    )
    // Un `/` final ne doit pas produire un double séparateur : le fournisseur
    // compare l'adresse caractère par caractère.
    expect(redirectUri('https://catalogue.exemple.fr/')).toBe(
      'https://catalogue.exemple.fr/api/auth/oidc/callback',
    )
  })
})

describe('emptyOidc', () => {
  it('décrit un SSO éteint et sans émetteur', () => {
    expect(emptyOidc()).toEqual({
      enabled: false,
      issuer: null,
      client_id: null,
      client_secret: null,
      scopes: [],
      provisioning: 'auto',
    })
  })
})

describe('oidcBody', () => {
  it('nettoie les saisies et vide ce qui est vide', () => {
    const body = oidcBody(
      {
        enabled: true,
        issuer: '  https://sso.exemple.fr/  ',
        client_id: '  easy3d ',
        client_secret: ' secret ',
        scopes: ['openid', ' email ', ''],
        provisioning: 'manual',
      },
      [],
    )

    expect(body).toEqual({
      enabled: true,
      issuer: 'https://sso.exemple.fr/',
      client_id: 'easy3d',
      client_secret: 'secret',
      scopes: ['openid', 'email'],
      provisioning: 'manual',
    })
  })

  it('accepte un bloc absent ou à moitié rempli', () => {
    expect(oidcBody(undefined, [])).toEqual(emptyOidc())
    expect(oidcBody({ enabled: true }, []).issuer).toBe(null)
  })

  /**
   * Un mode inconnu (backend plus récent) retombe sur `auto` plutôt que de
   * faire échouer l'enregistrement : le compte se crée alors avec le rôle par
   * défaut, ce qui est le comportement le moins surprenant.
   */
  it('retombe sur « auto » pour un mode inconnu', () => {
    expect(oidcBody({ provisioning: 'quelque_chose' }, []).provisioning).toBe('auto')
    expect(PROVISIONING_MODES).toContain(oidcBody({ provisioning: 'approval' }, []).provisioning)
  })

  /**
   * Un champ imposé par l'environnement est renvoyé tel quel : le serveur
   * l'ignorerait de toute façon (l'environnement gagne), et l'interface ne doit
   * pas laisser croire qu'on peut le changer ici.
   */
  it('ne touche pas aux champs figés par l’environnement', () => {
    const body = oidcBody(
      {
        enabled: false,
        issuer: 'https://du-fichier.exemple.fr',
        client_id: 'x',
        client_secret: '***',
        scopes: ['openid'],
        provisioning: 'manual',
      },
      ['enabled', 'issuer', 'client_secret', 'scopes', 'provisioning'],
    )

    // L'environnement allume le SSO : écrire `false` dans le fichier serait un
    // mensonge sur l'état réel de l'installation.
    expect(body.enabled).toBe(true)
    expect(body.issuer).toBe('https://du-fichier.exemple.fr')
    expect(body.scopes).toEqual(['openid'])
    expect(body.provisioning).toBe('manual')
    // Seul `client_id` reste modifiable : il est trimé comme les autres.
    expect(body.client_id).toBe('x')
  })

  /**
   * `***` est le marqueur du backend pour « un secret est enregistré » : le
   * renvoyer tel quel le conserve, et un champ vidé l'efface.
   */
  it('laisse passer le marqueur du secret et l’effacement', () => {
    expect(oidcBody({ client_secret: '***' }, []).client_secret).toBe('***')
    expect(oidcBody({ client_secret: '' }, []).client_secret).toBe(null)
  })
})

describe('oidcUsable', () => {
  it('n’est vrai que si le backend dit le SSO actif', () => {
    expect(oidcUsable({ locked: [], active: true })).toBe(true)
    expect(oidcUsable({ locked: ['enabled'], active: false, problem: 'client_id' })).toBe(false)
    expect(oidcUsable(null)).toBe(false)
    expect(oidcUsable(undefined)).toBe(false)
  })
})
