/** Dossier d'accueil d'une création : chemin relatif (`''` = racine). */
export interface NewFolderTarget {
  parent: string
}

/** Durée d'affichage de la confirmation (le temps de la lire). */
const DONE_MS = 4000

/**
 * Création d'un dossier vide, en **état partagé**.
 *
 * Même partage que `useRename` / `useDelete` : la fenêtre est montée une seule
 * fois (dans le layout) et le menu contextuel du fond de page l'ouvre avec le
 * dossier d'accueil — celui de la page courante, sinon la racine.
 *
 * Rien n'est ajouté à la liste ici : le backend rediffuse le catalogue complet
 * après la création, le nouveau dossier apparaît donc de lui-même.
 */
export function useNewFolder() {
  const { t } = useI18n()
  const toast = useToast()

  /** Cible courante : `null` = fenêtre fermée. */
  const target = useState<NewFolderTarget | null>('newFolder:target', () => null)
  /** Nom saisi (vide à l'ouverture : on crée, on ne renomme pas). */
  const name = useState<string>('newFolder:name', () => '')
  /** Création en cours (les boutons se désactivent). */
  const pending = useState<boolean>('newFolder:pending', () => false)
  /** Message d'erreur du backend, affiché dans la fenêtre. */
  const error = useState<string | null>('newFolder:error', () => null)

  /** Ouverture, telle que `UModal` l'attend (`v-model:open`). */
  const open = computed({
    get: () => target.value !== null,
    set: (value: boolean) => {
      if (!value) close()
    },
  })

  function start(next: NewFolderTarget) {
    name.value = ''
    error.value = null
    target.value = next
  }

  function close() {
    target.value = null
    error.value = null
  }

  async function submit() {
    const current = target.value
    if (!current || pending.value) return

    const wanted = name.value.trim()
    if (!wanted) return

    pending.value = true
    error.value = null
    try {
      await $fetch('/api/folder', {
        method: 'POST',
        body: { parent: current.parent, name: wanted },
      })
      toast.add({
        title: t('newFolder.done', { name: wanted }),
        color: 'success',
        icon: 'i-lucide-folder-plus',
        duration: DONE_MS,
      })
      close()
    } catch (err: any) {
      // Le refus du backend est destiné à l'utilisateur (« existe déjà », « nom
      // invalide ») : on le garde dans la fenêtre, ouverte, pour qu'il puisse
      // corriger sans retaper.
      error.value = err?.data?.message || err?.message || t('newFolder.failed')
    } finally {
      pending.value = false
    }
  }

  return { target, name, open, pending, error, start, close, submit }
}
