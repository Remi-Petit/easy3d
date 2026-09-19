<script setup lang="ts">
/**
 * Panneau de la recherche assistée : la question posée, les propositions du
 * modèle (avec sa raison) et les erreurs éventuelles.
 *
 * Le panneau remplace le catalogue tant que le mode est ouvert (voir
 * `layouts/default.vue`) : on ne mélange pas résultats et filtres classiques,
 * qui n'ont pas la même sémantique.
 */
import { hitHref } from '~/utils/ai'

const { outcome, loading, error, asked, reset } = useAiSearch()
</script>

<template>
  <section class="ai">
    <header class="ai__head">
      <span class="ai__badge">✦ {{ $t('ai.title') }}</span>
      <span v-if="asked" class="ai__asked" :title="asked">« {{ asked }} »</span>
      <button type="button" class="ai__clear" @click="reset()">{{ $t('ai.clear') }}</button>
    </header>

    <!-- Recherche en cours : plusieurs allers-retours vers le modèle. -->
    <p v-if="loading" class="ai__state">{{ $t('ai.searching') }}</p>

    <!-- Refus du backend (clé absente, fournisseur muet) : tel quel. -->
    <p v-else-if="error" class="ai__error">{{ error }}</p>

    <template v-else-if="outcome">
      <!-- Aucune proposition : le modèle explique, ou on le dit simplement. -->
      <p v-if="!outcome.hits.length" class="ai__state">
        {{ outcome.text || $t('ai.noResult') }}
      </p>

      <ul v-else class="ai__hits">
        <li v-for="hit in outcome.hits" :key="hit.rel">
          <NuxtLink :to="hitHref(hit)" class="ai__hit">
            <span class="ai__icon" aria-hidden="true">{{ hit.kind === 'folder' ? '📁' : '📄' }}</span>
            <span class="ai__main">
              <span class="ai__name">{{ basename(hit.rel) }}</span>
              <span class="ai__path">{{ hit.rel }}</span>
              <span class="ai__reason">{{ hit.reason }}</span>
            </span>
          </NuxtLink>
        </li>
      </ul>

      <p v-if="outcome.hits.length" class="ai__foot">
        {{ $t('ai.hops', { count: outcome.hops }) }}
      </p>
    </template>

    <!-- Rien encore demandé : on explique ce que la recherche sait faire. -->
    <p v-else class="ai__state">{{ $t('ai.hint') }}</p>
  </section>
</template>
