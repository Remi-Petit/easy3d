/** Préparation d'un envoi de fichiers vers le backend (voir `POST /api/upload`). */

/** Un fichier choisi ou déposé, avec le chemin d'où il vient. */
export interface UploadPick {
  file: File
  /**
   * Chemin source, séparateurs `/` :
   * - sélection de fichiers : le nom seul (`piece.stl`) ;
   * - sélection de dossier : `webkitRelativePath` (« Lot/sous/piece.stl ») ;
   * - dépôt : chemin de l'entrée déposée (`FileSystemEntry.fullPath`).
   */
  path: string
}

/** Un fichier à envoyer, et sa destination dans le catalogue. */
export interface UploadTask {
  file: File
  /** Chemin relatif cible (séparateurs `/`) : `<dossier>/<chemin du fichier>`. */
  rel: string
}

/** Entrée d'un dépôt : ce qu'on utilise de `FileSystemEntry`. */
export interface DropEntry {
  isFile: boolean
  fullPath: string
  /** Lit le fichier (une seule fois, sur une entrée de fichier). */
  file?: (resolve: (file: File) => void, reject: (reason?: unknown) => void) => void
  /** Ouvre un dossier (une seule fois, sur une entrée de dossier). */
  createReader?: () => {
    readEntries: (resolve: (entries: DropEntry[]) => void, reject: (reason?: unknown) => void) => void
  }
}

/** Données d'un dépôt : ce qu'on utilise de `DataTransfer`. */
export interface DropData {
  items?: ArrayLike<{
    kind: string
    webkitGetAsEntry?: () => DropEntry | null
    getAsFile?: () => File | null
  }>
}

/**
 * Chemin relatif d'un fichier, ou `null` s'il ne peut pas être envoyé.
 *
 * Les chemins que le backend refuserait sont écartés ici aussi (composant caché,
 * `..`, séparateurs Windows) : autant ne pas envoyer une requête pour rien.
 */
export function uploadRel(path: string, folder = ''): string | null {
  const parts = [folder, ...path.replace(/\\/g, '/').split('/')].filter(Boolean)
  if (!parts.length) return null
  if (parts.some((part) => part === '.' || part === '..' || part.startsWith('.'))) {
    return null
  }
  return parts.join('/')
}

/** Fichiers d'un `<input type="file">` (`webkitdirectory` pour un dossier). */
export function inputPicks(files: File[]): UploadPick[] {
  return files.map((file) => ({ file, path: file.webkitRelativePath || file.name }))
}

/**
 * Fichiers d'un dépôt, **dossiers compris**.
 *
 * Les entrées sont collectées avant tout `await` : le `DataTransfer` n'est
 * lisible que pendant l'événement `drop`, après quoi ses `items` sont vidés.
 * La lecture d'un dossier se fait ensuite par lots — l'API du navigateur rend au
 * plus 100 entrées par appel, et un tableau vide quand il n'y en a plus.
 */
export async function dropPicks(data: DropData): Promise<UploadPick[]> {
  const picks: UploadPick[] = []
  const entries: DropEntry[] = []

  for (const item of Array.from(data.items ?? [])) {
    if (item.kind !== 'file') continue
    const entry = item.webkitGetAsEntry?.()
    if (entry) {
      entries.push(entry)
      continue
    }
    // Navigateur sans API d'entrées : le fichier seul, sans son arborescence.
    const file = item.getAsFile?.()
    if (file) picks.push({ file, path: file.name })
  }

  for (const entry of entries) {
    await walkEntry(entry, picks)
  }
  return picks
}

/** Parcours récursif d'une entrée déposée. */
async function walkEntry(entry: DropEntry, out: UploadPick[]): Promise<void> {
  if (entry.isFile) {
    if (!entry.file) return
    const file = await new Promise<File>((resolve, reject) => entry.file!(resolve, reject))
    // `fullPath` commence par `/` (racine du dépôt) : la racine du catalogue
    // prend sa place.
    out.push({ file, path: entry.fullPath.replace(/^\/+/, '') })
    return
  }

  const reader = entry.createReader?.()
  if (!reader) return
  for (;;) {
    const batch = await new Promise<DropEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject),
    )
    if (!batch.length) break
    for (const child of batch) {
      await walkEntry(child, out)
    }
  }
}

/**
 * Envois à effectuer pour une sélection, dans l'ordre donné par le navigateur.
 *
 * `folder` est le dossier d'accueil (celui de la page courante) : vide pour la
 * racine du catalogue.
 */
export function uploadPlan(picks: UploadPick[], folder = ''): UploadTask[] {
  const tasks: UploadTask[] = []
  for (const pick of picks) {
    const rel = uploadRel(pick.path, folder)
    if (rel) tasks.push({ file: pick.file, rel })
  }
  return tasks
}
