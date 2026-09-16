<script setup lang="ts">
import type { FolderInfo, SubFolderInfo } from '~/composables/useModels'

const route = useRoute()
const { data, error, live } = useModels()
const { t } = useI18n()

/**
 * Chemin relatif du dossier, encodé dans l'URL exactement comme pour les fichiers
 * (`/dossiers/Maison%2Fsous`) : un dossier de premier niveau comme un sous-dossier
 * passent par la même page.
 */
const rel = computed(() =>
  decodeURIComponent(String(route.params.name)).replace(/^\/+|\/+$/g, ''),
)

/**
 * Dossier de premier niveau qui porte le scan : `scan_models` ne détaille que
 * ceux-là, les sous-dossiers étant décrits à plat (`subfolders`).
 */
const top = computed<FolderInfo | null>(() => {
  const [name] = segments(rel.value)
  return data.value?.folders[name ?? ''] ?? null
})

/** Dossier courant, sous-dossier compris. `null` s'il n'existe pas. */
const view = computed(() => (top.value ? folderView(top.value, rel.value) : null))

/** Sous-dossiers **directs** : ce que la page doit distinguer des fichiers. */
const subfolders = computed(() => view.value?.subfolders ?? [])

// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')

// Filtre global : barre rendue par le layout `default`, état partagé.
const { filtering, matches, sortFiles } = useFilter()

/** Les formats proposés sont ceux du dossier courant (sous-dossiers compris). */
useFilterTypes(() => view.value?.files ?? [])

/**
 * Fichiers affichés.
 *
 * Sans filtre : ceux rangés **directement** dans le dossier, puisque les
 * sous-dossiers ont leur propre section. Pendant une recherche : tout le
 * sous-arbre, car on cherche un fichier sans savoir où il est rangé.
 */
const files = computed(() => {
  const all = view.value?.files ?? []
  return sortFiles((filtering.value ? all : directFiles(all, rel.value)).filter(matches))
})

// En-tête global (rendu par le layout `default`).
usePageHeader(() => {
  const count = view.value?.count ?? 0
  const shown = files.value.length
  return {
    subtitle: filtering.value
      ? t('folder.subtitleFiltered', { shown, count }, count)
      : t('folder.subtitle', count, { count }),
    count: 0,
    live: live.value,
    offline: !!error.value,
  }
})

/** `FolderCard` attend la forme d'un dossier du scan (avec ses fichiers). */
function cardOf(sub: SubFolderInfo): FolderInfo {
  return {
    name: sub.name,
    count: sub.count,
    modified: sub.modified,
    note: sub.note,
    files: filesUnder(view.value?.files ?? [], sub.rel),
  }
}
</script>

<template>
  <BackLink />

  <div v-if="error" class="error">{{ error }}</div>

  <!-- Note du dossier (Markdown) : affichage + édition assistée. -->
  <NotePanel v-if="view" :key="view.rel" :rel="view.rel" :note="view.note" />

  <template v-if="view">
    <!--
      Sous-dossiers : les mêmes cartes que l'accueil, mais on reste dans la
      page. Cachés pendant une recherche : on cherche un fichier, pas un dossier.
    -->
    <template v-if="!filtering && subfolders.length">
      <p class="section-label">
        {{ $t('folder.subfolders', { count: subfolders.length }) }}
      </p>
      <div class="file-grid">
        <FolderCard
          v-for="sub in subfolders"
          :key="sub.rel"
          :folder="cardOf(sub)"
          :display-mode="displayMode"
          :to="folderHref(sub.rel)"
        />
      </div>

      <!-- L'intitulé « Fichiers » ne sert que s'il y a des sous-dossiers à
           distinguer ; sinon la liste se suffit à elle-même. -->
      <p class="section-label">{{ $t('folder.files', { count: files.length }) }}</p>
    </template>

    <div class="file-grid">
      <FileItem v-for="f in files" :key="f.path" :file="f" :display-mode="displayMode" />
      <p v-if="!files.length" class="empty">
        {{ filtering ? $t('folder.noMatch') : $t('folder.empty') }}
      </p>
    </div>
  </template>

  <!-- Chemin inconnu (lien obsolète, dossier renommé) : le dire, plutôt que de
       rester indéfiniment sur « chargement ». -->
  <div v-else-if="data" class="empty">{{ $t('folder.notFound') }}</div>
  <div v-else class="empty">{{ $t('common.loading') }}</div>
</template>
