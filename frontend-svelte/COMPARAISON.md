# Nuxt → SvelteKit : ce que ça change en consommation

Branche de **test**, pas une migration : l'application Nuxt reste la référence.
Le portage couvre le **catalogue** (accueil, dossier, fichier), avec la même
feuille de style (`src/app.css` est une copie de `frontend/app/assets/css/main.css`,
à l'import de `@nuxt/ui` près), les **mêmes fichiers de messages** que l'app Nuxt
(`../frontend/i18n/locales/*.json`), et le même relais WebSocket (dans `server.js`,
puisque SvelteKit ne sait pas monter un WebSocket dans une route).

## ⚠️ Ce qui n'est **pas** porté

`/admin`, `/account`, `/login`, les comptes, les notes collaboratives, l'envoi,
le renommage, la suppression, la recherche IA — soit l'essentiel du code
applicatif (les pages d'administration pèsent à elles seules ~100 Ko de Vue).
Les chiffres de **débit** ci-dessous ne sont donc pas une comparaison de
frameworks à fonctionnalités égales : le portage fait beaucoup moins de choses.
La **mémoire au repos** et le **coût par page rendue** sont, eux, comparables.

## Mesures

Même machine, même image de base (`node:22-bookworm-slim`), même backend Rust,
`NODE_ENV=production`, banc k6 `docker/k6` (20 utilisateurs virtuels, 20 s, page
`/`), processus Node mesuré seul via `/proc`.

### Au repos (redémarrage, aucune requête)

| | Nuxt 4 | SvelteKit | |
|---|---|---|---|
| RSS du process Node | 79,1 Mo | **76,6 Mo** | −3 % |
| Mémoire du conteneur | 29,7 Mio | **26,0 Mio** | −13 % |
| CPU (60 s, ticks) | 0,017 % | **0 %** | |

### Sous charge (20 VU, 20 s, page d'accueil)

| | Nuxt 4 | SvelteKit | |
|---|---|---|---|
| requêtes servies | 5 102 | **38 314** | ×7,5 |
| débit | 266 req/s | **2 002 req/s** | ×7,5 |
| latence moyenne | 78,2 ms | **10,4 ms** | ÷7,5 |
| latence p95 | 110,4 ms | **13,7 ms** | ÷8 |
| RSS du process, avant → après | 114 → **317 Mo** | 101 → **202 Mo** | −36 % |
| CPU du process (1 cœur saturé dans les deux cas) | 103,6 % | 95,6 % | |

### Ce que reçoit le navigateur (même session, même catalogue)

| | Nuxt 4 | SvelteKit | |
|---|---|---|---|
| HTML de la page d'accueil (connecté) | 32 198 o | 24 644 o | |
| cartes déjà dans le HTML | 0 (rendu côté client) | 72 (rendu côté serveur) | |
| HTML sans session | 18 892 o | 3 090 o | |

## Ce qu'il faut en retenir

1. **Au repos, aucun gain notable** : ~76 Mo contre ~79 Mo. L'essentiel de la
   mémoire d'un serveur SSR Node est le runtime et le tas V8, pas le framework.
   Passer à SvelteKit ne réduit donc **pas** la consommation d'un serveur qui ne
   sert presque personne — c'était l'espoir de départ, la mesure le dément.
2. **Sous charge, l'écart est net** : ~7× plus de pages servies par cœur, et une
   mémoire qui monte deux fois moins haut. Une partie vient du framework, une
   partie du portage réduit : il faudrait porter le reste pour trancher.
3. **Le rendu diffère** : l'app Nuxt livre une coquille et hydrate (le catalogue
   arrive en JSON), le portage rend le catalogue **côté serveur**. C'est un choix
   de ce portage, pas une contrainte de SvelteKit.
4. Le relais WebSocket demande un serveur HTTP maison (`server.js`) : c'est du
   code en plus à maintenir, que Nitro fournissait.

## Reproduire

```bash
cd frontend-svelte
bun install && bun run build
docker build -t easy3d-svelte:test .
docker run -d --name svelte-test --network easy3d_default \
  -e NUXT_HPCCAT_API_BASE=http://easy3d:8090 -p 3400:3000 easy3d-svelte:test
# puis, depuis la racine : le banc k6 avec BASE_URL=http://svelte-test:3000
```
