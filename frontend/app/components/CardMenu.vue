<script setup lang="ts">
/**
 * Menu contextuel d'une carte (clic droit).
 *
 * Posé au curseur (`position: fixed`) dans le `body` via `Teleport` : la carte
 * porte des transformations au survol, ce qui ferait de `fixed` un repère local
 * et décalerait le menu. Le menu se rabat quand il sortirait de la fenêtre.
 *
 * Il ne fait rien lui-même : il liste des entrées et **rend** le choix à la
 * carte, qui sait quoi en faire (renommer, et demain déplacer ou supprimer).
 */
export interface CardMenuItem {
  /** Identifiant rendu à la carte (`select`). */
  key: string
  label: string
  /** Icône Lucide, ex : `i-lucide-pencil`. */
  icon?: string
}

const props = defineProps<{
  open: boolean
  /** Position du clic, en pixels de fenêtre. */
  x: number
  y: number
  items: CardMenuItem[]
  /** Nom de l'élément visé : décrit le menu pour les lecteurs d'écran. */
  label?: string
}>()

const emit = defineEmits<{ select: [key: string]; close: [] }>()

const panel = ref<HTMLElement | null>(null)
const position = ref({ left: 0, top: 0 })

/** Marge gardée avec les bords de la fenêtre. */
const MARGIN = 6

/** Pose le menu au curseur, en le rabattant s'il dépasserait. */
function place() {
  const rect = panel.value?.getBoundingClientRect()
  const width = rect?.width ?? 0
  const height = rect?.height ?? 0

  position.value = {
    left: Math.max(MARGIN, Math.min(props.x, window.innerWidth - width - MARGIN)),
    top: Math.max(MARGIN, Math.min(props.y, window.innerHeight - height - MARGIN)),
  }
}

function close() {
  emit('close')
}

/** Un clic hors du menu le referme (avant que la page ne réagisse). */
function onPointerDown(event: MouseEvent) {
  if (panel.value?.contains(event.target as Node)) return
  close()
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') close()
}

function detach() {
  document.removeEventListener('mousedown', onPointerDown, true)
  document.removeEventListener('keydown', onKeydown)
  window.removeEventListener('scroll', close, true)
  window.removeEventListener('resize', close)
}

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) {
      detach()
      return
    }

    // Le menu suit le curseur : un défilement le laisserait à côté de sa carte.
    document.addEventListener('mousedown', onPointerDown, true)
    document.addEventListener('keydown', onKeydown)
    window.addEventListener('scroll', close, true)
    window.addEventListener('resize', close)

    await nextTick()
    place()
    // Le clavier doit pouvoir prendre la suite (Entrée, flèches, Échap).
    panel.value?.querySelector('button')?.focus()
  },
)

onBeforeUnmount(detach)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      ref="panel"
      class="card-menu"
      role="menu"
      :aria-label="label"
      :style="{ left: `${position.left}px`, top: `${position.top}px` }"
    >
      <button
        v-for="item in items"
        :key="item.key"
        type="button"
        role="menuitem"
        class="card-menu__item"
        @click="emit('select', item.key)"
      >
        <UIcon v-if="item.icon" :name="item.icon" class="card-menu__icon" />
        <span>{{ item.label }}</span>
      </button>
    </div>
  </Teleport>
</template>
