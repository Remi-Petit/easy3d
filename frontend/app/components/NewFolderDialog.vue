<script setup lang="ts">
/**
 * Fenêtre de création d'un dossier, montée **une seule fois** par le layout.
 *
 * L'état vit dans `useNewFolder` (partagé) : le menu contextuel du fond de page
 * l'ouvre avec son dossier d'accueil. Le champ est vide et prend le focus — on
 * crée un dossier neuf, il n'y a rien à pré-remplir.
 */
const { t } = useI18n()
const { target, name, open, pending, error, close, submit } = useNewFolder()

const field = ref<HTMLInputElement | null>(null)

/** Destination, annoncée sous le titre : le dossier courant ou la racine. */
const where = computed(() =>
  target.value?.parent ? t('upload.into', { folder: target.value.parent }) : t('upload.atRoot'),
)

/** Un nom vide ne crée rien : le bouton reste inerte plutôt que d'échouer. */
const canSubmit = computed(() => !!name.value.trim() && !pending.value)

watch(open, async (isOpen) => {
  if (!isOpen) return
  await nextTick()
  field.value?.focus()
  field.value?.select()
})
</script>

<template>
  <UModal v-model:open="open" :title="$t('newFolder.title')" :description="where">
    <template #body>
      <input
        ref="field"
        v-model="name"
        class="folder__input"
        type="text"
        spellcheck="false"
        autocomplete="off"
        :aria-label="$t('newFolder.field')"
        :placeholder="$t('newFolder.field')"
        @keydown.enter.prevent="submit"
      />
      <!-- Le refus du backend reste affiché ici : la fenêtre ne se ferme pas,
           on corrige le nom et on réessaie. -->
      <p v-if="error" class="folder__error">{{ error }}</p>
    </template>

    <template #footer>
      <div class="folder__actions">
        <button type="button" class="folder__cancel" :disabled="pending" @click="close">
          {{ $t('newFolder.cancel') }}
        </button>
        <button type="button" class="folder__confirm" :disabled="!canSubmit" @click="submit">
          {{ $t('newFolder.confirm') }}
        </button>
      </div>
    </template>
  </UModal>
</template>
