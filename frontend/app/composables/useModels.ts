// Types alignés sur le JSON du backend Rust (scanner.rs / api.rs).
export interface FileInfo {
  path: string
  /** Chemin relatif à la racine (séparateurs `/`), utilisable par /api/file. */
  rel: string
  created: number | null
  modified: number | null
}

export interface FolderInfo {
  name: string
  count: number
  files: FileInfo[]
}

export interface ModelsResponse {
  folders: Record<string, FolderInfo>
  files: FileInfo[]
  count: number
}

/** Rechargement périodique de la liste des modèles (temps réel "poli"). */
export function useModels(intervalMs = 5000) {
  const data = ref<ModelsResponse | null>(null)
  const error = ref<string | null>(null)
  const loading = ref(false)
  const lastUpdated = ref<Date | null>(null)

  let timer: ReturnType<typeof setInterval> | null = null

  async function refresh() {
    loading.value = true
    try {
      data.value = await $fetch<ModelsResponse>('/api/models')
      error.value = null
      lastUpdated.value = new Date()
    } catch (e: any) {
      error.value = e?.data?.statusMessage || e?.message || 'Erreur inconnue'
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    refresh()
    timer = setInterval(refresh, intervalMs)
  })

  onBeforeUnmount(() => {
    if (timer) clearInterval(timer)
  })

  return { data, error, loading, lastUpdated, refresh }
}
