import { describe, expect, it } from 'vitest'
import { deviceName } from '~/composables/useSessions'

/**
 * Le nom d'appareil sert à **reconnaître** une session dans la liste : « celui
 * de la bibliothèque » se repère à sa date et à son navigateur. Ce qui se teste
 * ici, c'est qu'on ne se trompe pas de nom — annoncer « Chrome » là où c'est
 * Edge ferait fermer la mauvaise session.
 *
 * La fonction reste volontairement grossière : le reste (version, système) n'est
 * pas fiable dans un `User-Agent`, et l'interface préfère ne rien dire plutôt
 * que d'inventer.
 */
describe('deviceName', () => {
  it('reconnaît les navigateurs courants', () => {
    const cas: Array<[string, string]> = [
      [
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36',
        'Chrome',
      ],
      [
        'Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0',
        'Firefox',
      ],
      [
        'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
        'Safari',
      ],
      // Edge contient « chrome », Chrome contient « safari » : l'ordre des tests
      // est ce qui fait la justesse du nom.
      [
        'Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36 Edg/131.0',
        'Edge',
      ],
      [
        'Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 OPR/106.0',
        'Opera',
      ],
      ['curl/8.5.0', 'CLI'],
    ]
    for (const [agent, attendu] of cas) {
      expect(deviceName(agent), agent).toBe(attendu)
    }
  })

  it('ne devine rien plutôt que de se tromper', () => {
    // Vide : la page affichera son propre libellé générique (`sessionUnknown`).
    expect(deviceName('')).toBe('')
    expect(deviceName('   ')).toBe('')
    expect(deviceName('un client exotique/1.0')).toBe('')
  })
})
