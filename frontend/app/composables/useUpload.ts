import type { UploadPick, UploadTask } from '~/utils/upload'

/** État d'un envoi : en cours, terminé, en échec, ou rien à envoyer. */
export type UploadState = 'idle' | 'running' | 'done' | 'failed' | 'empty'

/** Durée d'affichage du bilan d'un envoi terminé. */
const RESULT_MS = 6000

/** Envoi d'un fichier : le corps de la requête **est** le `File`. */
async function post(task: UploadTask): Promise<void> {
  await $fetch('/api/upload', {
    method: 'POST',
    query: { path: task.rel },
    body: task.file,
    headers: { 'content-type': task.file.type || 'application/octet-stream' },
  })
}

/**
 * Envoi avec une **seconde tentative** si le réseau a lâché.
 *
 * Un refus du backend (chemin réservé, dossier homonyme, taille excessive…) est
 * définitif : réessayer ne changerait rien, et le message doit remonter tel quel.
 * Une coupure, elle, mérite une seconde chance : le backend peut redémarrer
 * (mise à jour du conteneur) juste au moment de l'envoi.
 */
async function send(task: UploadTask): Promise<void> {
  try {
    await post(task)
  } catch (err: any) {
    const status = err?.statusCode
    if (status && status < 500) throw err
    await post(task)
  }
}

/**
 * Envoi de fichiers vers le backend, **un par un**.
 *
 * Séquentiel volontairement : chaque écriture déclenche un rescan du catalogue
 * côté backend, et dix envois parallèles ne feraient gagner du temps qu'au prix
 * d'une rafale de rescans. L'interface reste réactive, et l'avancement est
 * lisible (« 3/12 · piece.stl »).
 *
 * L'état est **partagé** (`useState`) : le dépôt de fichiers est géré par le
 * layout, l'avancement s'affiche dans la barre d'outils — les deux doivent voir
 * le même envoi.
 *
 * Le catalogue n'est pas rafraîchi ici : le watcher détecte les nouveaux
 * fichiers et rediffuse la liste sur le WebSocket.
 */
export function useUpload() {
  const state = useState<UploadState>('upload:state', () => 'idle')
  /** Nombre de fichiers de l'envoi en cours (ou du dernier bilan). */
  const total = useState('upload:total', () => 0)
  const done = useState('upload:done', () => 0)
  /** Nom du fichier en cours d'envoi. */
  const current = useState('upload:current', () => '')
  /** Échecs, un libellé lisible par fichier refusé. */
  const failures = useState<string[]>('upload:failures', () => [])

  const running = computed(() => state.value === 'running')

  let timer: ReturnType<typeof setTimeout> | undefined

  function clearTimer() {
    if (timer) {
      clearTimeout(timer)
      timer = undefined
    }
  }

  /** Efface le bilan après quelques secondes : il ne doit pas rester à l'écran. */
  function scheduleReset() {
    clearTimer()
    timer = setTimeout(() => {
      timer = undefined
      state.value = 'idle'
      total.value = 0
      done.value = 0
      failures.value = []
    }, RESULT_MS)
  }

  onScopeDispose(clearTimer)

  /**
   * Envoie `picks` dans `folder` (vide = racine du catalogue).
   *
   * Un échec n'interrompt pas les autres fichiers : c'est le bilan qui dit
   * combien sont passés et combien ont été refusés.
   */
  async function upload(picks: UploadPick[], folder = ''): Promise<void> {
    if (running.value) return
    clearTimer()

    const tasks = uploadPlan(picks, folder)
    if (!tasks.length) {
      state.value = 'empty'
      scheduleReset()
      return
    }

    state.value = 'running'
    total.value = tasks.length
    done.value = 0
    failures.value = []

    for (const task of tasks) {
      current.value = basename(task.rel)
      try {
        await send(task)
        done.value += 1
      } catch (err: any) {
        // Le message du backend est déjà destiné à l'utilisateur
        // (« chemin réservé : … ») : on le reprend tel quel.
        const message: string =
          err?.data?.message || err?.data?.statusMessage || err?.message || String(err)
        failures.value.push(`${task.file.name} : ${message}`)
      }
    }

    current.value = ''
    state.value = failures.value.length ? 'failed' : 'done'
    scheduleReset()
  }

  return { state, total, done, current, failures, running, upload }
}
