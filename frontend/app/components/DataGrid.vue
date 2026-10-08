<script setup lang="ts" generic="T extends object">
import type { GridColumn, GridSort } from '~/utils/grid'
import { GRID_SORT_LABEL_KEYS, sortRows, visibleRange } from '~/utils/grid'
import { nextSortMode, SORT_ICONS } from '~/utils/filter'

/**
 * Grille dense — la facture d'un `st.dataframe` de Streamlit.
 *
 * Streamlit s'appuie sur Glide Data Grid : un canvas, écrit en **React**. Ici,
 * pas de second framework dans l'application : une vraie `<table>`, mais
 * **virtualisée** (seules les lignes visibles sont rendues), avec en-tête
 * collant, tri au clic, colonnes redimensionnables et navigation au clavier.
 * C'est ce que la grille de Streamlit donne à voir, et ça reprend les jetons de
 * couleur de l'application plutôt qu'un thème à part.
 *
 * Le composant ne connaît **aucune** donnée métier : la page lui donne ses
 * colonnes (`utils/grid.ts`), ses lignes déjà filtrées, et remplit les cellules
 * par des gabarits nommés `#cell-<id>` (voir l'onglet Utilisateurs de
 * `/admin/accounts`). Sans gabarit, la cellule montre la propriété `id` de la
 * ligne.
 */

const props = withDefaults(
  defineProps<{
    /** Colonnes, dans l'ordre d'affichage (`utils/grid.ts`). */
    columns: GridColumn<T>[]
    /** Lignes à afficher, **déjà filtrées** par la page. */
    rows: T[]
    /** Propriété qui identifie une ligne (clé de rendu, et sélection). */
    rowKey?: string
    /** Identifiant de la ligne sélectionnée (`v-model:selected`). */
    selected?: string | null
    /**
     * Hauteur de la zone : `auto` (défaut) laisse la grille grandir avec son
     * contenu — une liste de deux lignes ne laisse pas 500 px de vide — et
     * c'est `maxHeight` qui arrête la croissance.
     */
    height?: string
    /** Au-delà, la grille **défile** et l'en-tête se colle en haut. */
    maxHeight?: string
    /** Hauteur d'une ligne, en pixels. */
    rowHeight?: number
    loading?: boolean
    /** Message affiché quand il n'y a aucune ligne (déjà traduit). */
    empty?: string
    /** Nom accessible de la grille (déjà traduit). */
    label?: string
  }>(),
  {
    rowKey: 'uuid',
    selected: null,
    height: 'auto',
    maxHeight: 'min(60vh, 520px)',
    rowHeight: 34,
    loading: false,
    empty: '',
    label: '',
  },
)

const emit = defineEmits<{
  'update:selected': [key: string | null]
  /** Double-clic, ou Entrée sur la ligne : à la page d'ouvrir ce qu'elle veut. */
  open: [row: T]
}>()

const { t } = useI18n()

// ── Tri ──────────────────────────────────────────────────────────────────
/** Tri en cours, ou `null` : l'ordre reçu est alors celui du backend. */
const tri = ref<GridSort | null>(null)

const lignes = computed(() => sortRows(props.rows, props.columns, tri.value))

/** Sens du tri, en mots (sert à l'infobulle de l'en-tête). */
const sensTri = computed(() => (tri.value ? t(GRID_SORT_LABEL_KEYS[tri.value.mode]) : ''))

/**
 * Clic sur un en-tête : décroissant, puis croissant, puis retour à l'ordre reçu
 * — le cycle de `filter.ts`, celui du catalogue et du journal.
 */
function triePar(column: GridColumn<T>) {
  if (!column.sortable) return
  const mode = tri.value?.id === column.id ? nextSortMode(tri.value.mode) : 'date-desc'
  tri.value = mode === 'none' ? null : { id: column.id, mode }
  // L'ordre change : le curseur désignerait une autre ligne. On le lâche plutôt
  // que de le laisser mentir (la sélection suit le curseur, voir `touches`).
  curseur.value = null
}

function ariaSort(column: GridColumn<T>): 'ascending' | 'descending' | 'none' {
  if (tri.value?.id !== column.id) return 'none'
  return tri.value.mode === 'date-asc' ? 'ascending' : 'descending'
}

function titreTri(column: GridColumn<T>): string {
  return sensTri.value
    ? t('grid.sortBy', { column: column.header, mode: sensTri.value })
    : t('grid.sortDefault', { column: column.header })
}

// ── Largeurs des colonnes ────────────────────────────────────────────────
/** Largeurs courantes, par colonne : elles partent des props, puis vivent ici. */
const largeurs = reactive<Record<string, number>>({})

watch(
  () => props.columns,
  (colonnes) => {
    for (const column of colonnes) {
      if (largeurs[column.id] == null) largeurs[column.id] = column.width
    }
  },
  { immediate: true },
)

