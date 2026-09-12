<script setup lang="ts">
import type { DisplayMode, FileInfo } from '~/composables/useModels'

// Page d'administration : réglages de l'application (écrits dans `config.yml`
// via `PUT /api/config`) et gestion des notes.
//
// Les notes passent par le **même** éditeur que les pages de détail
// (`NotePanel`, CRDT Yjs) : on n'écrit jamais un `.md` directement, sinon on
// écrase le travail d'une autre session en cours.
const { data, error, live } = useModels()
const { t } = useI18n()

// ── Réglages ────────────────────────────────────────────────────────────────
// Un seul réglage exposé : le mode d'affichage. Il est écrit **dès le clic**
// (pas de bouton « Enregistrer ») ; le backend le valide, l'écrit, l'applique et
// rediffuse la configuration sur le WebSocket.
const mode = ref<DisplayMode>('3d')
const saveState = ref<'idle' | 'saving' | 'ok' | 'error'>('idle')
const saveMessage = ref('')

/** Mode **appliqué** par le backend : la référence, pas notre sélection. */
const appliedMode = computed(() => data.value?.config?.display?.mode ?? '3d')

/**
 * Dossier réellement lu. Il n'est plus modifiable ici — il se change dans
 * `backend/config.yml` — mais l'afficher évite de se demander d'où viennent les
 * fichiers (`models_root` peut être vide, ou relatif à `backend/`).
 */
const resolvedRoot = ref('')

async function loadResolvedRoot() {
  try {
    const res = await $fetch<{ models_root: string }>('/api/config')
    resolvedRoot.value = res.models_root
  } catch {
    resolvedRoot.value = ''
  }
}

onMounted(loadResolvedRoot)

/** Enregistrement en cours / dernière valeur demandée (voir `persist`). */
let inFlight = false
let queued = false

// La sélection suit la configuration **appliquée** : si `config.yml` est modifié
// ailleurs (à la main, depuis un autre onglet), le réglage se réaligne. On ne le
// fait pas pendant un enregistrement, pour ne pas écraser le choix en cours.
watch(
  () => data.value?.config?.display?.mode,
  (serverMode) => {
    if (!serverMode || inFlight) return
    mode.value = serverMode
  },
  { immediate: true },
)

/**
 * Écrit le réglage dans `config.yml`. Les clics rapprochés sont sérialisés :
 * on garde la dernière valeur demandée sans multiplier les requêtes.
 */
async function persist() {
  if (inFlight) {
    queued = true
    return
  }

  inFlight = true
  try {
    do {
      queued = false
      saveState.value = 'saving'

      try {
        await $fetch('/api/config', {
          method: 'PUT',
          // La config complète est renvoyée : `models_root`, non exposé ici,
          // est donc préservé tel quel.
          body: {
            models_root: data.value?.config?.models_root ?? null,
            display: { mode: mode.value },
          },
        })
        saveState.value = 'ok'
        saveMessage.value = t('admin.saved')
        await loadResolvedRoot()
      } catch (e: any) {
        saveState.value = 'error'
        saveMessage.value =
          e?.data?.message ?? e?.data?.cause ?? e?.message ?? t('admin.saveFailed')
      }
    } while (queued)
  } finally {
    inFlight = false
  }
}

// ── Notes ───────────────────────────────────────────────────────────────────
type NoteTarget = { rel: string; label: string; where: string; hasNote: boolean }

const noteQuery = ref('')
const selected = ref<string | null>(null)

/** Tout ce qui peut porter une note : les dossiers, puis les fichiers. */
const targets = computed<NoteTarget[]>(() => {
  if (!data.value) return []

  const folders: NoteTarget[] = Object.values(data.value.folders).map((f) => ({
    rel: f.name,
    label: f.name,
    where: t('admin.whereFolder'),
    hasNote: !!f.note,
  }))

  const files: NoteTarget[] = [
    ...data.value.files.map((f: FileInfo) => ({
      rel: f.rel,
      label: basename(f.rel),
      where: t('admin.whereRoot'),
      hasNote: !!f.note,
    })),
    ...Object.entries(data.value.folders).flatMap(([name, folder]) =>
      folder.files.map((f) => ({
        rel: f.rel,
        label: basename(f.rel),
        where: name,
        hasNote: !!f.note,
      })),
    ),
  ]

  return [...folders, ...files]
})

const noteCount = computed(() => targets.value.filter((target) => target.hasNote).length)

