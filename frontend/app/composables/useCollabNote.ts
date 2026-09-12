/**
 * Édition collaborative d'une note (CRDT Yjs).
 *
 * Le document partagé contient un unique type racine `markdown`, exactement
 * comme le backend Rust (`collab::TEXT_KEY`). Le serveur ne fait que relayer
 * les modifications : la fusion est faite par le CRDT, ce qui permet à
 * plusieurs personnes d'écrire **au même moment**, à des endroits différents,
 * sans qu'aucune saisie n'en écrase une autre.
 *
 * Le texte de la note est donc lu en direct, et non plus depuis le scan du
 * dossier : deux sessions ouvertes sur la même note restent synchronisées.
 */
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import { yCollab } from 'y-codemirror.next'
import { collabBase, collabRoom, type WsConfig } from '~/utils/collab'

/**
 * Extensions CodeMirror à injecter au prochain éditeur d'un `editorId` donné.
 *
 * md-editor-v3 n'offre pas d'option par instance pour CodeMirror : il faut
 * passer par sa configuration globale, qui reçoit l'`editorId`. On y dépose
 * donc les extensions de collaboration juste avant de monter l'éditeur.
 */
const pending = new Map<string, unknown[]>()

/**
 * Extensions de collaboration à ajouter à celles de md-editor-v3.
 *
 * À brancher une seule fois, depuis l'appel à `config()` du composant, en même
 * temps que la traduction de la barre d'outils (un seul appel, donc aucun
 * risque d'écrasement entre les deux réglages).
 */
export function collabExtensions(editorId: string, extensions: unknown[]): unknown[] {
  const extra = pending.get(editorId)
  if (!extra) {
    return extensions
  }
  // md-editor-v3 n'accepte pas des extensions CodeMirror brutes : il parcourt la
  // liste en extrayant `.extension` de chaque entrée (voir `Wa` dans MdEditor).
  // Nos extensions doivent donc être enveloppées, sinon elles arrivent
  // `undefined` dans CodeMirror — qui refuse alors de créer l'éditeur.
  return [...extensions, ...extra.map((extension) => ({ type: 'collab', extension }))]
}

/** Ce qu'un panneau de note doit savoir de son document partagé. */
export interface CollabNote {
  /** Texte courant du document, réactif (mis à jour par tous les participants). */
  text: Ref<string>
  /** Le serveur d'édition est joignable. */
  connected: Ref<boolean>
  /** L'état initial du document a été reçu. */
  synced: Ref<boolean>
  /** Nombre d'**autres** participants sur cette note. */
  peers: Ref<number>
  /**
   * Prépare l'éditeur d'`editorId`, et renvoie le contenu initial à lui donner.
   *
   * À appeler **juste avant** de monter l'éditeur : les extensions doivent être
   * enregistrées avant sa création, et le contenu initial doit correspondre à
   * l'état du document partagé à cet instant précis, sinon l'éditeur impose le
   * sien et se désynchronise du CRDT.
   */
  beginEdit(editorId: string): string
  destroy(): void
}

/**
 * Ouvre le document collaboratif d'une note.
 *
 * `rel` est le chemin relatif de l'élément (nom de dossier, ou chemin de
 * fichier). Le composant qui l'utilise doit être recréé quand `rel` change.
 */
export function useCollabNote(rel: string): CollabNote {
  const pub = useRuntimeConfig().public as WsConfig

  const ydoc = new Y.Doc()
  const ytext = ydoc.getText('markdown')

  const text = ref(ytext.toString())
  const connected = ref(false)
  const synced = ref(false)
  const peers = ref(0)

  const provider = new WebsocketProvider(collabBase(pub), collabRoom(rel), ydoc)

  /** Reflet réactif du document, pour l'affichage en lecture. */
  const mirror = () => {
    text.value = ytext.toString()
  }
  ydoc.on('update', mirror)

  const countPeers = () => {
    // `getStates()` inclut notre propre session.
    peers.value = Math.max(0, (provider.awareness?.getStates().size ?? 1) - 1)
  }
  provider.awareness.on('change', countPeers)

  provider.on('status', ({ status }: { status: string }) => {
    connected.value = status === 'connected'
  })
  provider.on('sync', (etat: boolean) => {
    synced.value = etat
    mirror()
    countPeers()
  })

  /** Identifiant de l'éditeur préparé, pour nettoyer son enregistrement. */
  let editorId: string | null = null

  function beginEdit(id: string): string {
    const undoManager = new Y.UndoManager(ytext)
    pending.set(id, [yCollab(ytext, provider.awareness, { undoManager })])
    editorId = id
    // L'éditeur doit démarrer sur l'état exact du document partagé.
    return ytext.toString()
  }

  function destroy() {
    // Prévient les autres participants qu'on s'en va (retire notre curseur).
    provider.awareness?.setLocalState(null)
    if (editorId) {
      pending.delete(editorId)
    }
    provider.destroy()
    ydoc.destroy()
  }

  onBeforeUnmount(destroy)

  return { text, connected, synced, peers, beginEdit, destroy }
}
