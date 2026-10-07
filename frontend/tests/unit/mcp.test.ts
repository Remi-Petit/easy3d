import { describe, expect, it } from 'vitest'
import { mcpUrl } from '~/utils/mcp'

/**
 * L'adresse du serveur MCP est **recopiée** par l'utilisateur dans son éditeur :
 * une adresse fausse ne se voit qu'au moment où l'agent ne se connecte pas, et
 * le message d'erreur vient alors de l'éditeur, pas d'ici.
 */
describe('mcpUrl', () => {
  const page = { protocol: 'http:', hostname: 'localhost' }

  it('suit le réglage du déploiement quand il est posé', () => {
    expect(mcpUrl({ hpccatMcpBase: 'http://localhost:3101' }, page)).toBe(
      'http://localhost:3101/mcp',
    )
    // Le slash final est toléré (même convention que `collab.ts`).
    expect(mcpUrl({ hpccatMcpBase: 'https://easy3d.exemple.fr/' }, page)).toBe(
      'https://easy3d.exemple.fr/mcp',
    )
    // Une valeur vide ou absente est « non réglée », pas « adresse vide ».
    expect(mcpUrl({ hpccatMcpBase: '' }, page)).toBe('http://localhost:8090/mcp')
  })

  it('retombe sur le port publié par le compose livré', () => {
    expect(mcpUrl(undefined, page)).toBe('http://localhost:8090/mcp')
    // On garde l'hôte de la **page** (le serveur), pas celui du poste visiteur.
    expect(mcpUrl(undefined, { protocol: 'https:', hostname: 'catalogue.exemple.fr' })).toBe(
      'https://catalogue.exemple.fr:8090/mcp',
    )
  })

  it('ne casse pas sans origine connue (appel hors navigateur)', () => {
    expect(mcpUrl()).toBe('http://localhost:8090/mcp')
  })
})
