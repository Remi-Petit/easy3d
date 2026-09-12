<script setup lang="ts">
const props = defineProps<{
  rel: string
  name: string
}>()
const emit = defineEmits<{ close: [] }>()

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div class="modal" @click.self="emit('close')">
      <div class="modal__head">
        <span class="modal__title">{{ name }}</span>
        <button class="modal__close" @click="emit('close')" :title="$t('common.close')">✕</button>
      </div>
      <div class="modal__body">
        <ModelViewer :rel="rel" :auto-rotate="false" show-info />
      </div>
      <p class="modal__hint">{{ $t('viewer.hint') }}</p>
    </div>
  </Teleport>
</template>
