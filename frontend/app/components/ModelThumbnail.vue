<script setup lang="ts">
// Aperçu 3D paresseux : ne monte le viewer que lorsque la vignette est proche
// du viewport, pour limiter le nombre de contextes WebGL actifs.
// (getBoundingClientRect + scroll : fiable partout, contrairement à IO.)
const props = defineProps<{ rel: string }>()

const el = ref<HTMLElement | null>(null)
const visible = ref(false)

function check() {
  const rect = el.value?.getBoundingClientRect()
  if (!rect) return
  visible.value = rect.top < window.innerHeight + 200 && rect.bottom > -200
}

onMounted(() => {
  check()
  // Re-vérifie après le premier rendu (layout possiblement encore instable).
  requestAnimationFrame(check)
  window.addEventListener('scroll', check, { passive: true })
  window.addEventListener('resize', check)
})
onBeforeUnmount(() => {
  window.removeEventListener('scroll', check)
  window.removeEventListener('resize', check)
})
</script>

<template>
  <div ref="el" class="thumb">
    <ModelViewer v-if="visible" :rel="rel" />
    <div v-else class="thumb__ph">3D</div>
  </div>
</template>
