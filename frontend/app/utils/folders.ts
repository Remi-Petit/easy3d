/**
 * Hiérarchie des dossiers, reconstituée à partir des chemins relatifs.
 *
 * Le backend envoie tout **à plat** — les fichiers et les sous-dossiers portent
 * leur `rel` (`Maison/sous/b.stl`) — pour que le JSON ne dépende pas de la
 * profondeur de l'arborescence. C'est ici qu'on redonne une structure : enfants
 * directs d'un dossier, fichiers d'un sous-arbre.
 *
 * Logique pure, testable sans backend.
 */
import type { FileInfo, FolderInfo, SubFolderInfo } from '~/composables/useModels'

/** Segments non vides d'un chemin relatif (`Maison/sous` → `['Maison', 'sous']`). */
export function segments(rel: string): string[] {
  return rel.split('/').filter(Boolean)
}

/**
 * Chemin du dossier parent (`a/b/c` → `a/b`).
 *
 * Rend `''` pour un chemin de premier niveau (`Maison`), ce qui désigne la racine
 * du catalogue : c'est la convention des comparaisons ci-dessous.
 */
export function parentOf(rel: string): string {
  return segments(rel).slice(0, -1).join('/')
}

/** Nombre de niveaux d'un chemin (`Maison` → 1, `Maison/sous` → 2). */
export function depth(rel: string): number {
  return segments(rel).length
}

/** Éléments rangés **directement** dans `folder` (sous-dossiers ou fichiers). */
export function childrenOf<T extends { rel: string }>(items: T[], folder: string): T[] {
  return items.filter((item) => parentOf(item.rel) === folder)
}

/** Sous-dossiers **directs** d'un dossier (ses petits-enfants ne le sont pas). */
export function directFolders(subfolders: SubFolderInfo[], rel: string): SubFolderInfo[] {
  return childrenOf(subfolders, rel)
}

/** Fichiers rangés **directement** dans un dossier, hors sous-dossiers. */
export function directFiles(files: FileInfo[], rel: string): FileInfo[] {
  return childrenOf(files, rel)
}

/** Fichiers d'un dossier et de tout son sous-arbre. */
export function filesUnder(files: FileInfo[], rel: string): FileInfo[] {
  const prefix = `${rel}/`
  return files.filter((file) => file.rel.startsWith(prefix))
}

/** Ce qu'il faut pour afficher un dossier, de premier niveau ou non. */
export interface FolderView {
  /** Chemin relatif à la racine : identifiant, et clé des notes. */
  rel: string
  /** Nom du dossier seul (dernier segment). */
  name: string
  /** Fichiers contenus, récursivement. */
  count: number
  modified: number | null
  note?: string | null
  /** Fichiers du dossier **et** de ses sous-dossiers. */
  files: FileInfo[]
  /** Sous-dossiers **directs**. */
  subfolders: SubFolderInfo[]
}

/**
 * Vue d'un dossier à partir de son chemin relatif, ou `null` s'il n'existe pas.
 *
 * `top` est le dossier de premier niveau qui porte le scan : le backend ne
 * détaille que ceux-là, les sous-dossiers étant décrits à plat dans
 * `subfolders` (d'où la recherche par `rel`).
 */
export function folderView(top: FolderInfo, rel: string): FolderView | null {
  const all = top.subfolders ?? []

  if (rel === top.name) {
    return {
      rel: top.name,
      name: top.name,
      count: top.count,
      modified: top.modified,
      note: top.note,
      files: top.files,
      subfolders: directFolders(all, top.name),
    }
  }

  const found = all.find((sub) => sub.rel === rel)
  if (!found) return null

  return {
    rel: found.rel,
    name: found.name,
    count: found.count,
    modified: found.modified,
    note: found.note,
    files: filesUnder(top.files, found.rel),
    subfolders: directFolders(all, found.rel),
  }
}