function largeurDe(column: GridColumn<T>): number {
  return largeurs[column.id] ?? column.width
}

/** Somme des colonnes : c'est elle qui donne le défilement horizontal. */
const largeurTotale = computed(() =>
  props.columns.reduce((total, column) => total + largeurDe(column), 0),
)

/**
 * Redimensionnement à la souris.
 *
 * Les événements sont posés sur `window`, pas sur la poignée : sans ça, sortir
 * de la poignée pendant le glissement (ce qu'on fait forcément) arrêterait le
 * suivi.
 */
function redimensionne(evenement: PointerEvent, column: GridColumn<T>) {
  evenement.preventDefault()
  const depart = evenement.clientX
  const initiale = largeurDe(column)

  const bouge = (e: PointerEvent) => {
    largeurs[column.id] = Math.max(60, Math.round(initiale + e.clientX - depart))
  }
  const fini = () => {
    window.removeEventListener('pointermove', bouge)
    window.removeEventListener('pointerup', fini)
  }

  window.addEventListener('pointermove', bouge)
  window.addEventListener('pointerup', fini)
}

// ── Virtualisation ───────────────────────────────────────────────────────
const vue = ref<HTMLElement | null>(null)
const defilement = ref(0)
/** Avant la première mesure, on suppose une dizaine de lignes : le premier rendu
 *  doit montrer du contenu, pas une coquille vide. */
const hauteurVue = ref(props.rowHeight * 10)

const fenetre = computed(() =>
  visibleRange(lignes.value.length, props.rowHeight, defilement.value, hauteurVue.value),
)

const lignesRendues = computed(() =>
  lignes.value
    .slice(fenetre.value.start, fenetre.value.end)
    .map((row, i) => ({ row, index: fenetre.value.start + i })),
)

let observateur: ResizeObserver | null = null

onMounted(() => {
  const element = vue.value
  if (!element) return
  observateur = new ResizeObserver(() => {
    hauteurVue.value = element.clientHeight
  })
  observateur.observe(element)
})

onBeforeUnmount(() => observateur?.disconnect())

function defile(evenement: Event) {
  defilement.value = (evenement.target as HTMLElement).scrollTop
}

// ── Sélection et curseur ─────────────────────────────────────────────────
/** Cellule au clavier : { ligne, colonne }. La ligne suit, c'est la sélection. */
const curseur = ref<{ ligne: number; colonne: number } | null>(null)

/** Lignes entières visibles : le pas des touches Page↑ / Page↓. */
const lignesParPage = computed(() =>
  Math.max(1, Math.floor(hauteurVue.value / Math.max(1, props.rowHeight))),
)

function cleDe(row: T): string {
  const valeur = (row as Record<string, unknown>)[props.rowKey]
  return valeur == null ? '' : String(valeur)
}

function estSelectionnee(row: T): boolean {
  return props.selected != null && props.selected === cleDe(row)
}

/** Texte d'une cellule sans gabarit dédié : la propriété `id` de la ligne. */
function texte(row: T, column: GridColumn<T>): string {
  const valeur = (row as Record<string, unknown>)[column.id]
  return valeur == null ? '' : String(valeur)
}

function selectionne(row: T, index: number, colonne: number) {
  curseur.value = { ligne: index, colonne }
  emit('update:selected', cleDe(row))
}

/**
 * Navigation au clavier : flèches, origine/fin, Page↑/Page↓, Entrée.
 *
 * Le curseur **est** la sélection : une seule notion à l'écran, et les flèches
 * disent ce qu'elles sélectionnent. Le défilement suit (`gardeVisible`).
 */
function touches(evenement: KeyboardEvent) {
  const total = lignes.value.length
  if (!total || !props.columns.length) return

  const depart = curseur.value ?? { ligne: 0, colonne: 0 }
  const cible = { ...depart }
  const derniere = props.columns.length - 1

  switch (evenement.key) {
    case 'ArrowDown': cible.ligne = Math.min(total - 1, depart.ligne + 1); break
    case 'ArrowUp': cible.ligne = Math.max(0, depart.ligne - 1); break
    case 'ArrowRight': cible.colonne = Math.min(derniere, depart.colonne + 1); break
    case 'ArrowLeft': cible.colonne = Math.max(0, depart.colonne - 1); break
    case 'Home': cible.ligne = 0; break
    case 'End': cible.ligne = total - 1; break
    case 'PageDown': cible.ligne = Math.min(total - 1, depart.ligne + lignesParPage.value); break
    case 'PageUp': cible.ligne = Math.max(0, depart.ligne - lignesParPage.value); break
    case 'Enter': {
      const row = lignes.value[depart.ligne]
      if (row) emit('open', row)
      return
    }
    default:
      return
  }

  evenement.preventDefault()
  curseur.value = cible
  const row = lignes.value[cible.ligne]
  if (row) emit('update:selected', cleDe(row))
  gardeVisible(cible.ligne)
}

