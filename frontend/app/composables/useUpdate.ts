import { isNewer } from '~/utils/version'

/**
 * Réponse de `GET /api/update` (voir `server/api/update.get.ts`).
 *
 * Le serveur ne rend que des **faits** (version locale, dernier tag publié,
 * page de la release) : décider s'il y a une mise à jour est le travail de
 * `isNewer`, testé à part (`tests/unit/version.test.ts`).
 */
interface ReleaseInfo {
  current: string
  latest: string | null
  url: string
}

/**
 * Nouvelle version publiée : prévenir l'administration, et elle seule.
 *
 * Mettre à jour une installation est un acte d'administration : la pastille du
 * pied de sidebar n'apparaît donc qu'à qui a le droit d'entrer dans les
 * réglages (`config.read`). Sans comptes (`EASY3D_AUTH` éteint), le droit
 * répond `true` partout — la pastille est alors visible, comme le reste de
 * l'administration. Le test lui-même est fait par l'appelant
 * (`layouts/default.vue`), qui seul tient l'état d'authentification ; ce
 * composable ne fait que rapporter la version publiée.
 *
 * **Pas de sondage côté navigateur.** L'état est partagé (`useState`) et
 * demandé une fois par chargement de l'application ; l'endpoint, lui, met sa
 * réponse en cache six heures. Un onglet ouvert toute la journée ne génère donc
 * qu'une requête à GitHub pour toutes les visites, et un rafraîchissement de
 * page ne relance rien de coûteux (le cache HTTP de la réponse fait le reste).
 *
 * Un échec (hors ligne, miroir sans API) rend `available = false` : on ne
 * prévient que de ce qu'on a réellement vu, jamais d'une supposition.
 */
export function useUpdate() {
  /** Dernière réponse du serveur, ou `null` tant qu'aucune n'est arrivée. */
  const info = useState<ReleaseInfo | null>('easy3d:update', () => null)
  const loading = useState<boolean>('easy3d:update:loading', () => false)

  // La version qui tourne **vraiment** : celle du build, injectée par
  // `nuxt.config.ts` — la même que celle affichée dans le pied de sidebar.
  const current = String(useRuntimeConfig().public.version)

  /** Version publiée, sans son `v` (ou `null` si inconnue). */
  const latest = computed(() => info.value?.latest ?? null)

  /** `true` si une version strictement plus récente est sortie. */
  const available = computed(() => isNewer(latest.value, current))

  /** Page de la release (ou du dépôt) à ouvrir. */
  const url = computed(() => info.value?.url ?? '')

  /**
   * Interroge le serveur, au plus une fois par chargement de l'application.
   *
   * `force` sert au cas où l'on veut revérifier sans recharger la page (après
   * une mise à jour, par exemple).
   */
  async function refresh(force = false) {
    if (loading.value) return
    if (info.value && !force) return

    loading.value = true
    try {
      info.value = await $fetch<ReleaseInfo>('/api/update')
    } catch {
      // Muet : une vérification de version qui échoue ne doit rien casser, ni
      // afficher d'erreur — l'absence de pastille est la seule conséquence.
      info.value = null
    } finally {
      loading.value = false
    }
  }

  return { current, latest, available, url, loading, refresh }
}
