/**
 * Fournisseur d'identité (SSO) : ce qui se vérifie sans serveur.
 *
 * Deux règles, les mêmes que partout ailleurs dans cette interface :
 *
 * - **Le secret ne circule pas.** Le backend ne renvoie que `***` quand un
 *   secret est enregistré, et ce marqueur veut dire « garde celui que tu as »
 *   lorsqu'on le renvoie (voir `Oidc::merge_secret`). Un champ vidé, lui, veut
 *   dire « efface-le ».
 * - **Les phrases vivent ici, les faits viennent du serveur.** Le backend
 *   annonce ce qui est figé par l'environnement (`locked`) et ce qui manque
 *   (`problem`), sous forme d'identifiants ; c'est la page qui traduit.
 */

/** Champs du bloc `oidc` que l'environnement impose. */
export type OidcField =
  | 'enabled'
  | 'issuer'
  | 'client_id'
  | 'client_secret'
  | 'scopes'
  | 'provisioning'

/** Modes de provisionnement, tels que le backend les nomme. */
export const PROVISIONING_MODES = ['auto', 'manual', 'approval'] as const
export type ProvisioningMode = (typeof PROVISIONING_MODES)[number]

/**
 * Bloc `oidc` de `config.yml` (voir `config::Oidc`).
 *
 * `provisioning` est une **chaîne** et non une union stricte : un backend plus
 * récent peut en annoncer un mode que cette interface ne connaît pas encore, et
 * elle doit pouvoir l'enregistrer tel quel plutôt que de le perdre.
 */
export interface OidcConfig {
  enabled?: boolean
  issuer?: string | null
  client_id?: string | null
  client_secret?: string | null
  scopes?: string[]
  provisioning?: string | null
}

/** Ce que `/api/config` annonce du SSO (`api::OidcInfo`). */
export interface OidcInfo {
  /** Champs que l'environnement (`EASY3D_OIDC_*`) impose : grisés dans /admin. */
  locked: OidcField[]
  /** `true` si le SSO est réellement utilisable. */
  active: boolean
  /** Ce qui manque (`issuer`, `client_id`, `client_secret`), le cas échéant. */
  problem?: string | null
}

/**
 * Bloc `oidc` **vierge**, pour les installations qui n'y ont jamais touché (le
 * backend omet alors la section entière du YAML).
 */
export function emptyOidc(): OidcConfig {
  return {
    enabled: false,
    issuer: null,
    client_id: null,
    client_secret: null,
    scopes: [],
    provisioning: 'auto',
  }
}

/**
 * Bloc à envoyer au backend.
 *
 * `provisioning` inconnu retombe sur `auto` : c'est le mode le moins surprenant
 * (le compte se crée tout seul avec le rôle par défaut), et un backend plus
 * récent ne doit pas faire échouer un enregistrement à cause d'une valeur qu'on
 * ne connaît pas encore.
 */
export function oidcBody(oidc: OidcConfig | undefined, locks: OidcField[]): OidcConfig {
  const base = { ...emptyOidc(), ...(oidc ?? {}) }
  const mode: ProvisioningMode = PROVISIONING_MODES.includes(base.provisioning as ProvisioningMode)
    ? (base.provisioning as ProvisioningMode)
    : 'auto'

  // Un champ figé par l'environnement est renvoyé tel quel : l'interface l'affiche
  // grisé, et l'écraser ici n'aurait aucun effet côté serveur (l'environnement
  // gagne) tout en laissant croire le contraire dans le fichier.
  const locked = (field: OidcField) => locks.includes(field)

  return {
    enabled: locked('enabled') ? true : !!base.enabled,
    issuer: locked('issuer') ? (base.issuer ?? null) : (base.issuer?.trim() ?? '') || null,
    client_id: locked('client_id')
      ? (base.client_id ?? null)
      : (base.client_id?.trim() ?? '') || null,
    client_secret: (base.client_secret?.trim() ?? '') || null,
    scopes: locked('scopes')
      ? (base.scopes ?? [])
      : (base.scopes ?? []).map((scope) => scope.trim()).filter(Boolean),
    provisioning: locked('provisioning') ? base.provisioning || 'auto' : mode,
  }
}

/** Adresse de retour à déclarer chez le fournisseur. */
export function redirectUri(origin: string): string {
  return `${origin.replace(/\/+$/, '')}/api/auth/oidc/callback`
}

/**
 * `true` si le SSO est à la fois activé et utilisable.
 *
 * Sert à la page d'administration pour distinguer « éteint » de « allumé mais
 * incomplet » — deux situations qui n'appellent pas le même geste.
 */
export function oidcUsable(info: OidcInfo | null | undefined): boolean {
  return !!info?.active
}
