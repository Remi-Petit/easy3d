<script setup lang="ts">
import type { DisplayMode, FileInfo, ConfigResponse, WatchInfo } from '~/composables/useModels'
import { modelOptions } from '~/utils/ai'
import type { AiConfig, AiModelsResponse } from '~/utils/ai'

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

// Re-scan périodique : `null` (champ vide) = automatique, `0` = désactivé.
const pollSeconds = ref<number | string | null>(null)
const watchInfo = ref<WatchInfo | null>(null)

/** Mode **appliqué** par le backend : la référence, pas notre sélection. */
const appliedMode = computed(() => data.value?.config?.display?.mode ?? '3d')

/**
 * Valeur à enregistrer : le champ vidé veut dire « automatique », pas `0`
 * (qui, lui, désactive le re-scan — l'utilisateur se prononce).
 */
const pollValue = computed<number | null>(() => {
  const raw = pollSeconds.value
  if (raw === null || raw === undefined || raw === '') return null
  const seconds = Number(raw)
  return Number.isFinite(seconds) ? Math.max(0, Math.trunc(seconds)) : null
})

/**
 * Dossier réellement lu. Il n'est plus modifiable ici — il se change dans
 * `backend/config.yml` — mais l'afficher évite de se demander d'où viennent les
 * fichiers (`models_root` peut être vide, ou relatif à `backend/`).
 *
 * Le même appel rapporte la recommandation de re-scan : c'est le backend qui
 * connaît le système de fichiers de `models/`, donc lui qui sait si les
 * événements suffisent (voir `config::watch_poll_recommendation`).
 */
const resolvedRoot = ref('')

async function loadResolvedRoot() {
  try {
    const res = await $fetch<ConfigResponse>('/api/config')
    resolvedRoot.value = res.models_root
    watchInfo.value = res.watch
  } catch {
    resolvedRoot.value = ''
    watchInfo.value = null
  }
}

onMounted(() => {
  void loadResolvedRoot()
  // La liste des fournisseurs et l'état du bouton de recherche vivent dans
  // `useAiSearch` : on les relit ici, c'est le seul endroit où on les modifie.
  void refreshAi()
})

// ── Recherche assistée (IA) ───────────────────────────────────────────────────
// Le fournisseur et sa clé se règlent ici ; le bouton ✦ de la barre de recherche
// ne s'active qu'une fois un fournisseur choisi (voir `useAiSearch`).
const ai = useAiSearch()
// Les refs renvoyées par le composable sont déstructurées : le template ne
// déplie automatiquement que les refs de premier niveau du `setup`.
const { providers: aiProviders, refresh: refreshAi } = ai
const aiProvider = ref('')
const aiBaseUrl = ref('')
// Le modèle n'est plus saisi à la main : il se choisit parmi ceux que le
// fournisseur annonce pour la clé (bouton « Tester »).
const aiModel = ref('')
const aiModels = ref<string[]>([])
// La clé n'est jamais renvoyée en clair : le champ contient `***` quand une clé
// est enregistrée, et c'est cette valeur qu'on renvoie telle quelle pour la
// conserver (voir `Ai::merge_key`).
const aiKey = ref('')
const aiTest = ref<{ state: 'idle' | 'running' | 'ok' | 'error'; message: string }>({
  state: 'idle',
  message: '',
})
const aiSave = ref<{ state: 'idle' | 'saving' | 'ok' | 'error'; message: string }>({
  state: 'idle',
  message: '',
})
/**
 * Le formulaire a été touché : le serveur ne le réaligne plus.
 *
 * Sans ce verrou, le premier message du WebSocket (à chaque écriture de
 * `config.yml`, y compris par nous) écraserait une clé en cours de saisie.
 */
const aiTouched = ref(false)

/** Fournisseur choisi, tel que le backend le décrit (défauts compris). */
const aiDefaults = computed(() => aiProviders.value.find((p) => p.id === aiProvider.value) ?? null)
/** Le fournisseur exige-t-il une clé ? (faux pour un Ollama local) */
const aiNeedsKey = computed(() => aiDefaults.value?.needs_key ?? true)
/** Modèle enregistré, tel que le backend le connaît. */
const aiSavedModel = computed(() => data.value?.config?.ai?.model ?? '')

/** Choix proposés : ce que le fournisseur a annoncé, plus le modèle en cours. */
const aiModelOptions = computed(() =>
  modelOptions([...aiModels.value, aiSavedModel.value], aiModel.value),
)

/** Le formulaire a été modifié : c'est lui qui fait foi jusqu'au prochain
 * enregistrement. */
