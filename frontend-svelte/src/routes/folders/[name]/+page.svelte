<script lang="ts">
// Page d'un dossier : sous-dossiers puis fichiers — équivalent de
// `frontend/app/pages/folders/[name].vue`.
import { page } from '$app/state'
import type { Catalogue } from '$lib/api'
import { t } from '$lib/i18n'
import CarteModele from '$lib/components/CarteModele.svelte'

const catalogue = $derived(page.data.catalogue as Catalogue | null)
const nom = $derived(decodeURIComponent(page.params.name ?? ''))
const dossier = $derived(catalogue?.folders[nom])
const sousDossiers = $derived(
  (dossier?.subfolders ?? []).filter((s) => (s.rel.split('/').length ?? 0) === nom.split('/').length + 1),
)
</script>

{#if !dossier}
  <div class="empty">{$t('folder.notFound')}</div>
{:else}
  {@const mode = catalogue?.config?.display?.mode ?? '3d'}
  {#if sousDossiers.length}
    <p class="section-label">{$t('folder.subfolders', { count: sousDossiers.length })}</p>
    <div class="file-grid">
      {#each sousDossiers as sous (sous.rel)}
        <CarteModele
          dossier={{ ...sous, files: [] }}
          mode={mode}
        />
      {/each}
    </div>
  {/if}

  <p class="section-label">{$t('folder.files', { count: dossier.files.length })}</p>
  {#if dossier.files.length}
    <div class="file-grid">
      {#each dossier.files as fichier (fichier.path)}
        <CarteModele {fichier} {mode} />
      {/each}
    </div>
  {:else}
    <div class="empty">{$t('folder.empty')}</div>
  {/if}
{/if}
