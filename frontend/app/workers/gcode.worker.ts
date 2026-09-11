// Worker d'analyse G-code.
//
// Il ne fait que la plomberie : recevoir le fichier brut, appeler l'analyseur
// (logique pure dans `~/utils/gcode`) et renvoyer les tableaux. Le parsing est
// ainsi testable directement, hors contexte worker.
import { parseGcode } from '../utils/gcode'

/** Contexte du worker (évite de dépendre de la lib `webworker`). */
interface WorkerCtx {
  postMessage(message: unknown, transfer?: Transferable[]): void
  onmessage: ((ev: MessageEvent<ArrayBuffer>) => void) | null
}

const ctx = self as unknown as WorkerCtx

ctx.onmessage = (ev: MessageEvent<ArrayBuffer>) => {
  try {
    const text = new TextDecoder().decode(ev.data)
    const result = parseGcode(text)
    ctx.postMessage(result, [result.positions.buffer, result.colors.buffer])
  } catch (err: unknown) {
    ctx.postMessage({ error: err instanceof Error ? err.message : String(err) })
  }
}