/** Filtre + tri : les éléments qui ont déjà une note remontent en tête. */
const filteredTargets = computed(() => {
  const q = noteQuery.value.trim().toLowerCase()
  return targets.value
    .filter((target) => !q || target.rel.toLowerCase().includes(q))
    .sort((a, b) => Number(b.hasNote) - Number(a.hasNote) || a.rel.localeCompare(b.rel))
})

/** Note de l'élément sélectionné (un `rel` désigne un dossier *ou* un fichier). */
const selectedNote = computed<string | null>(() => {
  const rel = selected.value
  if (!rel || !data.value) return null

  const folder = data.value.folders[rel]
  if (folder) return folder.note ?? null

  const files = [...data.value.files, ...Object.values(data.value.folders).flatMap((f) => f.files)]
  return files.find((f) => f.rel === rel)?.note ?? null
})

// En-tête : le sous-titre est un état partagé, cette page doit le déclarer.
usePageHeader(() => ({
  subtitle: t('admin.subtitle'),
  count: 0,
  live: live.value,
  offline: !!error.value,
}))
</script>

<template>
  <div v-if="error" class="error">{{ error }}</div>

  <div v-if="!data" class="empty">{{ $t('common.loading') }}</div>

  <div v-else class="admin">
    <!-- Réglages : écrits dans config.yml, appliqués à chaud par le backend. -->
    <section class="admin__card">
      <h2 class="admin__title">{{ $t('admin.settings') }}</h2>

      <div class="admin__field">
        <span class="admin__label">{{ $t('admin.display') }}</span>
        <div class="admin__choices">
          <label class="admin__choice" :class="{ 'admin__choice--on': mode === '3d' }">
            <input v-model="mode" type="radio" value="3d" @change="persist" />
            <span>{{ $t('admin.mode3d') }}</span>
          </label>
          <label class="admin__choice" :class="{ 'admin__choice--on': mode === 'image' }">
            <input v-model="mode" type="radio" value="image" @change="persist" />
            <span>{{ $t('admin.modeImage') }}</span>
          </label>
        </div>
        <p class="admin__hint">
          <i18n-t keypath="admin.hint" scope="global">
            <template #file><code>backend/config.yml</code></template>
          </i18n-t>
          <span v-if="saveState === 'saving'" class="admin__status">…</span>
          <span
            v-else-if="saveState === 'ok'"
            class="admin__status admin__status--ok"
          >{{ saveMessage }}</span>
          <span
            v-else-if="saveState === 'error'"
            class="admin__status admin__status--err"
          >{{ saveMessage }}</span>
        </p>
      </div>

      <dl class="admin__applied">
        <div>
          <dt>{{ $t('admin.applied') }}</dt>
          <dd>{{ $t(appliedMode === 'image' ? 'admin.modeImage' : 'admin.mode3d') }}</dd>
        </div>
        <div>
          <dt>{{ $t('admin.rootRead') }}</dt>
          <dd :title="resolvedRoot">{{ resolvedRoot || $t('common.none') }}</dd>
        </div>
      </dl>
    </section>

    <!-- Notes : liste des éléments, avec accès à l'éditeur collaboratif. -->
    <section class="admin__card">
      <h2 class="admin__title">
        {{ $t('admin.notes') }} <span class="admin__count">{{ noteCount }}</span>
      </h2>

      <input
        v-model="noteQuery"
        class="admin__input"
        type="search"
        :placeholder="$t('filter.items')"
      />

      <ul class="admin__list">
        <li v-for="t in filteredTargets" :key="t.rel">
          <button
            type="button"
            class="admin__item"
            :class="{ 'admin__item--on': selected === t.rel }"
            @click="selected = t.rel"
          >
            <span class="admin__item-label" :title="t.rel">{{ t.label }}</span>
            <span class="admin__item-where">{{ t.where }}</span>
            <span v-if="t.hasNote" :title="$t('admin.hasNote')">📝</span>
          </button>
        </li>
        <li v-if="!filteredTargets.length" class="admin__empty">{{ $t('admin.noItems') }}</li>
      </ul>
    </section>

    <!-- Édition de la note sélectionnée (temps réel, plusieurs personnes OK). -->
    <section v-if="selected" class="admin__card admin__card--wide">
      <h2 class="admin__title">{{ selected }}</h2>
      <NotePanel :key="selected" :rel="selected" :note="selectedNote" />
    </section>
  </div>
</template>
