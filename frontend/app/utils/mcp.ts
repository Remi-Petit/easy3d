/**
 * Adresse du serveur MCP, telle qu'on la **montre** à l'utilisateur.
 *
 * Le serveur MCP est le backend lui-même (`/mcp`), et Nitro ne le relaie **pas**
 * — contrairement à `/ws` et `/collab/*`. L'adresse ne se déduit donc pas de
 * l'origine de la page comme le fait le temps réel (voir `utils/collab.ts`) : il
 * faut la connaître.
 *
 * Priorité à `NUXT_PUBLIC_HPCCAT_MCP_BASE` (posée par le déploiement, comme
 * `NUXT_PUBLIC_HPCCAT_WS_BASE`) ; sinon le port publié par le
 * `docker-compose.yml` livré (`8090`), sur l'hôte qui sert l'interface. C'est le
 * seul défaut qu'on puisse deviner — un déploiement qui publie l'API ailleurs
 * doit poser la variable, sans quoi l'adresse affichée serait fausse.
 *
 * Logique **pure**, testable sans navigateur.
 */

/** Configuration publique du frontend (`runtimeConfig.public`). */
export interface McpConfig {
  hpccatMcpBase?: string
}

/** Origine de la page, telle qu'il faut pour en déduire une adresse. */
export interface McpOrigin {
  protocol: string
  hostname: string
}

/** Port publié par le `docker-compose.yml` livré, pour l'API **et** le MCP. */
const PORT_DEFAUT = 8090

/** Adresse du serveur MCP, avec son chemin. */
export function mcpUrl(pub?: McpConfig, here?: McpOrigin): string {
  const configure = (pub?.hpccatMcpBase || '').replace(/\/+$/, '')
  if (configure) return `${configure}/mcp`

  const origine = here ?? (typeof location === 'undefined' ? undefined : location)
  const protocole = origine?.protocol === 'https:' ? 'https:' : 'http:'
  return `${protocole}//${origine?.hostname || 'localhost'}:${PORT_DEFAUT}/mcp`
}
