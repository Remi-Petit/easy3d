<script setup lang="ts">
/**
 * Choix de la langue : un **popup** avec sa recherche, à la place du `<select>`.
 *
 * Un `<select>` natif ne se cherche pas : même avec une poignée de langues il
 * faut le dérouler et lire chaque entrée. Ici on ouvre une fenêtre, on tape deux
 * lettres, on valide. Le champ prend le focus à l'ouverture et les flèches
 * déplacent la mise en avant, donc le clavier suffit de bout en bout — `Entrée`
 * choisit ce qui est en avant.
 *
 * Le popup vit avec son bouton dans le pied de la sidebar, mais `UModal` le rend
 * ailleurs dans le document : rien ne déborde de la colonne.
 */
const { locale, locales, setLocale } = useI18n()

/** Fenêtre ouverte (`UModal` l'attend en `v-model:open`). */
const open = ref(false)
/** Ce qui est tapé dans la recherche. */
const query = ref('')
/** Entrée mise en avant par les flèches (index dans `matches`). */
const highlight = ref(0)
/** Le champ, pour lui donner le focus à chaque ouverture. */
const field = ref<HTMLInputElement | null>(null)

/**
 * Langues proposées : nom affiché + code (`locales` peut être mixte).
 *
 * Triées par nom **dans la langue courante** : l'ordre du fichier de
 * configuration ne dit rien à personne dès qu'il y a une vingtaine de langues,
 * et `localeCompare` place les accents là où on les attend.
 */
const items = computed(() =>
  locales.value
    .map((l) =>
      typeof l === 'string' ? { code: l, name: l } : { code: l.code, name: l.name ?? l.code },
    )
    .sort((a, b) => a.name.localeCompare(b.name, locale.value)),
)

/** Comparaison « à la frappe » : sans casse ni accents (« francais » → Français). */
function fold(value: string): string {
  return value
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .toLowerCase()
}

/**
 * Ce que la recherche laisse passer.
 *
 * Le **code** compte autant que le nom : « de » trouve `Deutsch`, ce qui est le
 * réflexe de qui connaît les codes de langue, et le nom suffit aux autres.
 */
const matches = computed(() => {
  const needle = fold(query.value.trim())
  if (!needle) return items.value
  return items.value.filter(
    (item) => fold(item.name).includes(needle) || fold(item.code).includes(needle),
  )
})

/** La langue courante, telle qu'elle s'affiche sur le bouton. */
const current = computed(
  () => items.value.find((item) => item.code === locale.value)?.name ?? locale.value,
)

// À chaque ouverture, on repart d'une ardoise propre : recherche vide, première
// entrée en avant, et le champ prêt à recevoir la frappe.
watch(open, async (isOpen) => {
  if (!isOpen) return
  query.value = ''
  highlight.value = 0
  await nextTick()
  field.value?.focus()
})

/** La liste change sous les flèches : la mise en avant repart du haut. */
watch(matches, () => {
  highlight.value = 0
})

/** Déplace la mise en avant, en boucle (le bas revient au haut). */
function move(step: number) {
  const count = matches.value.length
  if (!count) return
  highlight.value = (highlight.value + step + count) % count
}

/** `Entrée` choisit l'entrée en avant — la première tant qu'on n'a rien touché. */
function chooseHighlighted() {
  const item = matches.value[highlight.value]
  if (item) choose(item.code)
}

/**
 * Changer de langue ferme la fenêtre.
 *
 * Le changement passe par `setLocale` : lui seul écrit le cookie que le SSR
 * relit. Notre état local ne fait que le suivre — `locales` vient du i18n, donc
 * une langue ajoutée au dossier `i18n/` apparaît ici sans autre geste.
 */
function choose(code: string) {
  setLocale(code as Parameters<typeof setLocale>[0])
  open.value = false
}
</script>

<template>
  <div class="lang">
    <span class="lang__label">{{ $t('language.label') }}</span>

    <button
      type="button"
      class="lang__btn"
      :aria-label="$t('language.choose')"
      :title="$t('language.choose')"
      @click="open = true"
    >
      <span class="lang__current">{{ current }}</span>
      <UIcon name="i-lucide-chevrons-up-down" class="lang__caret" aria-hidden="true" />
    </button>

    <UModal v-model:open="open" :title="$t('language.label')" :description="$t('language.choose')">
      <template #body>
        <input
          ref="field"
          v-model="query"
          class="lang__search"
          type="text"
          spellcheck="false"
          autocomplete="off"
          :aria-label="$t('language.search')"
          :placeholder="$t('language.search')"
          @keydown.down.prevent="move(1)"
          @keydown.up.prevent="move(-1)"
          @keydown.enter.prevent="chooseHighlighted"
        />

        <ul class="lang__list">
          <li v-for="(item, index) in matches" :key="item.code">
            <button
              type="button"
              class="lang__item"
              :class="{
                'lang__item--on': item.code === locale,
                'lang__item--over': index === highlight,
              }"
              :aria-current="item.code === locale ? 'true' : undefined"
              @mouseenter="highlight = index"
              @click="choose(item.code)"
            >
              <span>{{ item.name }}</span>
              <UIcon v-if="item.code === locale" name="i-lucide-check" class="lang__check" />
            </button>
          </li>
        </ul>

        <!-- Recherche sans réponse : on le dit, plutôt que de laisser un blanc
             qu'on prendrait pour une liste qui n'a pas chargé. -->
        <p v-if="!matches.length" class="lang__empty">{{ $t('language.none') }}</p>
      </template>
    </UModal>
  </div>
</template>
