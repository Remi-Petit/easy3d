<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import {
  describeDetail,
  eventFamily,
  eventKey,
  EVENT_FAMILY_LIST,
  EVENT_ICONS,
  familyCounts,
  filterEvents,
  sortEvents,
  type DetailShape,
  type EventFamily,
  type JournalEvent,
} from '~/utils/journal'
import { nextSortMode, SORT_ICONS, SORT_LABEL_KEYS, type SortMode } from '~/utils/filter'

/**
 * Journal d'audit (`/admin/audit`).
 *
 * Qui s'est connecté, qui a été refusé, quels jetons ont été créés ou révoqués,
 * quels droits ont changé. C'est ce qu'on relit le jour où un accès surprend —
 * et c'est **en lecture seule** : un journal modifiable ne serait pas un témoin.
 *
 * `admin.ts` vérifie le droit d'entrée (`users.read`) : un compte qui ne l'a pas
 * est renvoyé au catalogue plutôt que de découvrir un refus.
 */
definePageMeta({ middleware: 'admin' })

const { t, te, locale } = useI18n()
const { events, loading, errorCode, refresh } = useJournal()

onMounted(() => void refresh())

/**
 * Recherche assistée : l'état est **partagé** (`useState`, voir `useAiSearch`), et
 * la barre ci-dessous en reprend les commandes. La page a la sienne — plutôt que
 * celle du layout — parce que le journal a ses propres filtres, et que le bouton
 * doit se trouver dans cette rangée-là, à côté du tri et de la période.
 */
const {
  configured: aiReady,
  mode: aiMode,
  query: aiQuestion,
  loading: aiLoading,
  run: runAi,
  toggle: toggleAi,
} = useAiSearch()

/**
 * Date lisible, et l'heure (un journal se lit à la minute près).
 *
 * La **langue de l'interface** est passée explicitement : `toLocaleString()` sans
 * argument suit celle du navigateur, et un journal en français afficherait alors
 * des dates au format américain.
 */
function quand(secondes: number): string {
  return new Date(secondes * 1000).toLocaleString(locale.value)
}

/** Libellé d'un type d'événement ; un type inconnu est montré tel quel. */
function kindLabel(kind: string): string {
  const key = eventKey(kind)
  return te(key) ? t(key) : kind
}

/** Les refus se repèrent d'un coup d'œil : c'est ce qu'on vient chercher. */
function estRefus(kind: string): boolean {
  return kind === 'login_failed' || kind === 'login_blocked' || kind === 'sso_refused'
}

// ── Filtres ──────────────────────────────────────────────────────────────
/**
 * Filtres **locaux** : le catalogue a les siens (`useFilter`, partagé avec la
 * barre du layout) — on ne mélange pas les deux.
 */
const recherche = ref('')
const famillesChoisies = ref<EventFamily[]>([])
const periode = ref(0)

/** Tri par date : **même bouton et même cycle** que le catalogue (voir `utils/filter.ts`). */
const tri = ref<SortMode>('none')
const triLabel = computed(() => t(SORT_LABEL_KEYS[tri.value]))
const triIcon = computed(() => SORT_ICONS[tri.value])

function cycleTri() {
  tri.value = nextSortMode(tri.value)
}

const filtreActif = computed(
  () => !!recherche.value.trim() || famillesChoisies.value.length > 0 || periode.value !== 0,
)

/** Instant le plus ancien accepté par la période choisie (`0` = aucune borne). */
function bornePeriode(): number {
  const jours = periode.value
  return jours === 0 ? 0 : Math.floor(Date.now() / 1000) - jours * 86_400
}

/** Pastille des puces : le nombre d'événements de la famille, sur la période. */
const compteurs = computed(() => familyCounts(events.value, bornePeriode()))

function basculeFamille(famille: EventFamily) {
  famillesChoisies.value = famillesChoisies.value.includes(famille)
    ? famillesChoisies.value.filter((connu) => connu !== famille)
    : [...famillesChoisies.value, famille]
}

function effacerFiltres() {
  recherche.value = ''
  famillesChoisies.value = []
  periode.value = 0
}

/**
 * Ce qu'on dit de la précision d'un événement.
 *
 * Le `detail` a un sens différent selon le type (durée d'un jeton, champs
 * modifiés, code d'erreur) : `describeDetail` (utilitaire testé) tranche, et la
 * traduction se fait ici, dans le `setup` de la page.
 */
function detailText(event: JournalEvent): string {
  const forme: DetailShape = describeDetail(event.kind, event.detail)
  switch (forme.kind) {
    case 'none':
      return ''
    case 'code': {
      const key = `journal.detail.${forme.code}`
      return te(key) ? t(key) : forme.code
    }
    case 'days':
      return forme.days > 0 ? t('journal.days', { count: forme.days }) : t('journal.neverExpires')
    case 'changes':
      return forme.fields.map((field) => (te(`journal.change.${field}`) ? t(`journal.change.${field}`) : field)).join(', ')
    default:
      return forme.value
  }
}

