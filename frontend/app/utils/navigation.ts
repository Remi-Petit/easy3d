/**
 * Destinations de navigation déduites d'un chemin.
 *
 * Logique **pure**, testable sans routeur.
 */

/**
 * Page « parente » d'un chemin : c'est là que ramène le bouton de retour quand
 * on ne peut pas revenir à la position précédente (lien direct, rechargement).
 *
 * - `/fichier/DemaAuto%2Fx.stl` → `/dossiers/DemaAuto` (le dossier du fichier) ;
 * - `/fichier/x.stl` (racine) → `/` ;
 * - `/dossiers/DemaAuto` → `/` ;
 * - tout le reste → `/`.
 *
 * Un fichier rangé dans un sous-dossier appartient au dossier de **premier
 * niveau** : c'est ainsi que le backend regroupe les fichiers (voir
 * `scan_models`), et donc la page où il est réellement listé.
 */
export function parentPath(path: string): string {
  const segments = path.split('/').filter(Boolean)
  const [section] = segments

  if (section === 'dossiers') {
    // Le paramètre porte le chemin relatif du dossier (encodé, comme pour les
    // fichiers) : un sous-dossier remonte à son parent, un dossier de premier
    // niveau à l'accueil.
    const parts = relSegments(segments)
    return parts.length > 1
      ? folderHref(parts.slice(0, -1).join('/'))
      : '/'
  }

  if (section === 'fichier') {
    const [folder, ...rest] = relSegments(segments)
    // Un fichier à la racine n'a pas de dossier parent.
    if (!folder || rest.length === 0) {
      return '/'
    }
    return folderHref(folder)
  }

  return '/'
}

/** URL de la page d'un dossier, à partir de son chemin relatif. */
export function folderHref(rel: string): string {
  return `/dossiers/${encodeURIComponent(rel)}`
}

/**
 * Chemin relatif porté par les segments qui suivent la section, décodé puis
 * découpé (`['dossiers', 'a%2Fb']` → `['a', 'b']`).
 */
function relSegments(segments: string[]): string[] {
  return decodeURIComponent(segments.slice(1).join('/'))
    .split('/')
    .filter(Boolean)
}
