import { errorCodeOf } from '~/composables/useAccounts'

/**
 * Sessions ouvertes du compte connecté (`/account/sessions`).
 *
 * C'est la réponse à « je me suis connecté sur un poste partagé » : le serveur
 * sait quelles sessions existent (elle sont en base depuis le passage au schéma
 * v4, une par appareil), l'interface les montre et permet de fermer **la
 * sienne** — les autres appareils, eux, continuent de fonctionner.
 *
 * Ce que le serveur ne montre **jamais** : ni le jeton de session, ni son
 * empreinte. Ce qui est affiché ici (date, appareil, adresse) suffit à
 * reconnaître une session, et une empreinte vaudrait un mot de passe.
 *
 * Comme les autres composables, celui-ci ne traduit rien : il expose le **code**
 * d'erreur, que la page traduit dans son `setup`.
 */

/** Une session, telle que `/api/sessions` la renvoie. */
export interface SessionView {
  uuid: string
  created_at: number
  /** Date d'expiration : au-delà, la session ne vaut plus rien. */
  expires_at: number
  /** En-tête `User-Agent` de l'appareil (tronqué par le serveur). */
  user_agent: string
  /** Adresse du client, telle que le relais l'a annoncée (vide si inconnue). */
  ip: string
  /** `true` pour une session ouverte par le fournisseur d'identité. */
  sso: boolean
  /** `true` pour la session qui demande : c'est « cet appareil ». */
  current: boolean
}

/** Ce que renvoie `GET /sessions`. */
interface SessionsResponse {
  sessions: SessionView[]
}

export function useSessions() {
  const sessions = ref<SessionView[]>([])
  const loading = ref(false)
  const busy = ref(false)
  /** Code d'erreur de la dernière action (traduit par la page). */
  const errorCode = ref<string | null>(null)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const response = await $fetch<SessionsResponse>('/api/sessions')
      sessions.value = response.sessions
      errorCode.value = null
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
    } finally {
      loading.value = false
    }
  }

  /**
   * Ferme **une** session du compte.
   *
   * Fermer la sienne est permis (c'est une déconnexion comme une autre) : la
   * page ne le propose pas deux fois, c'est tout — le bouton « se déconnecter »
   * est ailleurs.
   */
  async function revoke(uuid: string): Promise<boolean> {
    busy.value = true
    errorCode.value = null
    try {
      await $fetch(`/api/sessions/${encodeURIComponent(uuid)}`, { method: 'DELETE' })
      await refresh()
      return true
    } catch (e: unknown) {
      errorCode.value = errorCodeOf(e)
      return false
    } finally {
      busy.value = false
    }
  }

  return { sessions, loading, busy, errorCode, refresh, revoke }
}

/**
 * Nom lisible d'un appareil, à partir de son `User-Agent`.
 *
 * Volontairement grossier : l'interface dit « Firefox », « Chrome », « Safari »,
 * sinon « Appareil inconnu ». Analyser un `User-Agent` finement est un métier à
 * part entière, et se tromper (« Edge » annoncé « Chrome ») serait pire que
 * rester vague — la date et l'adresse, elles, sont exactes.
 */
export function deviceName(userAgent: string): string {
  const value = userAgent.toLowerCase()
  if (!value.trim()) return ''
  // L'ordre compte : Edge contient « chrome », Chrome contient « safari ».
  if (value.includes('edg/')) return 'Edge'
  if (value.includes('opr/') || value.includes('opera')) return 'Opera'
  if (value.includes('firefox')) return 'Firefox'
  if (value.includes('chrome') || value.includes('chromium')) return 'Chrome'
  if (value.includes('safari')) return 'Safari'
  if (value.includes('curl') || value.includes('wget')) return 'CLI'
  return ''
}
