<script setup lang="ts">
/**
 * Fenêtre de **confirmation** d'une suppression, montée une seule fois par le
 * layout (voir `useDelete`).
 *
 * Une suppression ne se rattrape pas : on annonce donc précisément ce qui part —
 * le nom, et pour un dossier le nombre de fichiers qu'il emporte.
 */
const { t } = useI18n()
const { target, open, pending, error, close, submit } = useDelete()

/** Avertissement adapté à l'élément (et à ce qu'il contient). */
const warning = computed(() => {
  const item = target.value
  if (!item) return ''

  return item.kind === 'folder'
    ? t('delete.whatFolder', { name: item.label, count: item.count ?? 0 }, item.count ?? 0)
    : t('delete.whatFile', { name: item.label })
})
</script>

<template>
  <UModal v-model:open="open" :title="$t('delete.title')" :description="target?.rel ?? ''">
    <template #body>
      <p class="confirm__warning">{{ warning }}</p>
      <!-- Le refus du backend reste affiché ici : la fenêtre ne se ferme pas. -->
      <p v-if="error" class="confirm__error">{{ error }}</p>
    </template>

    <template #footer>
      <div class="confirm__actions">
        <button type="button" class="confirm__cancel" :disabled="pending" @click="close">
          {{ $t('delete.cancel') }}
        </button>
        <button
          type="button"
          class="confirm__confirm confirm__confirm--danger"
          :disabled="pending"
          @click="submit"
        >
          {{ $t('delete.confirm') }}
        </button>
      </div>
    </template>
  </UModal>
</template>
