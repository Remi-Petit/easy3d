/** Élément visé par un renommage : un fichier ou un dossier du catalogue. */
export interface RenameTarget {
  /** Chemin relatif à la racine (`Maison/Toit`, `Maison/piece.stl`). */
  rel: string
  /** Nom affiché (dernier segment), proposé dans le champ. */
  label: string
  /** Nature de l'élément — sert à nommer ce qu'on renomme. */
  kind: 'file' | 'folder'
}

/** Durée d'affichage de la confirmation (le temps de la lire). */
const DONE_MS = 4000

/**
 * Renommage d'un élément, en **état partagé**.
 *
 * La fenêtre est montée une seule fois (dans le layout) : chaque carte se
 * contente de l'ouvrir avec sa cible. C'est le même partage que `useUpload`, et
 * ça évite une fenêtre — et un formulaire — par carte affichée.
 *
 * Rien n'est rafraîchi ici : le backend rediffuse la liste complète après
 * l'opération (notes et aperçus compris), comme après un envoi.
 */
export function useRename() {
  const { t } = useI18n()
  const toast = useToast()

  /** Cible courante : `null` = fenêtre fermée. */
  const target = useState<RenameTarget | null>('rename:target', () => null)
  /** Nom saisi. */
  const name = useState<string>('rename:name', () => '')
  /** Enregistrement en cours (le bouton se désactive). */
  const pending = useState<boolean>('rename:pending', () => false)
  /** Message d'erreur du backend, affiché dans la fenêtre. */
  const error = useState<string | null>('rename:error', () => null)

  /** Ouverture, telle que `UModal` l'attend (`v-model:open`). */
  const open = computed({
    get: () => target.value !== null,
    set: (value: boolean) => {
      if (!value) close()
    },
  })

  /** Ouvre la fenêtre sur un élément, en proposant son nom actuel. */
  function start(next: RenameTarget) {
    name.value = next.label
    error.value = null
    target.value = next
  }

  function close() {
    target.value = null
    error.value = null
  }

  /**
   * Enregistre le nouveau nom.
   *
   * Un nom inchangé (ou vide) ferme la fenêtre sans requête : valider pour ne
   * rien changer est un geste courant, il ne mérite ni aller-retour ni message.
   */
  async function submit() {
    const current = target.value
    if (!current || pending.value) return

    const wanted = name.value.trim()
    if (!wanted || wanted === current.label) {
      close()
      return
    }

    pending.value = true
    error.value = null
    try {
      await $fetch('/api/rename', {
        method: 'POST',
        body: { path: current.rel, name: wanted },
      })
      toast.add({
        title: t('rename.done', { name: wanted }),
        color: 'success',
        icon: 'i-lucide-check',
        duration: DONE_MS,
      })
      close()
    } catch (err: any) {
      // Le message du backend est destiné à l'utilisateur (« existe déjà »,
      // « nom invalide ») : on le garde dans la fenêtre, ouverte, pour qu'il
      // puisse corriger sans retaper.
      error.value = err?.data?.message || err?.message || t('rename.failed')
    } finally {
      pending.value = false
    }
  }

  return { target, name, open, pending, error, start, close, submit }
}
