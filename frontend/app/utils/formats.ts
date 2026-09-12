// Import explicite : l'auto-import Nuxt n'existe pas sous Vitest.
import type { DisplayMode } from '~/composables/useModels'
import { ext } from '~/utils/format'

/**
 * Connaissance des formats **côté client**.
 *
 * Elle vient entièrement du backend (`GET /models` → `formats`, miroir de
 * `formats::FormatInfo` en Rust) : aucune extension n'est codée en dur ici.
 * Ajouter un format au backend suffit donc pour que le catalogue, les icônes et
 * les règles d'affichage le prennent en compte.
 *
 * Seul le *loader* three.js reste côté frontend (voir `ModelViewer`) : le
 * backend annonce la nature du contenu (`viewer`), pas la façon de le charger.
 */

/** Nature du contenu d'un format, telle que l'annonce le backend. */
export type Viewer = 'mesh' | 'gcode' | 'none'

/** Descripteur d'un format reconnu. */
export interface FormatInfo {
  /** Nom lisible (ex : `3MF`). */
  name: string
  /** Extensions gérées, minuscules et sans point. */
  extensions: string[]
  /** Le backend sait générer un aperçu PNG de ce format. */
  preview: boolean
  /** Visionneuse adaptée au contenu. */
  viewer: Viewer
}

/**
 * Repli tant que le backend n'a pas répondu (premier rendu, backend plus ancien
 * qui n'enverrait pas `formats`). À garder aligné sur `formats/` du backend.
 */
export const DEFAULT_FORMATS: FormatInfo[] = [
  { name: 'STL', extensions: ['stl'], preview: true, viewer: 'mesh' },
  { name: 'OBJ', extensions: ['obj'], preview: true, viewer: 'mesh' },
  { name: '3MF', extensions: ['3mf'], preview: true, viewer: 'mesh' },
  { name: 'G-code', extensions: ['gcode', 'gco'], preview: true, viewer: 'gcode' },
]

/** Format reconnu pour un chemin (d'après son extension), sinon `null`. */
export function formatFor(formats: FormatInfo[], rel: string): FormatInfo | null {
  const type = ext(rel)
  if (!type) return null
  return formats.find((format) => format.extensions.includes(type)) ?? null
}

/** Visionneuse d'un fichier : `mesh`, `gcode`, ou `none` si le format est inconnu. */
export function viewerOf(formats: FormatInfo[], rel: string): Viewer {
  return formatFor(formats, rel)?.viewer ?? 'none'
}

/** Le backend sait-il générer un aperçu de ce fichier ? */
export function hasPreview(formats: FormatInfo[], rel: string): boolean {
  return formatFor(formats, rel)?.preview ?? false
}

/**
 * Faut-il un rendu 3D interactif ?
 *
 * Toujours pour un maillage ; pour un G-code seulement en mode `3d` (en mode
 * `image`, on préfère l'aperçu statique écrit par le slicer).
 */
export function showsViewer(formats: FormatInfo[], rel: string, displayMode: DisplayMode): boolean {
  const viewer = viewerOf(formats, rel)
  return viewer === 'mesh' || (viewer === 'gcode' && displayMode === '3d')
}

/** Emoji représentant un fichier, d'après la visionneuse de son format. */
export function fileIcon(formats: FormatInfo[], rel: string): string {
  switch (viewerOf(formats, rel)) {
    case 'mesh':
      return '🧊'
    case 'gcode':
      return '🖨'
    default:
      return '📄'
  }
}
