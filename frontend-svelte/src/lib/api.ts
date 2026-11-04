// Accès à l'API Rust.
//
// Ce module est importé **aussi par le navigateur** : il ne doit donc rien lire
// de l'environnement du serveur (SvelteKit le refuse, à juste titre). L'adresse
// du backend vit dans `$lib/server/backend.ts`, réservée au serveur.
export type Fichier = {
  path: string
  rel: string
  created: number | null
  modified: number | null
  image?: string
  image_version?: string
  note?: string
}

export type Dossier = {
  name: string
  rel?: string
  count: number
  modified: number | null
  files: Fichier[]
  subfolders?: {
    rel: string
    name: string
    count: number
    modified: number | null
    note?: string
  }[]
  note?: string
}

export type Catalogue = {
  files: Fichier[]
  folders: Record<string, Dossier>
  count: number
  formats: { name: string; extensions: string[]; preview: boolean; viewer: string }[]
  config: { display?: { mode?: string } }
}

/** Catalogue complet, lu côté serveur (SSR) directement sur le backend. */
export async function chargerCatalogue(
  fetchImpl: typeof fetch,
  base: string,
): Promise<Catalogue | null> {
  try {
    const res = await fetchImpl(`${base}/models`)
    if (!res.ok) return null
    return (await res.json()) as Catalogue
  } catch {
    return null
  }
}

/** URL d'un aperçu, versionnée comme dans l'app Nuxt (cache qui se renouvelle). */
export function urlApercu(image: string, version?: string): string {
  // `&` et non `?` : `path` est déjà dans la requête. Avec un second `?`, le
  // backend recevait `path=.easy3d-thumbs/...?v=...` et répondait 404 pour
  // **toutes** les vignettes (vérifié dans le navigateur).
  const suffixe = version ? `&v=${encodeURIComponent(version)}` : ''
  return `/api/file?path=${encodeURIComponent(image)}${suffixe}`
}

/** URL de téléchargement d'un modèle. */
export function urlFichier(rel: string): string {
  return `/api/file?path=${encodeURIComponent(rel)}`
}

/** Extension d'un chemin relatif, en minuscules. */
export function extensionDe(rel: string): string {
  const point = rel.lastIndexOf('.')
  return point < 0 ? '' : rel.slice(point + 1).toLowerCase()
}

/** Nom de base d'un chemin relatif (`Maison/Toit/x.stl` → `x.stl`). */
export function nomDeBase(rel: string): string {
  return rel.split('/').filter(Boolean).pop() ?? rel
}
