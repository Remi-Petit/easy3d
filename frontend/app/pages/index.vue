<script setup lang="ts">
import type { FileInfo, FolderInfo } from '~/composables/useModels'

const { data, error, live } = useModels()

const query = ref('')

/** Une recherche est active dès que le champ contient du texte. */
const searching = computed(() => query.value.trim().length > 0)

/** Fichier enrichi du nom du dossier d'origine (absent si le fichier est à la racine). */
type SearchFile = FileInfo & { folder?: string }

/** Vue par défaut (aucune recherche) : tous les dossiers. */
const allFolders = computed<FolderInfo[]>(() =>
  data.value ? Object.values(data.value.folders) : [],
)

/** Vue par défaut (aucune recherche) : fichiers à la racine. */
const rootFiles = computed<FileInfo[]>(() => data.value?.files ?? [])

/** Recherche : dossiers dont le nom correspond. */
const matchedFolders = computed<FolderInfo[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !q) return []
  return allFolders.value.filter((f) => f.name.toLowerCase().includes(q))
})

/** Recherche : fichiers dont le nom correspond, à la racine ET dans les dossiers. */
const matchedFiles = computed<SearchFile[]>(() => {
  const q = query.value.trim().toLowerCase()
  if (!data.value || !q) return []
  const match = (f: FileInfo) => basename(f.path).toLowerCase().includes(q)
  const nested: SearchFile[] = Object.entries(data.value.folders).flatMap(
    ([name, folder]) => folder.files.filter(match).map((f) => ({ ...f, folder: name })),
  )
  return [...data.value.files.filter(match), ...nested]
})

const resultCount = computed(() => matchedFolders.value.length + matchedFiles.value.length)

const totalCount = computed(() => data.value?.count ?? 0)
// Mode d'affichage issu de la config backend ("image" | "3d").
const displayMode = computed(() => data.value?.config?.display?.mode ?? '3d')
// "connecté" = WS live (temps réel) OU données qui remontent (polling sans erreur).
const connected = computed(() => live.value || !error.value)
const liveLabel = computed(() =>
  live.value ? 'temps réel' : error.value ? 'hors ligne' : 'repli polling',
)
</script>

<template>
  <div class="container">
    <header class="header">
      <div class="brand">
        <div class="logo">3D</div>
        <div>
          <h1>easy3d</h1>
          <small>catalogue de modèles · STL / 3MF / GCODE</small>
        </div>
      </div>

      <div class="stats">
        <span class="pill"><span class="dot" :class="connected ? 'live' : 'err'" /> {{ liveLabel }}</span>
        <span class="pill"><b>{{ totalCount }}</b> fichiers</span>
      </div>
    </header>

    <div class="search">
      <span class="icon">🔍</span>
      <input v-model="query" type="text" placeholder="Filtrer par nom de fichier ou de dossier…" />
    </div>

    <div v-if="error" class="error">{{ error }}</div>

    <template v-if="data">
      <!-- Recherche active : une seule section « All » regroupant dossiers + fichiers. -->
      <template v-if="searching">
        <p class="section-label">All ({{ resultCount }})</p>
        <div v-if="resultCount" class="file-grid">
          <FolderCard v-for="f in matchedFolders" :key="`folder:${f.name}`" :folder="f" />
          <FileItem
            v-for="f in matchedFiles"
            :key="f.path"
            :file="f"
            :folder="f.folder"
            :display-mode="displayMode"
          />
        </div>
        <div v-else class="empty">Aucun résultat pour « {{ query.trim() }} ».</div>
      </template>

      <!-- Vue par défaut : Dossiers + Racine. -->
      <template v-else>
        <p class="section-label">Dossiers</p>
        <div v-if="allFolders.length" class="file-grid">
          <FolderCard v-for="f in allFolders" :key="f.name" :folder="f" />
        </div>
        <div v-else class="empty">Aucun dossier trouvé.</div>

        <p class="section-label">Racine ({{ rootFiles.length }})</p>
        <article class="card" v-if="rootFiles.length">
          <div class="file-grid">
            <FileItem v-for="f in rootFiles" :key="f.path" :file="f" :display-mode="displayMode" />
          </div>
        </article>
        <div v-else class="empty">Aucun fichier à la racine.</div>
      </template>
    </template>

    <div v-else class="empty">Chargement…</div>
  </div>
</template>
