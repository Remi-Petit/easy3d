<script setup lang="ts">
import * as THREE from 'three'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
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
const stats = ref({ triangles: 0 })

let renderer: THREE.WebGLRenderer | null = null
let scene: THREE.Scene | null = null
let camera: THREE.PerspectiveCamera | null = null
let controls: OrbitControls | null = null
let raf = 0
let model: THREE.Object3D | null = null

function buildLighting() {
  const amb = new THREE.AmbientLight(0xffffff, 0.6)
  const hemi = new THREE.HemisphereLight(0xffffff, 0x1e293b, 0.65)
  const key = new THREE.DirectionalLight(0xffffff, 1.35)
  key.position.set(3, 4, 2)
  return [amb, hemi, key]
}

function frame(mesh: THREE.Object3D) {
  if (!camera) return
  const box = new THREE.Box3().setFromObject(mesh)
  const size = box.getSize(new THREE.Vector3())
  const center = box.getCenter(new THREE.Vector3())
  // Recentre l'objet à l'origine puis le pose sur la grille.
  mesh.position.sub(center)
  mesh.position.y += size.y / 2
  const maxDim = Math.max(size.x, size.y, size.z) || 1
  const fovRad = (camera.fov * Math.PI) / 180
  const dist = (maxDim / (2 * Math.tan(fovRad / 2))) * 1.35
  camera.position.set(dist * 0.72, dist * 0.55, dist * 0.72)
  camera.near = dist / 100
  camera.far = dist * 20
  camera.updateProjectionMatrix()
  const target = new THREE.Vector3(0, size.y / 2, 0)
  camera.lookAt(target)
  if (controls) {
    controls.target.copy(target)
    controls.update()
  }
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
    const geometry = new STLLoader().parse(buffer)
    geometry.computeVertexNormals()

    const posAttr = geometry.getAttribute('position')
    const triCount = geometry.index ? geometry.index.count / 3 : (posAttr?.count ?? 0) / 3
    stats.value = { triangles: Math.floor(triCount) }

    const mat = new THREE.MeshStandardMaterial({
      color: 0x818cf8,
      metalness: 0.2,
      roughness: 0.55,
      flatShading: true,
    })
    const mesh = new THREE.Mesh(geometry, mat)
    scene.add(mesh)
    model = mesh
    frame(mesh)
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
  scene.add(new THREE.GridHelper(2, 10, 0x334155, 0x1e293b))

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
    </div>
  </div>
</template>
