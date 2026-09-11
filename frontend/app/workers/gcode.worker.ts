// Analyse d'un G-code **hors du thread principal**.
//
// Reçoit le fichier brut (`ArrayBuffer`) et renvoie les segments d'extrusion
// sous forme de `Float32Array` (positions + couleurs par couche), prêts à
// alimenter un `THREE.LineSegments`. Objectif : ne jamais figer l'interface,
// même sur un G-code de plusieurs centaines de Mo.

/** Nombre maximal de segments conservés (au-delà, on sous-échantillonne). */
const MAX_SEGMENTS = 400_000

export interface ParsedGcode {
  /** Sommets par paires (2 par segment), 3 floats chacun. */
  positions: Float32Array
  /** Couleur RGB de chaque sommet (même longueur que `positions`). */
  colors: Float32Array
  /** Nombre de segments d'extrusion retenus. */
  segments: number
  /** Nombre approximatif de couches (changements de Z). */
  layers: number
}

/** Contexte du worker (évite de dépendre de la lib `webworker`). */
interface WorkerCtx {
  postMessage(message: unknown, transfer?: Transferable[]): void
  onmessage: ((ev: MessageEvent<ArrayBuffer>) => void) | null
}

// ── Analyseur de nombres sans allocation (appelé des millions de fois) ──
// `scanFloat` positionne `value` et `index` ; les relire juste après l'appel.
let value = 0
let index = 0

/** Lit un nombre décimal à partir de `from`. `false` si aucun chiffre trouvé. */
function scanFloat(text: string, from: number, end: number): boolean {
  let i = from
  let sign = 1
  if (i < end) {
    const c = text.charCodeAt(i)
    if (c === 45 /* - */) {
      sign = -1
      i++
    } else if (c === 43 /* + */) {
      i++
    }
  }

  let n = 0
  let seen = false
  while (i < end) {
    const c = text.charCodeAt(i)
    if (c < 48 || c > 57) break
    n = n * 10 + (c - 48)
    i++
    seen = true
  }

  if (i < end && text.charCodeAt(i) === 46 /* . */) {
    i++
    let scale = 0.1
    while (i < end) {
      const c = text.charCodeAt(i)
      if (c < 48 || c > 57) break
      n += (c - 48) * scale
      scale *= 0.1
      i++
      seen = true
    }
  }

  if (!seen) return false
  value = sign * n
  index = i
  return true
}

/** Dégradé de couleur bleu → rouge selon la hauteur relative (0 → 1). */
const RAMP: [number, number, number][] = [
  [0.2, 0.45, 0.95],
  [0.25, 0.85, 0.85],
  [0.98, 0.8, 0.3],
  [0.98, 0.35, 0.3],
]

function rampColor(t: number, out: [number, number, number]) {
  const x = Math.min(Math.max(t, 0), 1) * (RAMP.length - 1)
  const i = Math.min(Math.floor(x), RAMP.length - 2)
  const f = x - i
  const a = RAMP[i]!
  const b = RAMP[i + 1]!
  out[0] = a[0] + (b[0] - a[0]) * f
  out[1] = a[1] + (b[1] - a[1]) * f
  out[2] = a[2] + (b[2] - a[2]) * f
}

