<script lang="ts">
// Page d'un fichier : grand aperçu, métadonnées, téléchargement — équivalent de
// `frontend/app/pages/files/[rel].vue`.
import { page } from '$app/state'
import { urlApercu, urlFichier, extensionDe, nomDeBase, type Catalogue } from '$lib/api'
import { t } from '$lib/i18n'

const catalogue = $derived(page.data.catalogue as Catalogue | null)
const rel = $derived(decodeURIComponent(page.params.rel ?? ''))

/** Le fichier, qu'il soit à la racine ou dans un dossier. */
const fichier = $derived.by(() => {
  if (!catalogue) return null
  const direct = catalogue.files.find((f) => f.rel === rel)
  if (direct) return direct
  for (const dossier of Object.values(catalogue.folders)) {
    const trouve = dossier.files.find((f) => f.rel === rel)
    if (trouve) return trouve
  }
  return null
})
</script>

{#if !fichier}
  <div class="empty">{$t('common.unknownError')}</div>
{:else}
  <article class="viewer">
    <div class="viewer__stage">
      {#if fichier.image}
        <img
          class="viewer__img"
          src={urlApercu(fichier.image, fichier.image_version)}
          alt={nomDeBase(fichier.rel)}
        />
      {:else}
        <div class="model-card__ph ph--other">
          <span class="ext">{extensionDe(fichier.rel).toUpperCase()}</span>
        </div>
      {/if}
    </div>

    <aside class="viewer__side">
      <h2 class="viewer__title">{nomDeBase(fichier.rel)}</h2>
      <p class="model-card__meta">
        {extensionDe(fichier.rel).toUpperCase()} · {fichier.rel}
      </p>
      {#if fichier.note}
        <p class="viewer__note">{fichier.note}</p>
      {/if}
      <a class="button" href={urlFichier(fichier.rel)} download>
        {$t('common.download')}
      </a>
    </aside>
  </article>
{/if}
