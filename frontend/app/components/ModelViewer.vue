<script setup lang="ts">
import * as THREE from 'three'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
import { ThreeMFLoader } from 'three/addons/loaders/3MFLoader.js'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'

const props = withDefaults(
  defineProps<{
    /** Chemin relatif à la racine des modèles (tel que renvoyé par /models → `rel`). */
    rel: string
    autoRotate?: boolean
    showInfo?: boolean
  }>(),
  { autoRotate: true, showInfo: false },
)

const wrapRef = ref<HTMLElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const loading = ref(true)
const loadError = ref<string | null>(null)
const stats = ref<{ triangles: number; dims?: number[] }>({ triangles: 0 })

let renderer: THREE.WebGLRenderer | null = null
let scene: THREE.Scene | null = null
let camera: THREE.PerspectiveCamera | null = null
let controls: OrbitControls | null = null
let raf = 0
let model: THREE.Object3D | null = null
let plateGroup: THREE.Group | null = null
/**
 * Plateau d'impression en mm (lit standard 300×300 mm).
 * Le modèle STL est affiché à son échelle réelle (1 unité = 1 mm).
 */
const PLATE = 300

function buildLighting() {
  const amb = new THREE.AmbientLight(0xffffff, 0.6)
  const hemi = new THREE.HemisphereLight(0xffffff, 0x1e293b, 0.65)
  const key = new THREE.DirectionalLight(0xffffff, 1.35)
  key.position.set(3, 4, 2)
  return [amb, hemi, key]
}

/**
 * Met le modèle à plat sur le plateau (centre + pose à y=0), sans le déformer.
 */
function fitToPlate(mesh: THREE.Object3D) {
  mesh.updateMatrixWorld(true)
  const box = new THREE.Box3().setFromObject(mesh)
  mesh.position.x -= box.getCenter(new THREE.Vector3()).x
  mesh.position.z -= box.getCenter(new THREE.Vector3()).z
  mesh.position.y -= box.min.y
}

/** Cadre la caméra sur le plateau (et la hauteur du modèle). */
function frame(mesh: THREE.Object3D) {
  if (!camera) return
  const box = new THREE.Box3().setFromObject(mesh)
  const size = box.getSize(new THREE.Vector3())
  const viewBase = Math.max(size.y, PLATE) || PLATE
  const fovRad = (camera.fov * Math.PI) / 180
  const dist = (viewBase / (2 * Math.tan(fovRad / 2))) * 1.45
  camera.position.set(dist * 0.72, dist * 0.55, dist * 0.72)
  camera.near = dist / 100
  camera.far = dist * 20
  camera.updateProjectionMatrix()
  const target = new THREE.Vector3(0, size.y * 0.5, 0)
  camera.lookAt(target)
  if (controls) {
    controls.target.copy(target)
    controls.update()
  }
}

/**
 * Compte les triangles d'un objet 3D (sommets non indexés = /3, indexés = /3).
 * S'applique à un `Mesh` comme à un `Group` (3MF contient plusieurs mailles).
 */
function countTriangles(root: THREE.Object3D): number {
  let n = 0
  root.traverse((o) => {
    const mesh = o as THREE.Mesh
    if ((mesh as any).isMesh && mesh.geometry) {
      const g = mesh.geometry as THREE.BufferGeometry
      const pos = g.getAttribute('position')
      if (!pos) return
      n += g.index ? g.index.count / 3 : pos.count / 3
    }
  })
  return Math.floor(n)
}

/**
 * Plateau d'impression 3D : lit semi-transparent + grille + bordures.
 * Dimensionné d'après l'empreinte du modèle pour qu'il reste dessus.
 */
function buildPlate() {
  if (!scene) return
  if (plateGroup) {
    scene.remove(plateGroup)
    plateGroup.traverse((o) => {
      const m = o as THREE.Mesh
      if (m.geometry) m.geometry.dispose()
      const mats = Array.isArray(m.material) ? m.material : [m.material]
      mats.forEach((mm) => mm?.dispose?.())
    })
    plateGroup = null
  }

  const group = new THREE.Group()
  const w = PLATE
  const d = PLATE
  const thickness = PLATE * 0.02

  // Plateau (boîte semi-transparente, face supérieure à y=0).
  const plateGeo = new THREE.BoxGeometry(w, thickness, d)
  const plateMat = new THREE.MeshStandardMaterial({
    color: 0x16202f,
    metalness: 0.05,
    roughness: 0.95,
    transparent: true,
    opacity: 0.6,
  })
  const plate = new THREE.Mesh(plateGeo, plateMat)
  plate.position.y = -thickness / 2
  group.add(plate)

  // Grille alignée au plateau.
  const grid = new THREE.GridHelper(w, w / 10, 0x475569, 0x334155)
  grid.position.y = 0.001
  group.add(grid)

  // Bordures du plateau.
  const edges = new THREE.LineSegments(
    new THREE.EdgesGeometry(plateGeo),
    new THREE.LineBasicMaterial({ color: 0x64748b }),
  )
  edges.position.copy(plate.position)
  group.add(edges)

  scene.add(group)
  plateGroup = group
}

