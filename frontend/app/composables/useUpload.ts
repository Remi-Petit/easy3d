import type { UploadPick, UploadTask } from '~/utils/upload'

/** Durée d'affichage du bilan (le temps de lire trois lignes). */
const RESULT_MS = 8000

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
  const { t } = useI18n()
  const toast = useToast()

  /** Un envoi est en cours (l'état est partagé, cf. ci-dessus). */
  const running = useState('upload:running', () => false)
  /** Nombre de fichiers de l'envoi en cours. */
  const total = useState('upload:total', () => 0)
  const done = useState('upload:done', () => 0)
  /** Nom du fichier en cours d'envoi. */
  const current = useState('upload:current', () => '')

  /**
   * Envoie `picks` dans `folder` (vide = racine du catalogue).
   *
   * Un échec n'interrompt pas les autres fichiers : c'est le bilan qui dit
   * combien sont passés et combien ont été refusés.
   */
  async function upload(picks: UploadPick[], folder = ''): Promise<void> {
    if (running.value) return

    const tasks = uploadPlan(picks, folder)
    if (!tasks.length) {
      toast.add({
        title: t('upload.empty'),
        color: 'warning',
        icon: 'i-lucide-circle-alert',
        duration: RESULT_MS,
      })
      return
    }

    running.value = true
    total.value = tasks.length
    done.value = 0
    /** Échecs de cet envoi : un libellé lisible par fichier refusé. */
    const failures: string[] = []

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
        failures.push(`${task.file.name} : ${message}`)
      }
    }

    current.value = ''
    const added = done.value
    running.value = false

    // Le bilan part en notification ; l'avancement, lui, reste dans la barre
    // d'outils (une notification qui se met à jour à chaque fichier serait
    // illisible).
    if (failures.length) {
      toast.add({
        title: t('upload.failed', { count: failures.length }, failures.length),
        // Le détail plutôt que le seul compteur : « 2 échecs » sans les noms
        // oblige à deviner lequel des fichiers pose problème et pourquoi.
        description: [...failures.slice(0, 3), failures.length > 3 ? '…' : '']
          .filter(Boolean)
          .join(' • '),
        color: 'error',
        icon: 'i-lucide-triangle-alert',
        duration: RESULT_MS * 2,
      })
      return
    }

    toast.add({
      title: t('upload.done', { count: added }, added),
      color: 'success',
      icon: 'i-lucide-check',
      duration: RESULT_MS,
    })
  }

  return { running, total, done, current, upload }
}
