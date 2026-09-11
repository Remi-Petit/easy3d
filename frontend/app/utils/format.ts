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

/** URL d'un fichier du répertoire modèles (proxifiée par Nitro vers le backend). */
export function fileUrl(rel: string): string {
  return `/api/file?path=${encodeURIComponent(rel)}`
}

/** Emoji représentant un fichier, d'après son extension. */
export function fileIcon(rel: string): string {
  const t = ext(rel)
  if (['stl', 'obj', '3mf'].includes(t)) return '🧊'
  if (['gcode', 'gco'].includes(t)) return '🖨'
  return '📄'
}

/** Le backend renvoie des timestamps en secondes (unix). */
export function toDate(ts: number | null): Date | null {
  return ts ? new Date(ts * 1000) : null
}

export function timeAgo(date: Date | null, now: Date = new Date()): string {
  if (!date) return '—'
  const diff = now.getTime() - date.getTime()
  const s = Math.floor(diff / 1000)
  if (s < 5) return 'à l’instant'
  if (s < 60) return `il y a ${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `il y a ${m} min`
  const h = Math.floor(m / 60)
  if (h < 24) return `il y a ${h}h`
  const d = Math.floor(h / 24)
  return `il y a ${d}j`
}
