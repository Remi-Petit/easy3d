import type { JournalEvent } from '~/utils/journal'

/**
 * Journal d'audit (`/admin/journal`).
 *
 * Lecture seule, et **volontairement** : un journal que l'interface pourrait
 * modifier ne vaudrait rien comme témoin. Le serveur le borne (les plus anciens
 * partent), donc la page n'a rien à nettoyer non plus.
 *
 * Comme les autres composables, celui-ci ne traduit rien : il expose les codes
 * du backend (`kind`, `detail`), que la page traduit dans son `setup`.
 */
export function useJournal() {
  const events = ref<JournalEvent[]>([])
  const loading = ref(false)
  const errorCode = ref<string | null>(null)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      // Pas de `limit` dans l'URL : la borne est décidée par le backend (et
      // relayée telle quelle par le proxy Nitro). Une interface qui pourrait
      // demander « tout le journal » finirait par le faire.
      events.value = await $fetch<JournalEvent[]>('/api/journal')
      errorCode.value = null
    } catch (e: unknown) {
      const status = (e as { statusCode?: number })?.statusCode
      errorCode.value = status ? `http_${status}` : 'unknown'
    } finally {
      loading.value = false
    }
  }

  return { events, loading, errorCode, refresh }
}
