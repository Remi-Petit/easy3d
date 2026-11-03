<script lang="ts">
// Page d'accueil : dossiers puis fichiers à la racine — même structure et mêmes
// classes que `frontend/app/pages/index.vue`.
import { t } from '$lib/i18n'
import { urlApercu, extensionDe } from '$lib/api'
import CarteModele from '$lib/components/CarteModele.svelte'

let { data } = $props()

const catalogue = $derived(data.catalogue)
const dossiers = $derived(catalogue ? Object.values(catalogue.folders) : [])
const racine = $derived(catalogue?.files ?? [])

/** Recherche : mêmes règles que l'app Nuxt (nom pour un dossier, rel pour un fichier). */
let recherche = $state('')
const requete = $derived(recherche.trim().toLowerCase())

const dossiersFiltres = $derived(
  requete ? dossiers.filter((d) => d.name.toLowerCase().includes(requete)) : dossiers,
)
const racineFiltree = $derived(
  requete ? racine.filter((f) => f.rel.toLowerCase().includes(requete)) : racine,
)
const filtrer = $derived(requete.length > 0)
</script>

{#if !catalogue}
  <div class="empty">{$t('common.unknownError')}</div>
{:else}
  <div class="toolbar">
    <div class="search">
      <span class="icon">🔍</span>
      <input
        type="search"
        bind:value={recherche}
        placeholder={$t('catalog.title')}
        aria-label={$t('catalog.title')}
      />
    </div>
  </div>

  {#if filtrer}
    {@const total = dossiersFiltres.length + racineFiltree.length}
    <p class="section-label">{$t('catalog.all', { count: total })}</p>
    {#if total}
      <div class="file-grid">
        {#each dossiersFiltres as dossier (dossier.name)}
          <CarteModele dossier={dossier} mode={catalogue.config?.display?.mode ?? '3d'} />
        {/each}
        {#each racineFiltree as fichier (fichier.path)}
          <CarteModele fichier={fichier} mode={catalogue.config?.display?.mode ?? '3d'} />
        {/each}
      </div>
    {:else}
      <div class="empty">{$t('catalog.noResultFor', { what: `« ${recherche.trim()} »` })}</div>
    {/if}
  {:else}
    <p class="section-label">{$t('catalog.folders')}</p>
    {#if dossiers.length}
      <div class="file-grid">
        {#each dossiers as dossier (dossier.name)}
          <CarteModele dossier={dossier} mode={catalogue.config?.display?.mode ?? '3d'} />
        {/each}
      </div>
    {:else}
      <div class="empty">{$t('catalog.noFolder')}</div>
    {/if}

    <p class="section-label">{$t('catalog.root', { count: racine.length })}</p>
    {#if racine.length}
      <div class="file-grid">
        {#each racine as fichier (fichier.path)}
          <CarteModele fichier={fichier} mode={catalogue.config?.display?.mode ?? '3d'} />
        {/each}
      </div>
    {:else}
      <div class="empty">{$t('catalog.noRootFile')}</div>
    {/if}
  {/if}
{/if}
