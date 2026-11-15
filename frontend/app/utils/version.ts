/**
 * Comparaison de versions publiées.
 *
 * `frontend/package.json` écrit `0.4.5`, le tag de release porte `v0.4.5` : on
 * compare donc des **nombres**, segment par segment, et non des chaînes —
 * `0.10.0` est plus récent que `0.9.0`, ce qu'un `<` de chaînes dirait à
 * l'envers (c'est le piège classique d'un tri de versions en texte).
 *
 * Les suffixes (`-rc1`, `+build`) sont ignorés : l'API GitHub
 * `/releases/latest` ne renvoie déjà pas de pré-version, et une `0.5.0-rc1`
 * face à une `0.5.0` ne doit surtout pas passer pour une mise à jour.
 */

/** Découpe `v0.4.5` / `0.4.5-rc1` en segments numériques. */
function segments(version: string): number[] {
  const [core = ''] = version.trim().split(/[-+]/, 1)
  return core
    .replace(/^v/i, '')
    .split('.')
    .map((part) => Number.parseInt(part, 10) || 0)
}

/**
 * `true` si `latest` est **strictement** plus récente que `current`.
 *
 * Une version illisible (`latest` vide, ou une balise qui n'est pas un numéro)
 * répond `false` : mieux vaut ne rien annoncer qu'une fausse alerte.
 */
export function isNewer(latest: string | null | undefined, current: string): boolean {
  if (!latest) return false
  const a = segments(latest)
  const b = segments(current)
  // Comparable seulement si les deux commencent par un chiffre : `0.x` par
  // défaut, `1.0.0-beta` reste un numéro, `latest` n'en est pas un.
  if (!/^\s*v?\d/i.test(latest.trim())) return false

  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const left = a[index] ?? 0
    const right = b[index] ?? 0
    if (left !== right) return left > right
  }
  return false
}
