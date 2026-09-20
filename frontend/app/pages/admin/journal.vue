<script setup lang="ts">
import { describeDetail, eventKey, type DetailShape, type JournalEvent } from '~/utils/journal'

/**
 * Journal d'audit (`/admin/journal`).
 *
 * Qui s'est connecté, qui a été refusé, quels jetons ont été créés ou révoqués,
 * quels droits ont changé. C'est ce qu'on relit le jour où un accès surprend —
 * et c'est **en lecture seule** : un journal modifiable ne serait pas un témoin.
 *
 * `admin.ts` vérifie le droit d'entrée (`users.read`) : un compte qui ne l'a pas
 * est renvoyé au catalogue plutôt que de découvrir un refus.
 */
definePageMeta({ middleware: 'admin' })

const { t, te } = useI18n()
const { events, loading, errorCode, refresh } = useJournal()

onMounted(() => void refresh())

/** Date lisible, et l'heure (un journal se lit à la minute près). */
function quand(secondes: number): string {
  return new Date(secondes * 1000).toLocaleString()
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

/** Une ligne : le type, puis de quoi le situer (qui, où, quand). */
const lignes = computed(() =>
  events.value.map((event) => ({
    event,
    kind: kindLabel(event.kind),
    detail: detailText(event),
    refus: estRefus(event.kind),
    quand: quand(event.at),
  })),
)
</script>

<template>
  <div v-if="errorCode" class="error">{{ t('journal.errors.load') }}</div>

  <div class="admin">
    <section class="admin__card">
      <h2 class="admin__title">
        {{ t('journal.title') }}
        <span class="admin__count">{{ events.length }}</span>
      </h2>
      <p class="admin__hint">{{ t('journal.hint') }}</p>

      <ul v-if="lignes.length" class="admin__list">
        <li
          v-for="(ligne, index) in lignes"
          :key="index"
          class="admin__item"
          :class="{ 'admin__item--alert': ligne.refus }"
        >
          <span class="admin__item-label">
            {{ ligne.kind }}
            <span v-if="ligne.event.actor" class="accounts__tag">{{ ligne.event.actor }}</span>
          </span>
          <span class="admin__item-where" :title="ligne.event.user_agent">
            {{ ligne.quand }}
            <template v-if="ligne.event.subject"> · {{ ligne.event.subject }}</template>
            <template v-if="ligne.detail"> · {{ ligne.detail }}</template>
            <template v-if="ligne.event.ip"> · {{ ligne.event.ip }}</template>
          </span>
        </li>
      </ul>
      <p v-else class="admin__empty">{{ loading ? t('common.loading') : t('journal.empty') }}</p>
    </section>
  </div>
</template>
