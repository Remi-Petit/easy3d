/**
 * Informations d'impression d'un fichier (`GET /print-info`).
 *
 * Le backend les ramène au **vocabulaire commun des slicers** (voir
 * `backend/src/formats/print_info.rs` : les clés changent d'un slicer à
 * l'autre, l'interface non). Tous les champs sont facultatifs : un STL n'en a
 * aucun, et aucun slicer n'annonce tout.
 */
export interface PrintInfo {
  /** Durée d'impression estimée, en secondes. */
  duration_seconds?: number
  /** Matière annoncée (`PLA`, `PETG`…). */
  material?: string
  /** Poids de filament, en grammes (seulement si le slicer l'annonce). */
  filament_grams?: number
  /** Hauteur de couche, en mm. */
  layer_height?: string
  /** Hauteur de la première couche, en mm. */
  first_layer_height?: string
  /** Diamètre de buse, en mm. */
  nozzle?: string
  /** Imprimante visée. */
  printer?: string
  /** Remplissage annoncé (ex. `100%`). */
  fill_density?: string
  /** Supports activés. */
  support?: boolean
  /** Encombrement de l'objet, en mm : `[X, Y, Z]`. */
  size_mm?: [number, number, number]
  /** Slicer qui a produit le fichier. */
  slicer?: string
  /** Taille du fichier sur le disque, en octets. */
  bytes?: number
}

/**
 * `true` si le fichier annonce quelque chose sur l'impression.
 *
 * La **taille du fichier** ne compte pas : c'est la seule chose qu'un STL puisse
 * dire, et une carte « Impression » qui n'apprendrait que ça remplirait la
 * colonne pour rien (elle est déjà dans les propriétés du fichier).
 */
export function hasPrintFacts(info: PrintInfo | null | undefined): boolean {
  if (!info) return false
  return Object.entries(info).some(
    ([champ, valeur]) => champ !== 'bytes' && valeur !== undefined && valeur !== null,
  )
}

/**
 * Durée d'impression en clair : `43 min`, `2 h 05`, `31 h 20`.
 *
 * Les unités sont des **symboles** (`h`, `min`) compris dans les quatre langues
 * de l'interface : les écrire en toutes lettres demanderait quatre traductions
 * pour une ligne qui n'en dit pas plus.
 */
export function formatDuration(seconds: number): string {
  const minutes = Math.max(0, Math.round(seconds / 60))
  if (minutes < 60) return `${minutes} min`
  const heures = Math.floor(minutes / 60)
  const reste = minutes % 60
  return reste ? `${heures} h ${String(reste).padStart(2, '0')}` : `${heures} h`
}

/**
 * Taille d'un fichier, dans la langue de l'interface (`4,1 Mo` / `4.1 MB`).
 *
 * `Intl` plutôt qu'un suffixe écrit à la main : c'est lui qui connaît
 * l'abréviation et la virgule décimale de chaque langue.
 */
export function formatBytes(bytes: number, locale?: string): string {
  const enMo = bytes >= 1_000_000
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: enMo ? 'megabyte' : 'kilobyte',
    maximumFractionDigits: 1,
  }).format(bytes / (enMo ? 1_000_000 : 1_000))
}
