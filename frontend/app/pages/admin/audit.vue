<script setup lang="ts">
import {
  describeDetail,
  eventKey,
  EVENT_FAMILY_LIST,
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

/** Une ligne : le type, puis de quoi le situer (qui, quoi, quand). */
const lignes = computed(() => {
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

  return retenus.map((event) => ({
    event,
    kind: kindLabel(event.kind),
    detail: detailText(event),
    refus: estRefus(event.kind),
    quand: quand(event.at),
  }))
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

      <table v-if="lignes.length" class="audit__table">
        <thead>
          <tr>
            <th scope="col">{{ t('journal.col.when') }}</th>
            <th scope="col">{{ t('journal.col.kind') }}</th>
            <th scope="col">{{ t('journal.col.actor') }}</th>
            <th scope="col">{{ t('journal.col.subject') }}</th>
            <th scope="col">{{ t('journal.col.detail') }}</th>
            <th scope="col">{{ t('journal.col.ip') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(ligne, index) in lignes"
            :key="index"
            :class="{ 'audit__row--alert': ligne.refus }"
            :title="ligne.event.user_agent"
          >
            <td class="audit__when">{{ ligne.quand }}</td>
            <td class="audit__kind">{{ ligne.kind }}</td>
            <td>{{ ligne.event.actor ?? '—' }}</td>
            <td class="audit__subject">{{ ligne.event.subject || '—' }}</td>
            <td>{{ ligne.detail || '—' }}</td>
            <td class="audit__ip">{{ ligne.event.ip || '—' }}</td>
          </tr>
        </tbody>
      </table>
      <p v-else class="admin__empty">
        {{
          loading
            ? t('common.loading')
            : filtreActif
              ? t('journal.emptyFiltered')
              : t('journal.empty')
        }}
      </p>
    </template>
  </div>
</template>
