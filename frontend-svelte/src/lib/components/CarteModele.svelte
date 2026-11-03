<script lang="ts">
// Carte de dossier ou de fichier — équivalent de `FolderCard.vue` / `FileItem.vue`.
//
// L'aperçu du dossier est **emprunté** à l'un de ses fichiers, comme dans l'app
// Nuxt : la première image disponible, sinon la première vignette générée.
import { urlApercu, extensionDe, nomDeBase } from '$lib/api'
import { t, langue } from '$lib/i18n'

let { dossier, fichier, mode = 'image' } = $props()

/** Aperçu : le fichier lui-même, ou celui qu'on emprunte au dossier. */
const apercu = $derived.by(() => {
  if (fichier) return fichier.image ? fichier : null
  if (!dossier) return null
  return dossier.files.find((f) => f.image) ?? null
})

const nom = $derived(dossier ? dossier.name : nomDeBase(fichier.rel))
const compte = $derived(dossier ? dossier.count : 1)
const quand = $derived(dossier ? dossier.modified : fichier?.modified ?? null)
const lien = $derived(dossier ? `/folders/${encodeURIComponent(dossier.name)}` : `/files/${fichier.rel}`)
const note = $derived(dossier ? dossier.note : fichier?.note)

/** « il y a 3 jours », dans la langue courante. */
function depuis(secondes: number | null, code: string): string {
  if (!secondes) return ''
  const ecart = Date.now() / 1000 - secondes
  const rtf = new Intl.RelativeTimeFormat(code || 'fr', { numeric: 'auto' })
  if (ecart < 3600) return rtf.format(-Math.round(ecart / 60), 'minute')
  if (ecart < 86400) return rtf.format(-Math.round(ecart / 3600), 'hour')
  return rtf.format(-Math.round(ecart / 86400), 'day')
}
</script>

<article class="model-card model-card--file">
  <a class="model-card__link" href={lien}>
    <div class="model-card__preview">
      {#if apercu?.image}
        <img
          class="model-card__img"
          src={urlApercu(apercu.image, apercu.image_version)}
          alt={nom}
          loading="lazy"
        />
      {:else}
        <div class="model-card__ph ph--other">
          {#if dossier}
            <span class="folder-icon">📁</span>
          {:else}
            <span class="ext">{extensionDe(fichier.rel).toUpperCase()}</span>
          {/if}
        </div>
      {/if}
      {#if dossier}<span class="folder-count">{dossier.count}</span>{/if}
      {#if note}<span class="note-badge" title={$t('folder.hasNote')}>📝</span>{/if}
    </div>
    <div class="model-card__body">
      <span class="model-card__name" title={nom}>{nom}</span>
      <span class="model-card__meta">
        {#if dossier}
          {$t('folder.meta', { count: compte, when: depuis(quand, $langue) })}
        {:else}
          {extensionDe(fichier.rel).toUpperCase()} · {depuis(quand, $langue)}
        {/if}
      </span>
    </div>
  </a>
</article>
