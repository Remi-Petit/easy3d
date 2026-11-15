/**
 * Dernière version **publiée** du projet (voir `useUpdate`).
 *
 * Pourquoi ici et non dans le backend Rust : c'est un appel sortant vers une
 * API publique, sans état ni droit à vérifier — et Nitro sait déjà le mettre en
 * cache. Un appel par instance et par `MAX_AGE`, **quel que soit le nombre
 * d'administrateurs** ; faire la requête depuis le navigateur multiplierait les
 * appels par le nombre de postes ouverts (et par leurs onglets), pour se
 * heurter au quota anonyme de GitHub (60 requêtes par heure et par IP).
 *
 * Il n'y a **pas de polling** : le cache fait le travail. Une copie de
 * l'interface restée ouverte six heures ne revoit rien, et une visite toutes
 * les cinq minutes ne coûte rien non plus — c'est la première requête après
 * expiration qui interroge GitHub.
 *
 * Une panne (poste hors ligne, GitHub injoignable, proxy filtrant) laisse
 * l'erreur remonter : `defineCachedFunction` ne retient que les réponses, donc
 * un échec n'est jamais gravé pour six heures. L'interface, elle, n'affiche
 * simplement pas de pastille — on ne prévient que de ce qu'on a vraiment vu.
 */
const MAX_AGE = 6 * 60 * 60 // secondes

/**
 * URL de l'API des releases pour un dépôt GitHub, sinon `null`.
 *
 * Un dépôt **miroir** (Gitea, GitLab, copie locale) n'a pas cette API : y
 * interroger GitHub renverrait la version de l'amont, donc une alerte qui ne
 * correspond pas au code déployé. Pas de vérification possible, pas de
 * pastille — c'est plus honnête qu'une fausse alerte.
 */
function releasesApi(repoUrl: string): string | null {
  const match = /^https?:\/\/(?:www\.)?github\.com\/([^/]+)\/([^/?#]+?)(?:\.git)?\/?$/.exec(repoUrl)
  if (!match) return null
  return `https://api.github.com/repos/${match[1]}/${match[2]}/releases/latest`
}

export default defineCachedEventHandler(
  async (event) => {
    const config = useRuntimeConfig(event)
    const current: string = String(config.public.version)
    const repoUrl: string = String(config.public.repoUrl)
    const api = releasesApi(repoUrl)

    if (!api) return { current, latest: null, url: repoUrl }

    const release = await $fetch<{ tag_name?: string, html_url?: string }>(api, {
      headers: {
        // GitHub refuse une requête sans `User-Agent` ; l'en-tête `accept`
        // fige la version de l'API qu'on attend (`tag_name`, `html_url`).
        accept: 'application/vnd.github+json',
        'user-agent': `easy3d/${current}`,
      },
      // Trois secondes suffisent pour joindre l'API ; au-delà, la page n'a pas
      // à attendre un réseau qui ne répond pas — l'appel repartira au tic
      // suivant, et la pastille n'est qu'un bonus.
      timeout: 3000,
    })

    return {
      current,
      latest: String(release.tag_name ?? '').replace(/^v/i, '') || null,
      // La page de la release plutôt que l'adresse du dépôt : c'est là qu'on
      // lit ce qui change avant de mettre à jour.
      url: release.html_url || `${repoUrl}/releases/latest`,
    }
  },
  {
    maxAge: MAX_AGE,
    // `swr` : une requête arrivée après expiration reçoit la valeur connue
    // pendant que la nouvelle se charge. La pastille ne fait jamais attendre.
    swr: true,
    name: 'update',
    // Une seule entrée, quelle que soit la requête : le résultat ne dépend pas
    // de l'appelant.
    getKey: () => 'latest',
  },
)