function markAiTouched() {
  aiTouched.value = true
  aiTest.value = { state: 'idle', message: '' }
  aiSave.value = { state: 'idle', message: '' }
}

/**
 * Choix du fournisseur : on montre l'adresse qui sera utilisée si le champ est
 * vide, et la liste des modèles repart de zéro (elle appartient à une adresse et
 * à une clé données).
 */
function pickProvider() {
  markAiTouched()
  aiModels.value = []

  const defaults = aiDefaults.value
  if (defaults && !aiBaseUrl.value.trim()) aiBaseUrl.value = defaults.base_url
}

/** Efface la clé enregistrée : vide veut dire « supprime-la » côté backend. */
function clearAiKey() {
  markAiTouched()
  aiKey.value = ''
}

/**
 * Interroge le fournisseur et remplit la liste des modèles.
 *
 * Rien n'est enregistré : les valeurs du formulaire partent telles quelles, et
 * `***` dit au backend de réutiliser la clé qu'il détient. C'est aussi la
 * vérification de la clé — un refus remonte le message du fournisseur.
 */
async function testAi() {
  aiTest.value = { state: 'running', message: '' }

  try {
    const res = await $fetch<AiModelsResponse>('/api/ai/models', {
      method: 'POST',
      body: {
        provider: aiProvider.value || null,
        base_url: aiBaseUrl.value.trim() || null,
        api_key: aiKey.value.trim() || null,
      },
    })

    aiModels.value = res.models
    // Un choix par défaut utile, sans écraser un modèle déjà retenu.
    if (!aiModel.value || !res.models.includes(aiModel.value)) {
      aiModel.value = res.models[0] ?? ''
    }
    aiTest.value = { state: 'ok', message: t('ai.modelsFound', res.models.length) }
  } catch (e: any) {
    aiModels.value = []
    aiTest.value = {
      state: 'error',
      message: e?.data?.message ?? e?.data?.cause ?? e?.message ?? t('common.unknownError'),
    }
  }
}

/**
 * Enregistre la carte IA.
 *
 * Contrairement aux réglages d'affichage (qui s'appliquent au changement), rien
 * n'est écrit avant ce clic : la clé ne part qu'une fois, et le modèle a dû être
 * choisi dans ce que le fournisseur annonce.
 */
async function saveAi() {
  aiSave.value = { state: 'saving', message: '' }

  try {
    await $fetch('/api/config', { method: 'PUT', body: configBody(formAi()) })
    aiSave.value = { state: 'ok', message: t('admin.saved') }
    aiTouched.value = false
    await loadResolvedRoot()
    // Le bouton ✦ de la barre de recherche suit la configuration enregistrée.
    void refreshAi()
  } catch (e: any) {
    aiSave.value = {
      state: 'error',
      message: e?.data?.message ?? e?.data?.cause ?? e?.message ?? t('admin.saveFailed'),
    }
  }
}

/** Phrase expliquant la recommandation, construite depuis les faits du backend. */
const recommendation = computed(() => {
  const info = watchInfo.value
  if (!info) return ''

  if (!info.filesystem) return t('admin.watchUnknown')
  return info.recommended > 0
    ? t('admin.watchVirtual', { fs: info.filesystem, seconds: info.recommended })
    : t('admin.watchLocal', { fs: info.filesystem })
})

/** Ce que le backend applique **maintenant** (après rechargement à chaud). */
const appliedWatch = computed(() => {
  const effective = watchInfo.value?.effective ?? 0
  return effective > 0
    ? t('admin.watchEvery', { seconds: effective })
    : t('admin.watchOff')
})

/** Applique la valeur recommandée (et l'enregistre si elle diffère). */
function useRecommended() {
  if (!watchInfo.value) return
  pollSeconds.value = watchInfo.value.recommended
  persist()
}

/**
 * Retire la valeur choisie : retour à « automatique », où c'est la
 * recommandation du backend qui décide. Le champ vide *est* ce mode, mais il
 * faut pouvoir y revenir après avoir saisi un nombre.
 */
function resetToAuto() {
  pollSeconds.value = null
  persist()
}

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

// Même principe pour le re-scan : la valeur **appliquée** fait foi, donc un
// changement venu d'ailleurs (fichier modifié à la main) réaligne le champ.
watch(
  () => data.value?.config?.watch?.poll_seconds,
  (serverValue) => {
    if (inFlight) return
    pollSeconds.value = serverValue ?? null
  },
  { immediate: true },
)

