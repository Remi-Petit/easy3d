// Types alignés sur le JSON du backend Rust (scanner.rs / api.rs).
import { DEFAULT_FORMATS, type FormatInfo } from '~/utils/formats'
import type { WsConfig } from '~/utils/collab'

export interface FileInfo {
  path: string
  /** Chemin relatif à la racine (séparateurs `/`), utilisable par /api/file. */
  rel: string
  created: number | null
  modified: number | null
  /** Image d'aperçu associée (même nom, même dossier), si présente. */
  image?: string | null
  /** Note explicative (Markdown) associée, si présente. */
  note?: string | null
}

export interface FolderInfo {
  name: string
  count: number
  /**
   * Dernière modification du dossier (mtime du répertoire, secondes unix) :
   * ajout, suppression ou renommage d'un fichier dedans — pas seulement
   * l'édition d'un fichier existant.
   */
  modified: number | null
  files: FileInfo[]
  /** Note explicative (Markdown) du dossier, si présente. */
  note?: string | null
}

export type DisplayMode = 'image' | '3d'

export interface AppConfig {
  models_root?: string | null
  display?: {
    mode?: DisplayMode
  }
}

export interface ModelsResponse {
  folders: Record<string, FolderInfo>
  files: FileInfo[]
  count: number
  /**
   * Formats reconnus par le backend (extensions, aperçu, visionneuse).
   * Absent d'un backend plus ancien : `useFormats` retombe sur les défauts.
   */
  formats?: FormatInfo[]
  /** Configuration applicative, exposée par le backend. */
  config?: AppConfig
}

/**
 * Liste des modèles en **temps réel** via WebSocket, avec repli sur le polling.
 *
 * - WS `/ws` : le backend pousse un `ModelsResponse` (même JSON que `/api/models`)
 *   à chaque changement détecté par le watcher. Donc pas de rafraîchissement
 *   manuel ni de polling tant que le WS est connecté.
 * - Repli : si le WS est indisponible (ou tombe), on repasse au polling
 *   `/api/models` + tentative de reconnexion auto.
 */
export function useModels(intervalMs = 5000) {
  const { t } = useI18n()
  const data = ref<ModelsResponse | null>(null)
  const error = ref<string | null>(null)
  const loading = ref(false)
  const lastUpdated = ref<Date | null>(null)
  /** `true` si le WebSocket est actuellement connecté. */
  const live = ref(false)
  /**
   * Formats annoncés par le backend, partagés avec `useFormats()` (les
   * composants n'ont ainsi aucune extension à connaître).
   */
  const formats = useState<FormatInfo[]>('easy3d:formats', () => DEFAULT_FORMATS)

  let timer: ReturnType<typeof setInterval> | null = null
  let ws: WebSocket | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  // URL du WS, exposée côté client via runtimeConfig.public : même origine que
  // l'interface par défaut, Nitro relayant `/ws` vers le backend.
  function wsUrl(): string {
    const base = wsBase(useRuntimeConfig().public as WsConfig)
    return base ? `${base}/ws` : ''
  }

  /** Réinitialise le timeout de reconnexion (évite les piles de setTimeout). */
  function clearReconnect() {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer)
      reconnectTimer = null
    }
  }

  /**
   * Mémorise une réponse du backend, qu'elle vienne du WS ou du polling :
   * données du catalogue + formats reconnus (identiques dans les deux cas).
   */
  function apply(response: ModelsResponse) {
    data.value = response
    if (response.formats?.length) formats.value = response.formats
    error.value = null
    lastUpdated.value = new Date()
  }

  async function refresh() {
    loading.value = true
    try {
      apply(await $fetch<ModelsResponse>('/api/models'))
    } catch (e: any) {
      error.value = e?.data?.statusMessage || e?.message || t('common.unknownError')
    } finally {
      loading.value = false
    }
  }

  function startPolling() {
    if (timer) clearInterval(timer)
    timer = setInterval(refresh, intervalMs)
  }

  function stopPolling() {
    if (timer) {
      clearInterval(timer)
      timer = null
    }
  }

  function startWs() {
    let url = ''
    try {
      url = wsUrl()
    } catch {
      startPolling()
      return
    }
    if (!url) {
      startPolling()
      return
    }

    try {
      ws = new WebSocket(url)
    } catch {
      startPolling()
      return
    }

    ws.onopen = () => {
      live.value = true
      error.value = null
      // Le WS pousse tout : on coupe le polling pendant qu'il est vivant.
      stopPolling()
    }
    ws.onmessage = (ev) => {
      try {
        apply(JSON.parse(ev.data) as ModelsResponse)
      } catch {
        // Message non-JSON : on l'ignore.
      }
    }
    ws.onerror = () => {
      live.value = false
    }
    ws.onclose = () => {
      live.value = false
      // Repli : polling + reconnexion après `intervalMs`.
      startPolling()
      clearReconnect()
      reconnectTimer = setTimeout(() => {
        if (ws) {
          ws.onclose = null
          ws.close()
          ws = null
        }
        startWs()
      }, intervalMs)
    }
  }

  onMounted(() => {
    // Premier chargement rapide via HTTP, puis connexion WS.
    refresh()
    startWs()
  })

  onBeforeUnmount(() => {
    stopPolling()
    clearReconnect()
    if (ws) {
      ws.onclose = null
      ws.close()
      ws = null
    }
  })

  return { data, error, loading, lastUpdated, live, refresh }
}
