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
    return '/'
  }

  if (section === 'fichier') {
    const rel = decodeURIComponent(segments.slice(1).join('/'))
    const [folder, ...rest] = rel.split('/')
    // Un fichier à la racine n'a pas de dossier parent.
    if (!folder || rest.length === 0) {
      return '/'
    }
    return `/dossiers/${encodeURIComponent(folder)}`
  }

  return '/'
}
