/** Utilitaires de formatage (auto-importés par Nuxt via `app/utils/`). */

export function basename(p: string): string {
  const parts = p.split(/[\\/]/)
  return parts[parts.length - 1] || p
}

export function ext(p: string): string {
  const b = basename(p)
  const i = b.lastIndexOf('.')
  return i >= 0 ? b.slice(i + 1).toLowerCase() : ''
}

/**
 * URL d'un fichier du répertoire modèles (proxifiée par Nitro vers le backend).
 *
 * `version` est une version opaque annoncée par le backend (champ
 * `image_version` du scan). Recopiée dans l'URL, elle permet au backend de
 * répondre `immutable` : le navigateur ne redemande plus l'aperçu aux
 * chargements suivants. Un fichier réécrit change de version, donc d'URL — le
 * cache ne peut pas servir une vignette périmée.
 *
 * Sans version, la réponse est `no-cache` : le navigateur stocke mais doit
 * revalider (le backend répond `304` avec son `ETag`).
 */
export function fileUrl(rel: string, version?: string | null): string {
  const v = version ? `&v=${encodeURIComponent(version)}` : ''
  return `/api/file?path=${encodeURIComponent(rel)}${v}`
}

/**
 * URL de téléchargement d'un fichier : même route, plus `download` pour que le
 * proxy réponde en `Content-Disposition: attachment`. Le nom enregistré est
 * celui du chemin (accents compris).
 */
export function fileDownloadUrl(rel: string): string {
  return `${fileUrl(rel)}&download=1`
}

/** Le backend renvoie des timestamps en secondes (unix). */
export function toDate(ts: number | null): Date | null {
  return ts ? new Date(ts * 1000) : null
}

/**
 * Temps relatif compact, découpé en **clé de traduction + valeur**.
 *
 * Le calcul est volontairement séparé du rendu : la fonction reste pure (donc
 * testable sans contexte i18n) et les libellés vivent dans `time.*` des fichiers
 * de langue. Le rendu est fait par le composable `useTimeAgo`.
 */
export interface RelativeTime {
  /** `now` = moins de 5 s ; sinon l’unité affichée. */
  key: 'now' | 'seconds' | 'minutes' | 'hours' | 'days'
  count: number
}

/** `null` = date inconnue (à rendre par `common.none`). */
export function relativeTime(date: Date | null, now: Date = new Date()): RelativeTime | null {
  if (!date) return null
  const s = Math.floor((now.getTime() - date.getTime()) / 1000)
  if (s < 5) return { key: 'now', count: 0 }
  if (s < 60) return { key: 'seconds', count: s }
  const m = Math.floor(s / 60)
  if (m < 60) return { key: 'minutes', count: m }
  const h = Math.floor(m / 60)
  if (h < 24) return { key: 'hours', count: h }
  return { key: 'days', count: Math.floor(h / 24) }
}
