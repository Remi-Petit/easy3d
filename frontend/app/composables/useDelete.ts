/** Élément visé par une suppression : un fichier ou un dossier du catalogue. */
export interface DeleteTarget {
  /** Chemin relatif à la racine (`Maison/Toit`, `Maison/piece.stl`). */
  rel: string
  /** Nom affiché (dernier segment). */
  label: string
  /** Nature de l'élément — décide de la phrase d'avertissement. */
  kind: 'file' | 'folder'
  /** Nombre de fichiers emportés (dossier) : annoncé avant de confirmer. */
  count?: number
}

/** Durée d'affichage de la confirmation (le temps de la lire). */
const DONE_MS = 4000

/**
 * Suppression d'un élément, en **état partagé**.
 *
 * Comme le renommage : la fenêtre est montée une seule fois (dans le layout) et
 * chaque carte l'ouvre avec sa cible. Rien n'est rafraîchi ici, le backend
 * rediffuse la liste complète après l'opération.
 */
export function useDelete() {
  const { t } = useI18n()
  const toast = useToast()
  const route = useRoute()

  /** Cible courante : `null` = fenêtre fermée. */
  const target = useState<DeleteTarget | null>('delete:target', () => null)
  /** Suppression en cours (les boutons se désactivent). */
  const pending = useState<boolean>('delete:pending', () => false)
  /** Message d'erreur du backend, affiché dans la fenêtre. */
  const error = useState<string | null>('delete:error', () => null)

  /** Ouverture, telle que `UModal` l'attend (`v-model:open`). */
  const open = computed({
    get: () => target.value !== null,
    set: (value: boolean) => {
      if (!value) close()
    },
  })

  function start(next: DeleteTarget) {
    error.value = null
    target.value = next
  }

  function close() {
    target.value = null
    error.value = null
  }

  /**
   * Quitte la page courante si elle désignait ce qui vient d'être supprimé.
   *
   * Supprimer le dossier que l'on regarde (ou un de ses ancêtres) laisserait la
   * page sur « dossier introuvable » : on remonte chez le parent, là où il reste
   * des choses à voir. Idem pour la page d'un fichier supprimé.
   */
  async function leaveGonePage(rel: string) {
    const path = decodeURIComponent(route.path)
    const under = (prefix: string) => {
      const current = path.slice(prefix.length)
      return current === rel || current.startsWith(`${rel}/`)
    }
    const isFile = path.startsWith('/fichier/')
    const isFolder = path.startsWith('/dossiers/')
    if (!(isFile && under('/fichier/')) && !(isFolder && under('/dossiers/'))) return

    const parent = parentOf(rel)
    await navigateTo(parent ? folderHref(parent) : '/')
  }

  async function submit() {
    const current = target.value
    if (!current || pending.value) return

    const { rel, label } = current
    pending.value = true
    error.value = null
    try {
      await $fetch('/api/delete', { method: 'POST', body: { path: rel } })
      toast.add({
        title: t('delete.done', { name: label }),
        color: 'success',
        icon: 'i-lucide-trash-2',
        duration: DONE_MS,
      })
      close()
      // Après la fermeture : la page peut avoir disparu avec l'élément.
      await leaveGonePage(rel)
    } catch (err: any) {
      // Le refus reste affiché dans la fenêtre, ouverte : on peut réessayer.
      error.value = err?.data?.message || err?.message || t('delete.failed')
    } finally {
      pending.value = false
    }
  }

  return { target, open, pending, error, start, close, submit }
}
