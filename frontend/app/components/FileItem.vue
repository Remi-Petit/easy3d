<script setup lang="ts">
import type { FileInfo } from '~/composables/useModels'

const props = defineProps<{ file: FileInfo }>()

const name = computed(() => basename(props.file.path))
const type = computed(() => ext(props.file.path) || '?')
const when = computed(() => timeAgo(toDate(props.file.modified)))
</script>

<template>
  <li class="file">
    <span class="type">{{ type }}</span>
    <span class="name" :title="file.path">{{ name }}</span>
    <span class="meta">{{ when }}</span>
  </li>
</template>
