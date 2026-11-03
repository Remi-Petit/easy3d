// Adresse du backend Rust, côté serveur uniquement.
//
// Séparé de `$lib/api` : ce dernier part aussi dans le bundle du navigateur, où
// `$env/dynamic/private` est interdit (SvelteKit refuse la compilation, et c'est
// la bonne barrière — un identifiant de backend n'a rien à faire côté client).
import { env } from '$env/dynamic/private'

export function baseBackend(): string {
  return (
    env.NUXT_HPCCAT_API_BASE ||
    env.EASY3D_API_BASE ||
    `http://127.0.0.1:${env.NUXT_HPCCAT_API_PORT || '8090'}`
  ).replace(/\/+$/, '')
}
