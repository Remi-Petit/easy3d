/**
 * En-tête global : les pages **déclarent** son contenu, le layout `default`
 * s'occupe du rendu.
 *
 * Le titre, le logo et le lien de retour sont déduits de la route par le layout
 * (voir `layouts/default.vue`) : ils sont donc identiques côté serveur et
 * client. Seules les valeurs dynamiques (sous-titre, compteur, connexion)
 * passent par cet état, et elles sont remplies **après** l'hydratation.
 */

export interface PageHeaderState {
  /** Sous-titre affiché sous le titre (vide au premier rendu). */
  subtitle: string
  /** Nombre de fichiers de la pastille — utilisé sur l'accueil uniquement. */
  count: number
  /** WebSocket connecté (pastille verte). */
  live: boolean
  /** Backend injoignable (pastille rouge). */
  offline: boolean
}

const INITIAL: PageHeaderState = {
  subtitle: '',
  count: 0,
  live: false,
  offline: false,
}

/** Lit l'en-tête courant (utilisé par le layout). */
export function usePageHeaderState() {
  return useState<PageHeaderState>('page-header', () => ({ ...INITIAL }))
}

/**
 * Déclare l'en-tête de la page courante.
 *
 * À appeler dans le `setup()` d'une page ; le getter est réévalué à chaque
 * changement de ses dépendances (données live, compteurs…).
 *
 * Le remplissage est différé à `onMounted` : le HTML serveur et le premier
 * rendu client partagent ainsi l'état initial, ce qui évite tout
 * « hydration mismatch » (la page s'exécute avant la fin de l'hydratation).
 */
export function usePageHeader(read: () => PageHeaderState) {
  const state = usePageHeaderState()
  onMounted(() => {
    watchEffect(() => {
      state.value = read()
    })
  })
  return state
}