/** Une ligne du tableau : l'événement, et tout ce qu'il faut pour l'afficher. */
type Ligne = {
  /** Code stable du type : c'est lui qui porte la famille, l'icône et la couleur. */
  kind: string
  famille: EventFamily | null
  icone: string
  libelle: string
  date: string
  acteur: string
  sujet: string
  detail: string
  ip: string
  /** Le client HTTP, montré au survol de la ligne. */
  agent: string
  refus: boolean
}

/** Une ligne : le type, puis de quoi le situer (qui, quoi, quand). */
const lignes = computed<Ligne[]>(() => {
  const retenus = sortEvents(
    filterEvents(
      events.value,
      { text: recherche.value, families: famillesChoisies.value, since: bornePeriode() },
      // La recherche couvre **ce qui est affiché** : le type traduit, le compte, le
      // sujet, le détail traduit et l'adresse.
      (event) =>
        [kindLabel(event.kind), event.actor ?? '', event.subject, detailText(event), event.ip].join(
          ' ',
        ),
    ),
    tri.value,
  )

  return retenus.map((event) => {
    const famille = eventFamily(event.kind)
    return {
      kind: event.kind,
      famille,
      // Une famille inconnue (type d'un backend plus récent) reste lisible : le
      // point neutre plutôt qu'une icône qui voudrait dire quelque chose.
      icone: famille ? EVENT_ICONS[famille] : 'i-lucide-dot',
      libelle: kindLabel(event.kind),
      date: quand(event.at),
      acteur: event.actor ?? '—',
      sujet: event.subject || '—',
      detail: detailText(event) || '—',
      ip: event.ip || '—',
      agent: event.user_agent,
      refus: estRefus(event.kind),
    }
  })
})

/**
 * Les colonnes, décrites pour TanStack (le moteur de `UTable`) : Nuxt UI les
 * habille, la page n'écrit plus de `<td>`.
 *
 * Le libellé du type est pris sur `libelle` — sa valeur **traduite** — et c'est
 * la seule colonne qui a un rendu à elle (voir le gabarit) : l'icône et la
 * couleur de la famille.
 */
const colonnes = computed<TableColumn<Ligne>[]>(() => [
  {
    accessorKey: 'date',
    header: t('journal.col.when'),
    meta: { class: { td: 'audit__nowrap' } },
  },
  { accessorKey: 'libelle', header: t('journal.col.kind') },
  { accessorKey: 'acteur', header: t('journal.col.actor') },
  { accessorKey: 'sujet', header: t('journal.col.subject') },
  { accessorKey: 'detail', header: t('journal.col.detail') },
  {
    accessorKey: 'ip',
    header: t('journal.col.ip'),
    meta: { class: { td: 'audit__nowrap' } },
  },
])

/**
 * La seule classe qui dépend de la ligne : un refus porte un trait rouge
 * (l'API de Nuxt UI accepte une fonction, voir `meta.class.tr`).
 */
const meta = computed(() => ({
  class: {
    tr: (row: { original: Ligne }) => (row.original.refus ? 'audit__row--alert' : ''),
  },
}))

// ── Pagination ──────────────────────────────────────────────────────────
/** Tailles de page proposées : le journal se lit par petits bouts, ou par gros. */
const TAILLES = [25, 50, 100]

const page = ref(1)
const taillePage = ref(TAILLES[0])

/** Les lignes de la page courante (`UPagination` travaille en pages de 1). */
const pageCourante = computed(() =>
  lignes.value.slice((page.value - 1) * taillePage.value, page.value * taillePage.value),
)

/**
 * Toucher à un filtre ramène à la première page : sans ça, resserrer la
 * recherche laisserait sur une page devenue vide, et on croirait le journal
 * vide.
 */
watch([recherche, famillesChoisies, periode, taillePage], () => {
  page.value = 1
})
</script>

