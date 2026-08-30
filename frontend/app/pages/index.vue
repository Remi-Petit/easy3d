<script setup lang="ts">
import type { FolderInfo } from '~/composables/useModels'

const { data, error, live } = useModels()

const query = ref('')

const folders = computed<FolderInfo[]>(() => {
  if (!data.value) return []
  const q = query.value.trim().toLowerCase()
  const all = Object.values(data.value.folders)
  if (!q) return all
  return all.filter(
    (f) =>
      f.name.toLowerCase().includes(q) ||
      f.files.some((file) => basename(file.path).toLowerCase().includes(q)),
  )
})

const rootFiles = computed(() => {
  if (!data.value) return []
  const q = query.value.trim().toLowerCase()
  if (!q) return data.value.files
  return data.value.files.filter((f) => basename(f.path).toLowerCase().includes(q))
})

const totalCount = computed(() => data.value?.count ?? 0)
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
          <small>catalogue de modèles · STL / GCODE</small>
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
      <p class="section-label">Dossiers</p>
      <div v-if="folders.length" class="file-grid">
        <FolderCard v-for="f in folders" :key="f.name" :folder="f" />
      </div>
      <div v-else class="empty">Aucun dossier trouvé.</div>

      <p class="section-label">Racine ({{ rootFiles.length }})</p>
      <article class="card" v-if="rootFiles.length">
        <div class="file-grid">
          <FileItem v-for="f in rootFiles" :key="f.path" :file="f" />
        </div>
      </article>
      <div v-else class="empty">Aucun fichier à la racine.</div>
    </template>

    <div v-else class="empty">Chargement…</div>
  </div>
</template>