/** Analyse le texte d'un G-code et en extrait les segments d'extrusion. */
export function parseGcode(text: string): ParsedGcode {
  // Positions en cours d'accumulation (2 sommets par segment).
  let buf = new Float32Array(3 * 200_000)
  let n = 0
  const push = (x: number, y: number, z: number) => {
    if (n + 3 > buf.length) {
      const grown = new Float32Array(buf.length * 2)
      grown.set(buf)
      buf = grown
    }
    buf[n++] = x
    buf[n++] = y
    buf[n++] = z
  }

  // Modes : G90/G91 (déplacements) et M82/M83 (extrusion).
  let absolute = true
  let absoluteE = true
  let x = 0
  let y = 0
  let z = 0
  let e = 0
  let hasPrev = false

  let minZ = Number.POSITIVE_INFINITY
  let maxZ = Number.NEGATIVE_INFINITY
  let layers = 0
  let lastLayerZ = Number.NaN

  const len = text.length
  let lineStart = 0

  while (lineStart < len) {
    let lineEnd = text.indexOf('\n', lineStart)
    if (lineEnd === -1) lineEnd = len

    // Saute les espaces/tabulations et les lignes vides ou commentées.
    let i = lineStart
    while (i < lineEnd) {
      const c = text.charCodeAt(i)
      if (c !== 32 && c !== 9 && c !== 13) break
      i++
    }

    const first = i < lineEnd ? text.charCodeAt(i) : 0
    if (first === 0 || first === 59 /* ; */) {
      lineStart = lineEnd + 1
      continue
    }

    // Commande : lettre G/M suivie d'un numéro (G1, G01, M83…).
    const upper = first >= 97 ? first - 32 : first
    const isG = upper === 71 /* G */
    const isM = upper === 77 /* M */
    if (!isG && !isM) {
      lineStart = lineEnd + 1
      continue
    }

    let j = i + 1
    let cmd = 0
    while (j < lineEnd) {
      const c = text.charCodeAt(j)
      if (c < 48 || c > 57) break
      cmd = cmd * 10 + (c - 48)
      j++
    }

    if (isM) {
      if (cmd === 82) absoluteE = true
      else if (cmd === 83) absoluteE = false
      lineStart = lineEnd + 1
      continue
    }
    if (cmd === 90) {
      absolute = true
      lineStart = lineEnd + 1
      continue
    }
    if (cmd === 91) {
      absolute = false
      lineStart = lineEnd + 1
      continue
    }
    // Seuls G0/G1 (déplacement) et G92 (réinitialisation) portent des axes.
    if (cmd > 1 && cmd !== 92) {
      lineStart = lineEnd + 1
      continue
    }

    // Paramètres : X, Y, Z, E (les autres — F, S… — sont lus puis ignorés).
    let px = 0
    let py = 0
    let pz = 0
    let pe = 0
    let sawX = false
    let sawY = false
    let sawZ = false
    let sawE = false

    let k = j
    while (k < lineEnd) {
      const c = text.charCodeAt(k)
      const letter = c >= 97 ? c - 32 : c
      if (letter < 65 || letter > 90) {
        k++
        continue
      }
      const ok = scanFloat(text, k + 1, lineEnd)
      k = ok ? index : k + 1
      if (!ok) continue
      const v = value
      switch (letter) {
        case 88: // X
          px = v
          sawX = true
          break
        case 89: // Y
          py = v
          sawY = true
          break
        case 90: // Z
          pz = v
          sawZ = true
          break
        case 69: // E
          pe = v
          sawE = true
          break
      }
    }

    // G92 : redéfinit la position courante (aucun déplacement de matière).
    if (cmd === 92) {
      if (sawX) x = px
      if (sawY) y = py
      if (sawZ) z = pz
      if (sawE) { e = pe; }
      hasPrev = true
      lineStart = lineEnd + 1
      continue
    }

    const ox = x
    const oy = y
    const oz = z

    if (absolute) {
      if (sawX) x = px
      if (sawY) y = py
      if (sawZ) z = pz
    } else {
      if (sawX) x += px
      if (sawY) y += py
      if (sawZ) z += pz
    }

    // Extrusion = consommation de filament positive.
    let extruding = false
    if (sawE) {
      const delta = absoluteE ? pe - e : pe
      extruding = delta > 1e-6
      e = absoluteE ? pe : e + pe
    }

    if (extruding && hasPrev) {
      push(ox, oy, oz)
      push(x, y, z)
      if (oz < minZ) minZ = oz
      if (z < minZ) minZ = z
      if (oz > maxZ) maxZ = oz
      if (z > maxZ) maxZ = z
      const layerZ = z
      if (Number.isNaN(lastLayerZ) || Math.abs(layerZ - lastLayerZ) > 1e-3) {
        layers++
        lastLayerZ = layerZ
      }
    }
    hasPrev = true

    lineStart = lineEnd + 1
  }

  // Sous-échantillonnage régulier si le fichier contient trop de segments.
  const total = n / 6
  const step = total > MAX_SEGMENTS ? Math.ceil(total / MAX_SEGMENTS) : 1
  const segments = Math.ceil(total / step)
  const positions = new Float32Array(segments * 6)
  if (step === 1) {
    positions.set(buf.subarray(0, n))
  } else {
    let o = 0
    for (let s = 0; s < total; s += step) {
      const src = s * 6
      for (let f = 0; f < 6; f++) positions[o + f] = buf[src + f]!
      o += 6
    }
  }

  // Couleurs : dégradé selon la hauteur (Z), comme une prévisualisation de slicer.
  const colors = new Float32Array(segments * 6)
  const span = maxZ > minZ ? maxZ - minZ : 1
  const rgb: [number, number, number] = [0, 0, 0]
  for (let v = 0; v < segments * 2; v++) {
    const zz = positions[v * 3 + 2]!
    rampColor((zz - minZ) / span, rgb)
    colors[v * 3] = rgb[0]
    colors[v * 3 + 1] = rgb[1]
    colors[v * 3 + 2] = rgb[2]
  }

  return { positions, colors, segments, layers }
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
