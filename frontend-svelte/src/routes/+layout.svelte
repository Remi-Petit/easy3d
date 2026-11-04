<script lang="ts">
// Coquille de l'application : sidebar (identité, navigation, état) + panneau de
// contenu avec son en-tête. Mêmes classes CSS que l'application Nuxt
// (`.app-sidebar`, `.topbar`, `.page`, `.brand`, `.status`…) — la feuille de
// style est la même, le rendu doit l'être aussi.
import { page } from '$app/state'
import { langue, langues, t, appliquerLangue } from '$lib/i18n'
import { nomDeBase } from '$lib/api'
import '../app.css'

let { data, children } = $props()

// Langue initiale : celle résolue côté serveur (cookie), comme l'app Nuxt — le
// HTML arrive donc déjà traduit, sans bascule à l'hydratation.
langue.set(data.langue)
$effect(() => appliquerLangue($langue))

/** Nom de dossier ou de fichier courant, pour le titre (dernier segment). */
const titre = $derived.by(() => {
  const params = page.params as Record<string, string | undefined>
  if (params.name) return nomDeBase(nomParametre(params.name))
  if (params.rel) return nomDeBase(nomParametre(params.rel))
  return $t('catalog.title')
})

/**
 * SvelteKit décode déjà les paramètres d'URL ; un second décodage planterait sur
 * un nom contenant un `%` littéral, d'où le repli.
 */
function nomParametre(valeur: string): string {
  try {
    return decodeURIComponent(valeur)
  } catch {
    return valeur
  }
}

/** Sur un dossier, le nombre de fichiers qu'il contient (comme l'app Nuxt). */
const sousTitre = $derived.by(() => {
  const params = page.params as Record<string, string | undefined>
  if (!params.name) return ''
  const dossier = data.catalogue?.folders[nomParametre(params.name)]
  return dossier ? $t('folder.subtitle', { count: dossier.files.length }) : ''
})

const surAccueil = $derived(page.url.pathname === '/')

/** État du backend, comme l'app Nuxt : temps réel ou hors ligne. */
const etat = $derived(data.catalogue ? 'online' : 'offline')
const libelleEtat = $derived(
  data.catalogue ? $t('status.live') : $t('status.offline'),
)

function choisirLangue(event: Event) {
  const code = (event.currentTarget as HTMLSelectElement).value
  langue.set(code)
  // Cookie un an, même nom que l'app Nuxt : les deux interfaces partagent le
  // choix de langue.
  document.cookie = `easy3d_lang=${code}; path=/; max-age=31536000; samesite=lax`
}
</script>

<div class="app-shell">
  <aside class="app-sidebar">
    <div class="brand">
      <div class="logo">3D</div>
      <div>
        <p class="brand__name">easy3d</p>
        <small>{$t('app.tagline')}</small>
      </div>
    </div>

    <nav class="nav">
      <a class="nav__item nav__item--on" href="/">{$t('nav.catalog')}</a>
    </nav>

    <div class="foot">
      <!-- Le choix de langue est un `<select>` natif : même cookie, même
           comportement que le sélecteur de l'app Nuxt, sans dépendance. -->
      <select
        class="lang"
        aria-label={$t('language.label')}
        value={$langue}
        onchange={choisirLangue}
      >
        {#each langues as l (l.code)}
          <option value={l.code}>{l.name}</option>
        {/each}
      </select>

      <div class="stats">
        <span class="status">
          <span class="dot {etat}"></span>
          {libelleEtat}
        </span>
        <span class="version">v{data.version}</span>
      </div>
    </div>
  </aside>

  <main class="panel">
    <header class="navbar">
      {#if !surAccueil}
        <a class="navbar__back" href="/" aria-label={$t('common.back')}>‹</a>
      {/if}
      <h1 class="navbar__title">{titre}</h1>
      {#if sousTitre}
        <small class="navbar__subtitle">{sousTitre}</small>
      {/if}
    </header>

    <div class="page">
      {@render children()}
    </div>
  </main>
</div>

