<script setup lang="ts">
/**
 * Barre d'onglets d'une page — le système de « Mon compte », partagé.
 *
 * Les onglets sont des **liens déguisés** : chaque vue a sa propre URL, et c'est
 * elle qui fait foi (un lien vers l'onglet l'ouvre directement, le bouton
 * « retour » refait le trajet inverse). Le composant ne connaît donc aucune
 * adresse : il annonce l'onglet choisi, la page navigue.
 *
 * C'est un vrai `tablist` : les flèches gauche/droite font le tour des onglets,
 * et le focus suit. Le panneau correspondant suit la même convention d'`id` —
 * `panneau-<id>`, désigné par `aria-labelledby="onglet-<id>"` — ce qui évite
 * d'avoir à la répéter de part et d'autre.
 */

const props = defineProps<{
  /** Onglets, dans l'ordre d'affichage ; l'`id` nomme les `id` du DOM. */
  tabs: { id: string; label: string }[]
  /** Onglet actif, désigné par son `id`. */
  active: string
  /** Nom accessible du `tablist` : le titre de la page. */
  label: string
}>()

const emit = defineEmits<{ select: [id: string] }>()

/** Boutons montés, par `id` : sert à déplacer le focus au clavier. */
const boutons = new Map<string, HTMLButtonElement>()

function retenir(id: string, el: unknown) {
  if (el instanceof HTMLButtonElement) boutons.set(id, el)
  else boutons.delete(id)
}

/** Flèches gauche/droite : on fait le tour des onglets, focus compris. */
function touches(e: KeyboardEvent, index: number) {
  if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
  e.preventDefault()
  const pas = e.key === 'ArrowRight' ? 1 : -1
  const suivant = props.tabs[(index + pas + props.tabs.length) % props.tabs.length]
  if (!suivant) return
  // Le bouton visé est déjà monté (même liste, seul l'état actif change) : le
  // focus peut suivre tout de suite.
  emit('select', suivant.id)
  boutons.get(suivant.id)?.focus()
}
</script>

<template>
  <div class="panel-tabs" role="tablist" :aria-label="label">
    <button
      v-for="(tab, index) in tabs"
      :id="`onglet-${tab.id}`"
      :key="tab.id"
      :ref="(el) => retenir(tab.id, el)"
      type="button"
      role="tab"
      class="panel-tab"
      :class="{ 'panel-tab--on': tab.id === active }"
      :aria-selected="tab.id === active"
      :tabindex="tab.id === active ? 0 : -1"
      :aria-controls="`panneau-${tab.id}`"
      @click="emit('select', tab.id)"
      @keydown="touches($event, index)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>