// …et pour la recherche assistée : la configuration appliquée réaligne le
// formulaire, y compris la clé masquée. Un formulaire déjà touché est laissé
// tranquille : sinon la première réponse du WebSocket effacerait la saisie en
// cours (et remettrait `***` au milieu d'une clé fraîchement tapée).
watch(
  () => data.value?.config?.ai,
  (server) => {
    if (aiTouched.value) return
    aiProvider.value = server?.provider ?? ''
    aiBaseUrl.value = server?.base_url ?? ''
    aiModel.value = server?.model ?? ''
    aiKey.value = server?.api_key ?? ''
    // La liste voyage avec la configuration : la liste déroulante est donc
    // remplie dès l'ouverture de la page, sans recliquer sur « Tester ».
    aiModels.value = server?.models ?? []
  },
  { immediate: true },
)

/**
 * Corps de configuration complet.
 *
 * Le bloc IA vient de l'appelant : les réglages d'affichage s'enregistrent au
 * changement et ne doivent **pas** emporter une clé en cours de saisie, alors
 * que le bouton « Enregistrer » de la carte IA fait précisément l'inverse.
 */
function configBody(ai: AiConfig) {
  return {
    models_root: data.value?.config?.models_root ?? null,
    display: { mode: mode.value },
    watch: { poll_seconds: pollValue.value },
    ai,
  }
}

/**
 * Bloc IA **enregistré**, tel que le backend le connaît.
 *
 * La clé y est masquée (`***`) : la renvoyer telle quelle la conserve, et un
 * bloc absent reste un bloc absent (rien à effacer).
 */
function storedAi(): AiConfig {
  return data.value?.config?.ai ?? {}
}

