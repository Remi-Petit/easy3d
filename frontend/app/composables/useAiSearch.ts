import type { AiOutcome, AiProvider } from '~/utils/ai'
import type { ConfigResponse } from '~/composables/useModels'
import { aiConfigured } from '~/utils/ai'

/**
 * Recherche assistée : état **partagé** (barre de recherche, panneau de
 * résultats, administration).
 *
 * Tout passe par `useState` : la barre d'outils et le panneau de résultats sont
 * deux composants distincts, et la page d'administration doit pouvoir rafraîchir
 * l'état du bouton après avoir enregistré un fournisseur.
 *
 * Rien ne part tant qu'aucun fournisseur n'est configuré : le bouton reste
 * désactivé (voir `layouts/default.vue`), et l'absence de configuration est
 * relue depuis `/api/config` — la même source que le reste de l'interface.
 */
export function useAiSearch() {
  const { t } = useI18n()

  /** Fournisseurs proposés par le backend (aucune liste en dur ici). */
  const providers = useState<AiProvider[]>('easy3d:ai:providers', () => [])
  /** `true` si un fournisseur est choisi : le bouton de recherche s'active. */
  const configured = useState<boolean>('easy3d:ai:configured', () => false)
  /** Le mode « recherche assistée » est ouvert (le catalogue est masqué). */
  const mode = useState<boolean>('easy3d:ai:mode', () => false)
  const query = useState<string>('easy3d:ai:query', () => '')
  const loading = useState<boolean>('easy3d:ai:loading', () => false)
  const error = useState<string | null>('easy3d:ai:error', () => null)
  const outcome = useState<AiOutcome | null>('easy3d:ai:outcome', () => null)
  /** La question réellement envoyée (le champ peut être retouché après coup). */
  const asked = useState<string>('easy3d:ai:asked', () => '')

  /**
   * Relit la configuration et la liste des fournisseurs.
   *
   * Appelée au montage de la barre d'outils et après un enregistrement dans
   * l'administration. Un backend muet laisse la recherche **désactivée** :
   * plutôt qu'un bouton qui échouerait à coup sûr.
   */
  async function refresh() {
    try {
      const config = await $fetch<ConfigResponse>('/api/config')
      configured.value = aiConfigured(config.config?.ai)

      if (!providers.value.length) {
        providers.value = await $fetch<AiProvider[]>('/api/ai/providers')
      }
    } catch {
      configured.value = false
    }
  }

  /** Ouvre ou ferme la recherche assistée. */
  function toggle() {
    mode.value = !mode.value
    error.value = null
  }

  /** Quitte la recherche assistée (le catalogue reprend sa place). */
  function close() {
    mode.value = false
    error.value = null
  }

  /** Efface la question et les propositions. */
  function reset() {
    query.value = ''
    asked.value = ''
    outcome.value = null
    error.value = null
  }

  /**
   * Lance la recherche sur la question saisie.
   *
   * L'appel est long (le modèle interroge le catalogue en plusieurs étapes) :
   * c'est `loading` qui porte l'attente, l'interface reste utilisable.
   */
  async function run() {
    const question = query.value.trim()
    if (!question || loading.value) return

    asked.value = question
    loading.value = true
    error.value = null
    outcome.value = null

    try {
      outcome.value = await $fetch<AiOutcome>('/api/ai/search', {
        method: 'POST',
        body: { query: question },
      })
    } catch (e: any) {
      // Les messages du backend sont écrits pour l'utilisateur (« clé API
      // manquante », « appel de … impossible ») : ils remontent tels quels.
      error.value = e?.data?.message ?? e?.data?.cause ?? e?.message ?? t('ai.failed')
    } finally {
      loading.value = false
    }
  }

  return {
    providers,
    configured,
    mode,
    query,
    loading,
    error,
    outcome,
    asked,
    refresh,
    run,
    toggle,
    close,
    reset,
  }
}