/** Recale le défilement pour que la ligne du curseur soit dans la vue. */
function gardeVisible(ligne: number) {
  const element = vue.value
  if (!element) return
  const haut = ligne * props.rowHeight
  const bas = haut + props.rowHeight
  if (haut < element.scrollTop) element.scrollTop = haut
  else if (bas > element.scrollTop + element.clientHeight) {
    element.scrollTop = bas - element.clientHeight
  }
}
</script>

<template>
  <div
    ref="vue"
    class="data-grid"
    :style="{
      height,
      maxHeight,
      '--grid-row-h': `${rowHeight}px`,
      '--grid-width': `${largeurTotale}px`,
    }"
    tabindex="0"
    @scroll.passive="defile"
    @keydown="touches"
  >
    <table
      class="data-grid__table"
      role="grid"
      :aria-label="label"
      :aria-rowcount="lignes.length + 1"
    >
      <!-- Les largeurs vivent dans le `colgroup` : c'est ce qui permet de les
           changer sans toucher au DOM des cellules. -->
      <colgroup>
        <col
          v-for="column in columns"
          :key="column.id"
          :style="{ width: `${largeurDe(column)}px` }"
        />
      </colgroup>

      <thead class="data-grid__head">
        <tr role="row">
          <th
            v-for="(column, index) in columns"
            :key="column.id"
            scope="col"
            role="columnheader"
            class="data-grid__th"
            :class="[`data-grid__th--${column.align ?? 'left'}`, { 'data-grid__th--tri': column.sortable }]"
            :aria-colindex="index + 1"
            :aria-sort="column.sortable ? ariaSort(column) : undefined"
            :tabindex="column.sortable ? 0 : undefined"
            :title="column.sortable ? titreTri(column) : undefined"
            @click="triePar(column)"
            @keydown.enter.prevent.stop="triePar(column)"
            @keydown.space.prevent.stop="triePar(column)"
          >
            <span class="data-grid__th-inner">
              <span class="data-grid__th-label">{{ column.header }}</span>
              <!-- Flèche hors du libellé : un titre long la rognerait avec lui. -->
              <span v-if="tri && tri.id === column.id" class="data-grid__fleche" aria-hidden="true">
                {{ SORT_ICONS[tri.mode] }}
              </span>
            </span>

            <!-- Poignée posée sur le filet de droite (le `th` ne rogne donc pas
                 son contenu : c'est le libellé qui porte l'ellipse). -->
            <span
              class="data-grid__resize"
              role="separator"
              :aria-label="t('grid.resize', { column: column.header })"
              @click.stop
              @pointerdown="redimensionne($event, column)"
            />
          </th>
        </tr>
      </thead>

      <tbody>
        <!-- Cales de virtualisation : elles gardent la hauteur totale réelle, donc
             la barre de défilement ne bouge pas quand on défile. -->
        <tr
          v-if="fenetre.paddingTop"
          class="data-grid__spacer"
          aria-hidden="true"
          :style="{ height: `${fenetre.paddingTop}px` }"
        >
          <td :colspan="columns.length" />
        </tr>

        <tr
          v-for="ligne in lignesRendues"
          :key="cleDe(ligne.row)"
          role="row"
          class="data-grid__row"
          :class="{ 'data-grid__row--on': estSelectionnee(ligne.row) }"
          :aria-rowindex="ligne.index + 2"
          :aria-selected="estSelectionnee(ligne.row)"
          @click="selectionne(ligne.row, ligne.index, curseur?.colonne ?? 0)"
          @dblclick="emit('open', ligne.row)"
        >
          <td
            v-for="(column, index) in columns"
            :key="column.id"
            role="gridcell"
            class="data-grid__td"
            :class="[
              `data-grid__td--${column.align ?? 'left'}`,
              { 'data-grid__td--focus': curseur?.ligne === ligne.index && curseur?.colonne === index },
            ]"
            :aria-colindex="index + 1"
          >
            <slot :name="`cell-${column.id}`" :row="ligne.row" :index="ligne.index">
              {{ texte(ligne.row, column) }}
            </slot>
          </td>
        </tr>

        <tr
          v-if="fenetre.paddingBottom"
          class="data-grid__spacer"
          aria-hidden="true"
          :style="{ height: `${fenetre.paddingBottom}px` }"
        >
          <td :colspan="columns.length" />
        </tr>

        <!-- Aucune ligne : un seul message, sur toute la largeur. -->
        <tr v-if="!lignes.length" role="row">
          <td :colspan="columns.length" role="gridcell" class="data-grid__empty">
            <slot name="empty">{{ loading ? t('common.loading') : empty }}</slot>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