/** Bloc IA du formulaire, prêt à être enregistré. */
function formAi(): AiConfig {
  return {
    // `null` = pas de fournisseur : la clé est alors effacée côté backend.
    provider: aiProvider.value || null,
    base_url: aiBaseUrl.value.trim() || null,
    model: aiModel.value || null,
    // `***` = « garde la clé enregistrée », vide = « supprime-la ».
    api_key: aiProvider.value ? aiKey.value.trim() : null,
    // Les modèles annoncés sont enregistrés avec le reste : on choisit ensuite
    // un autre modèle sans avoir à réinterroger le fournisseur.
    models: aiModels.value,
  }
}

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
          // Le bloc IA part tel qu'il est **enregistré** : ces réglages-ci ne
          // touchent pas aux champs de la carte IA, qui ont leur propre bouton.
          body: configBody(storedAi()),
        })
        saveState.value = 'ok'
        saveMessage.value = t('admin.saved')
        await loadResolvedRoot()
        // Le bouton de recherche de la barre d'outils suit : il s'active dès
        // qu'un fournisseur est choisi.
        void refreshAi()      } catch (e: any) {
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

      <!-- Re-scan périodique : nécessaire seulement là où le système de fichiers
           ne signale pas les dépôts faits hors de l'interface (montages
           virtualisés ou réseau). Le backend dit ce qu'il en est ici. -->
      <div class="admin__field">
        <span class="admin__label">{{ $t('admin.watch') }}</span>
        <div class="admin__row">
          <input
            v-model="pollSeconds"
            class="admin__input admin__input--short"
            type="number"
            min="0"
            :placeholder="$t('admin.watchAuto')"
            @change="persist"
          />
          <span class="admin__unit">{{ $t('admin.watchUnit') }}</span>
          <button
            v-if="watchInfo && watchInfo.recommended !== (watchInfo.effective ?? 0)"
            type="button"
            class="admin__action"
            @click="useRecommended"
          >
            {{ $t('admin.watchUse', { seconds: watchInfo.recommended }) }}
          </button>
          <!-- Non affiché en mode automatique : il n'y aurait rien à retirer. -->
          <button
            v-if="pollValue !== null"
            type="button"
            class="admin__action"
            @click="resetToAuto"
          >
            {{ $t('admin.watchAutoAction') }}
          </button>
        </div>
        <p class="admin__hint">{{ $t('admin.watchHint') }}</p>
        <p v-if="recommendation" class="admin__hint admin__hint--rec">{{ recommendation }}</p>
      </div>

      <dl class="admin__applied">
        <div>
          <dt>{{ $t('admin.applied') }}</dt>
          <dd>{{ $t(appliedMode === 'image' ? 'admin.modeImage' : 'admin.mode3d') }}</dd>
        </div>
        <div>
          <dt>{{ $t('admin.watchApplied') }}</dt>
          <dd>{{ appliedWatch }}</dd>
        </div>
        <div>
          <dt>{{ $t('admin.rootRead') }}</dt>
          <dd :title="resolvedRoot">{{ resolvedRoot || $t('common.none') }}</dd>
        </div>
      </dl>
    </section>

    <!-- Recherche assistée : le fournisseur de modèle et sa clé d'API. Tant
         qu'aucun fournisseur n'est choisi, le bouton ✦ de la barre de
         recherche reste grisé. -->
    <section class="admin__card">
      <h2 class="admin__title">{{ $t('ai.settings') }}</h2>

      <div class="admin__field admin__field--ai">
        <label class="admin__label" for="ai-provider">{{ $t('ai.provider') }}</label>
        <div class="admin__row">
          <select
            id="ai-provider"
            v-model="aiProvider"
            class="admin__input"
            @change="pickProvider"
          >
            <option value="">{{ $t('ai.none') }}</option>
            <option v-for="p in aiProviders" :key="p.id" :value="p.id">
              {{ p.label }}
            </option>
          </select>
        </div>
        <p class="admin__hint">{{ $t('ai.providerHint') }}</p>
      </div>

      <div class="admin__field admin__field--ai">
        <label class="admin__label" for="ai-base-url">{{ $t('ai.baseUrl') }}</label>
        <div class="admin__row">
          <input
            id="ai-base-url"
            v-model="aiBaseUrl"
            class="admin__input"
            type="text"
            :placeholder="aiDefaults?.base_url ?? ''"
            @input="markAiTouched"
          />
        </div>
        <p class="admin__hint">{{ $t('ai.baseUrlHint') }}</p>
      </div>

      <div class="admin__field admin__field--ai">
        <label class="admin__label" for="ai-key">{{ $t('ai.apiKey') }}</label>
        <div class="admin__row">
          <input
            id="ai-key"
            v-model="aiKey"
            class="admin__input"
            type="password"
            autocomplete="off"
            :placeholder="aiNeedsKey ? 'sk-...' : $t('ai.noKeyNeeded')"
            @input="markAiTouched"
          />
          <button v-if="aiKey" type="button" class="admin__action" @click="clearAiKey">
            {{ $t('ai.apiKeyClear') }}
          </button>
          <!--
            « Tester » ne se contente pas de vérifier la clé : il demande au
            fournisseur les modèles qu'elle ouvre, et remplit la liste de choix
            ci-dessous. Rien n'est enregistré à ce stade.
          -->
          <button
            type="button"
            class="admin__action"
            :disabled="!aiProvider || aiTest.state === 'running'"
            @click="testAi"
          >
            {{ aiTest.state === 'running' ? $t('ai.testing') : $t('ai.test') }}
          </button>
        </div>
        <p class="admin__hint">{{ $t('ai.apiKeyHint') }}</p>
        <p
          v-if="aiTest.message"
          class="admin__hint"
          :class="aiTest.state === 'ok' ? 'admin__hint--rec' : 'admin__status--err'"
        >
          {{ aiTest.message }}
        </p>
      </div>

      <div class="admin__field admin__field--ai">
        <label class="admin__label" for="ai-model">{{ $t('ai.model') }}</label>
        <div class="admin__row">
          <select
            id="ai-model"
            v-model="aiModel"
            class="admin__input"
            @input="markAiTouched"
          >
            <option v-if="!aiModelOptions.length" value="">{{ $t('common.none') }}</option>
            <option v-for="name in aiModelOptions" :key="name" :value="name">
              {{ name }}
            </option>
          </select>
        </div>
        <p class="admin__hint">{{ $t('ai.modelHint') }}</p>
      </div>

      <!--
        Enregistrement explicite : la clé ne part qu'ici, une fois, et après
        avoir choisi un modèle dans ce que le fournisseur annonce.
      -->
      <div class="admin__field admin__field--actions">
        <!-- Actif même sans fournisseur : c'est ainsi qu'on désactive la
             recherche (le backend efface alors la clé enregistrée). -->
        <button
          type="button"
          class="admin__action admin__action--primary"
          :disabled="aiSave.state === 'saving'"
          @click="saveAi"
        >
          {{ aiSave.state === 'saving' ? '…' : $t('ai.save') }}
        </button>
        <span v-if="aiSave.state === 'ok'" class="admin__status admin__status--ok">
          {{ aiSave.message }}
        </span>
        <span v-else-if="aiSave.state === 'error'" class="admin__status admin__status--err">
          {{ aiSave.message }}
        </span>
      </div>
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