function resize() {
  if (!renderer || !camera || !wrapRef.value) return
  const w = wrapRef.value.clientWidth
  const h = wrapRef.value.clientHeight
  if (!w || !h) return
  renderer.setSize(w, h, false)
  camera.aspect = w / h
  camera.updateProjectionMatrix()
}

function animate() {
  if (!renderer || !scene || !camera || !controls) return
  raf = requestAnimationFrame(animate)
  controls.update()
  renderer.render(scene, camera)
}

async function loadModel() {
  if (!scene || !camera) return
  try {
    const res = await fetch(`/api/file?path=${encodeURIComponent(props.rel)}`)
    if (!res.ok) throw new Error(`Erreur HTTP ${res.status}`)
    const buffer = await res.arrayBuffer()

    // `.3mf` est un zip chargé via le 3MFLoader (peut contenir plusieurs
    // mailles) ; STL (et OBJ sans loader dédié) passe par le STLLoader.
    const is3mf = ext(props.rel) === '3mf'
    let object: THREE.Object3D

    if (is3mf) {
      object = new ThreeMFLoader().parse(buffer) as unknown as THREE.Group
      // Conforme au spec 3MF (Z-up, comme STL) : conversion en Y-up three.js.
      object.rotation.x = -Math.PI / 2
    } else {
      const geometry = new STLLoader().parse(buffer)
      geometry.computeVertexNormals()
      const mesh = new THREE.Mesh(
        geometry,
        new THREE.MeshStandardMaterial({
          color: 0x818cf8,
          metalness: 0.2,
          roughness: 0.55,
          flatShading: true,
        }),
      )
      mesh.rotation.x = -Math.PI / 2
      object = mesh
    }

    object.updateMatrixWorld(true)
    scene.add(object)
    model = object
    fitToPlate(object)

    const triCount = countTriangles(object)
    // Cotes réelles (mm) après conversion Z-up → Y-up.
    const bb = new THREE.Box3().setFromObject(object)
    const s = bb.getSize(new THREE.Vector3())
    const r = (v: number) => Math.round(v * 100) / 100
    stats.value = { triangles: triCount, dims: [r(s.x), r(s.y), r(s.z)] }

    frame(object)
    buildPlate()
    loading.value = false
    animate()
  } catch (e: any) {
    loadError.value = e?.message ?? String(e)
    loading.value = false
  }
}

function init() {
  const canvas = canvasRef.value
  const wrap = wrapRef.value
  if (!canvas || !wrap) return
  const w = wrap.clientWidth || 1
  const h = wrap.clientHeight || 1

  scene = new THREE.Scene()
  scene.background = new THREE.Color(0x0d1322)
  camera = new THREE.PerspectiveCamera(50, w / h, 0.01, 10000)
  renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2))
  renderer.setSize(w, h, false)

  buildLighting().forEach((l) => scene?.add(l))
  // Le plateau d'impression (avec sa grille) est ajouté au chargement du modèle.

  controls = new OrbitControls(camera, renderer.domElement)
  controls.enableDamping = true
  controls.dampingFactor = 0.08
  controls.autoRotate = props.autoRotate
  controls.autoRotateSpeed = 1.4

  loadModel()
}

function dispose() {
  cancelAnimationFrame(raf)
  if (controls) {
    controls.dispose()
    controls = null
  }
  if (plateGroup) {
    plateGroup.traverse((o) => {
      const m = o as THREE.Mesh
      if (m.geometry) m.geometry.dispose()
      const mats = Array.isArray(m.material) ? m.material : [m.material]
      mats.forEach((mm) => mm?.dispose?.())
    })
    plateGroup = null
  }
  if (model) {
    model.traverse((o) => {
      const m = o as THREE.Mesh
      if (m.geometry) m.geometry.dispose()
      const mats = Array.isArray(m.material) ? m.material : [m.material]
      mats.forEach((mm) => mm?.dispose?.())
    })
    model = null
  }
  if (renderer) {
    renderer.dispose()
    renderer = null
  }
  scene = null
  camera = null
}

onMounted(() => {
  init()
  window.addEventListener('resize', resize)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', resize)
  dispose()
})
</script>

<template>
  <div ref="wrapRef" class="model-viewer">
    <canvas ref="canvasRef" class="model-viewer__canvas" />
    <div v-if="loading" class="model-viewer__overlay">chargement…</div>
    <div v-if="loadError" class="model-viewer__overlay model-viewer__overlay--err">⚠ {{ loadError }}</div>
    <div v-if="showInfo && !loading && !loadError" class="model-viewer__info">
      {{ stats.triangles.toLocaleString('fr-FR') }} triangles
      <template v-if="stats.dims">&nbsp;· {{ stats.dims.join(' × ') }}</template>
    </div>
  </div>
</template>