<template>
  <div v-if="errorCode" class="error">{{ t('journal.errors.load') }}</div>

  <!--
    Une seule colonne, **pleine largeur** : le journal se parcourt (colonnes
    alignées, dates à gauche, refus repérables). La grille à deux colonnes de la
    page Comptes lui laissait la moitié droite vide.
  -->
  <div class="audit">
    <!--
      La rangée reprend **celle du catalogue** : mêmes classes (`.toolbar`,
      `.search`, `.ai-toggle`, `.sort`), donc même facture — le champ large avec
      sa loupe, puis le bouton ✦, le tri et la période. Les puces des familles
      restent sur leur propre ligne, comme les formats au catalogue.
    -->
    <div class="toolbar">
      <!-- En recherche assistée, le champ reçoit la question (comme au catalogue). -->
      <div v-if="aiMode" class="search search--ai">
        <span class="icon">✦</span>
        <input
          v-model="aiQuestion"
          type="text"
          :placeholder="t('ai.placeholder')"
          :disabled="aiLoading"
          @keydown.enter="runAi()"
        />
        <button
          type="button"
          class="search__go"
          :disabled="aiLoading || !aiQuestion.trim()"
          :aria-label="t('ai.submit')"
          :title="t('ai.submit')"
          @click="runAi()"
        >
          {{ aiLoading ? '…' : '→' }}
        </button>
      </div>
      <div v-else class="search">
        <span class="icon">🔍</span>
        <input
          v-model="recherche"
          type="text"
          :placeholder="t('journal.search')"
          :aria-label="t('journal.search')"
        />
      </div>

      <span class="ai-toggle">
        <button
          type="button"
          class="ai-btn"
          :class="{ 'ai-btn--on': aiMode }"
          :disabled="!aiReady"
          :aria-pressed="aiMode"
          aria-describedby="ai-btn-help"
          @click="toggleAi()"
        >
          ✦ {{ t('ai.button') }}
        </button>
        <span id="ai-btn-help" class="ai-toggle__tip" role="tooltip">
          {{ aiReady ? t('ai.open') : t('ai.needSetup') }}
        </span>
      </span>

      <button
        v-if="!aiMode"
        type="button"
        class="sort"
        :class="`sort--${tri}`"
        :title="t('filter.sortHint', { mode: triLabel })"
        :aria-label="t('filter.sortBy', { mode: triLabel })"
        @click="cycleTri()"
      >
        <span class="sort__icon">{{ triIcon }}</span>
        <span class="sort__label">{{ t('filter.date') }}</span>
      </button>

      <select
        v-if="!aiMode"
        v-model.number="periode"
        class="admin__input admin__input--short"
        :aria-label="t('journal.period')"
      >
        <option :value="0">{{ t('journal.periods.all') }}</option>
        <option :value="1">{{ t('journal.periods.day') }}</option>
        <option :value="7">{{ t('journal.periods.week') }}</option>
        <option :value="30">{{ t('journal.periods.month') }}</option>
      </select>
    </div>

    <!-- Même vocabulaire que les puces de format du catalogue : famille + nombre. -->
    <div v-if="!aiMode" class="types">
      <button
        v-for="famille in EVENT_FAMILY_LIST"
        :key="famille"
        type="button"
        class="types__item"
        :class="{ 'types__item--on': famillesChoisies.includes(famille) }"
        @click="basculeFamille(famille)"
      >
        {{ t(`journal.family.${famille}`) }}
        <b>{{ compteurs[famille] }}</b>
      </button>
      <button v-if="filtreActif" type="button" class="types__clear" @click="effacerFiltres()">
        {{ t('filter.clear') }}
      </button>
    </div>

    <!-- Recherche assistée : l'assistant remplace le journal le temps de la
         question (on ne mélange pas une réponse rédigée et un tableau filtré). -->
    <AiResults v-if="aiMode" />

    <template v-else>
      <p class="audit__meta">
        <span class="section-label">
          {{
            filtreActif
              ? t('journal.filtered', { count: lignes.length, total: events.length })
              : t('journal.count', { count: events.length })
          }}
        </span>
        <span class="admin__hint">{{ t('journal.hint') }}</span>
      </p>

      <!--
        Le tableau vient de Nuxt UI (`UTable`, TanStack Table sous le capot) : il
        apporte l'en-tête collant, l'état vide, le rendu des cellules et les
        colonnes. Ce qui reste à cette page, c'est ce que ce journal a de
        particulier : les familles, leurs couleurs, et les refus marqués.
      -->
      <UTable
        :data="pageCourante"
        :columns="colonnes"
        :loading="loading"
        sticky="header"
        :meta="meta"
        :ui="{
          root: 'audit__table-root',
          base: 'audit__table',
          thead: 'audit__head',
          th: 'audit__th',
          td: 'audit__td',
          empty: 'admin__empty',
        }"
      >
        <!-- Le type d'événement : l'icône et la couleur de sa famille — un refus
             ne se lit pas comme une connexion. -->
        <template #libelle-cell="{ row }">
          <span
            class="audit__kind"
            :class="row.original.famille && `audit__kind--${row.original.famille}`"
          >
            <UIcon :name="row.original.icone" aria-hidden="true" />
            {{ row.original.libelle }}
          </span>
        </template>

        <!-- La date porte le client HTTP en infobulle : c'est là qu'on va le
             chercher, et ça ne tient pas dans une colonne. -->
        <template #date-cell="{ row }">
          <span :title="row.original.agent">{{ row.original.date }}</span>
        </template>

        <template #empty>
          {{
            loading
              ? t('common.loading')
              : filtreActif
                ? t('journal.emptyFiltered')
                : t('journal.empty')
          }}
        </template>
      </UTable>

      <!-- Pagination : la taille de page à gauche, les numéros à droite. Elle
           n'apparaît que s'il y a plus d'une page à parcourir. -->
      <div v-if="lignes.length > taillePage" class="audit__pages">
        <label class="audit__perpage">
          {{ t('journal.perPage') }}
          <select v-model.number="taillePage" class="admin__input admin__input--short">
            <option v-for="taille in TAILLES" :key="taille" :value="taille">{{ taille }}</option>
          </select>
        </label>
        <UPagination
          v-model:page="page"
          :items-per-page="taillePage"
          :total="lignes.length"
          :sibling-count="1"
        />
      </div>
    </template>
  </div>
</template>
