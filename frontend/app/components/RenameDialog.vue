<script setup lang="ts">
/**
 * Fenêtre de renommage, montée **une seule fois** par le layout.
 *
 * L'état vit dans `useRename` (partagé) : les cartes l'ouvrent avec leur cible,
 * cette fenêtre n'a qu'à l'afficher. Le champ est pré-rempli du nom actuel et
 * **sélectionné** — comme dans un explorateur de fichiers, taper remplace.
 */
const { target, name, open, pending, error, close, submit } = useRename()

const field = ref<HTMLInputElement | null>(null)

watch(open, async (isOpen) => {
  if (!isOpen) return
  await nextTick()
  field.value?.focus()
  field.value?.select()
})
</script>

<template>
  <UModal v-model:open="open" :title="$t('rename.title')" :description="target?.rel ?? ''">
    <template #body>
      <input
        ref="field"
        v-model="name"
        class="rename__input"
        type="text"
        spellcheck="false"
        autocomplete="off"
        :aria-label="$t('rename.field')"
        :placeholder="$t('rename.field')"
        @keydown.enter.prevent="submit"
      />
      <!-- Le refus du backend reste affiché ici : la fenêtre ne se ferme pas,
           on corrige le nom et on réessaie. -->
      <p v-if="error" class="rename__error">{{ error }}</p>
    </template>

    <template #footer>
      <div class="rename__actions">
        <button type="button" class="rename__cancel" :disabled="pending" @click="close">
          {{ $t('rename.cancel') }}
        </button>
        <button type="button" class="rename__confirm" :disabled="pending" @click="submit">
          {{ $t('rename.confirm') }}
        </button>
      </div>
    </template>
  </UModal>
</template>
