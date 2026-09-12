import type { DisplayMode } from '~/composables/useModels'
import {
  DEFAULT_FORMATS,
  fileIcon,
  formatFor,
  hasPreview,
  showsViewer,
  viewerOf,
  type FormatInfo,
  type Viewer,
} from '~/utils/formats'

/**
 * Formats reconnus par le backend, **partagés par toute l'application**.
 *
 * La liste est publiée par `useModels` (champ `formats` de `GET /models` et du
 * WebSocket) : elle reflète donc exactement ce que le backend sait lire.
 * Tant qu'aucune réponse n'est arrivée, on retombe sur `DEFAULT_FORMATS`.
 *
 * Les helpers prennent le chemin relatif du fichier et appliquent les règles
 * d'affichage (visionneuse, aperçu), pour que les composants n'aient aucune
 * extension à connaître.
 */
export function useFormats() {
  const formats = useState<FormatInfo[]>('easy3d:formats', () => DEFAULT_FORMATS)

  return {
    /** Liste brute des formats annoncés (réactive). */
    formats,
    /** Format reconnu pour un fichier, sinon `null`. */
    formatFor: (rel: string): FormatInfo | null => formatFor(formats.value, rel),
    /** Visionneuse d'un fichier : `mesh`, `gcode` ou `none`. */
    viewerOf: (rel: string): Viewer => viewerOf(formats.value, rel),
    /** Le backend sait générer un aperçu de ce fichier. */
    hasPreview: (rel: string): boolean => hasPreview(formats.value, rel),
    /** Rendu 3D interactif attendu pour ce fichier. */
    showsViewer: (rel: string, displayMode: DisplayMode): boolean =>
      showsViewer(formats.value, rel, displayMode),
    /** Emoji représentant un fichier. */
    fileIcon: (rel: string): string => fileIcon(formats.value, rel),
  }
}
