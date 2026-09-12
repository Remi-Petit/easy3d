# easy3d

Catalogue de modèles 3D (STL / 3MF / GCODE) avec backend Rust et frontend Nuxt.

## Structure

```
easy3d/
├── backend/       ← API Rust (axum) : scan + watch de ./models
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── api.rs        ← routes HTTP (/models, /file, /note, /health, /mcp)
│   │   ├── mcp.rs        ← serveur MCP : outils pour les agents
│   │   ├── collab.rs     ← édition collaborative des notes (CRDT Yjs)
│   │   ├── config.rs     ← configuration YAML (rechargée à chaud)
│   │   ├── formats/      ← un fichier par format (stl, obj, three_mf, gcode)
│   │   ├── notes.rs      ← notes Markdown (.easy3d-notes/)
│   │   ├── scanner.rs    ← scan récursif + métadonnées
│   │   ├── thumbnail.rs  ← aperçus PNG (.easy3d-thumbs/)
│   │   └── watcher.rs    ← description des événements FS
│   └── tests/
├── frontend/      ← app Nuxt (bun) : UI catalogue + proxy API
│   ├── app/
│   │   ├── pages/
│   │   ├── components/
│   │   ├── composables/
│   │   └── assets/css/
│   ├── server/api/       ← proxy vers le backend Rust
│   └── nuxt.config.ts
├── models/        ← données 3D (STL / 3MF / GCODE), partagées
├── .gitignore
└── README.md
```

## Prérequis

- [Rust](https://rustup.rs/) (toolchain stable)
- [bun](https://bun.sh/) pour le frontend

## Backend

```bash
cd backend
cargo run
```

- Sert `GET /models` (liste structurée avec `path` et `rel`), `GET /file?path=<rel>`
  (contenu binaire d'un modèle), `GET /note?path=<rel>` (note Markdown) et
  `GET /health` sur `http://127.0.0.1:8090` (ou `PORT`).
- `GET /collab/{rel}` est un WebSocket d'**édition collaborative** : chaque note est
  un document CRDT Yjs (`yrs`), le serveur en tient la version autoritaire et relaie
  les modifications entre participants. Les notes sont persistées en Markdown dans
  `models/.easy3d-notes/`, en miroir de l'arborescence (`DemaAuto/x.stl` →
  `.easy3d-notes/DemaAuto/x.stl.md`). Le dossier est ignoré par le scan et par la
  surveillance, qui l'écrit lui-même.
- Chaque note a un **compagnon binaire** `<rel>.ydoc` (état CRDT) à côté du `.md`.
  Il permet au serveur de repartir sur le **même** document Yjs après un redémarrage :
  sans lui, il ré-amorçait un document neuf, et les insertions encore détenues par les
  clients connectés se cumulaient avec les siennes (texte en double).
- Le champ `rel` est le chemin relatif à la racine, utilisé par le frontend pour `/file`.
- Sécurité : `/file` rejette la traversée de dossier (`..`, chemins absolus).
- Surveille `./models` (via `MODELS_ROOT`, relatif au repo par défaut).
- `.env` : `PORT=60005`.

## MCP (agents)

Le backend expose un **serveur MCP** sur `http://127.0.0.1:8090/mcp` (transport
*Streamable HTTP*, monté dans le même process axum : `cargo run` suffit).

Les outils partagent l'état vivant du serveur — le catalogue courant, la
configuration et les documents collaboratifs — donc les modifications d'un agent
sont visibles dans l'interface sans redémarrage, et inversement.

| Outil | Rôle |
|-------|------|
| `list_formats` | formats reconnus (extensions, aperçu, visionneuse) |
| `list_models` | catalogue, filtrable par nom / dossier / extension |
| `get_model` | détail d'une entrée (format, aperçu, note, taille, dates) |
| `list_notes` | notes existantes + nombre d'entrées à documenter |
| `read_note` | contenu d'une note |
| `create_note` / `update_note` / `append_note` | écriture d'une note |
| `get_config` | dossier des modèles, mode d'affichage, chemin du YAML |
| `set_display_mode` | bascule `3d` / `image` |
| `delete_note` | ⚠️ suppression d'une note |
| `set_models_root` | ⚠️ changement de dossier des modèles |

Points à connaître :

- Les notes s'écrivent **via le CRDT**, jamais en écrivant le `.md` : si un onglet
a la note ouverte, les deux modifications fusionnent au lieu de s'écraser, et la
persistance (`.md` + `.ydoc`) suit le flush habituel. Une note ne peut être créée
que sur un élément **existant** du catalogue (fichier ou dossier).
- Les outils ⚠️ (destructeurs) sont **désactivés** par défaut :
`EASY3D_MCP_ALLOW_WRITE=1 cargo run` pour les activer.
- Un changement de dossier des modèles écrit la configuration ; c'est le watcher
qui rebascule la surveillance (quelques dizaines de ms plus tard). `get_config`
distingue le dossier configuré, le dossier résolu et celui réellement surveillé.

Côté client, il n'y a **rien à lancer** : le serveur MCP est le backend lui-même.

```jsonc
// .vscode/mcp.json  (VS Code / Copilot)
{ "servers": { "easy3d": { "type": "http", "url": "http://127.0.0.1:8090/mcp" } } }
```

```bash
# Claude Code
claude mcp add --transport http easy3d http://127.0.0.1:8090/mcp

# Claude Desktop (HTTP non natif : passer par un proxy stdio)
npx mcp-remote http://127.0.0.1:8090/mcp
```

L'API n'a **aucune authentification** : le MCP s'adresse à `127.0.0.1`. Ne
l'exposez pas au-delà (bind public, tunnel) sans ajouter au minimum un jeton
d'API — sinon `set_models_root` et `delete_note` sont ouverts à qui peut joindre
le port.

## Frontend

```bash
cd frontend
bun install
bun run dev
```

- UI sur `http://localhost:3000`.
- Les routes `/api/models`, `/api/file` et `/api/health` proxyent vers le backend.
- **Notes** : chaque dossier et chaque fichier peut porter une note Markdown, éditable
  sans écrire de Markdown (barre d'outils) et **en temps réel** : plusieurs sessions
  peuvent écrire en même temps, à des endroits différents, sans s'écraser. Le texte
  affiché suit les modifications des autres participants en direct.
- **Aperçu 3D** : chaque fichier STL / 3MF affiche une vignette 3D (three.js),
  cliquable pour ouvrir une vue plein écran (triangles + rotation/zoom).
  Les GCODE affichent un badge (pas de maillage).
- **Téléchargement** : la page détail d'un fichier propose un bouton
  « Télécharger ». Il pointe vers `/api/file?path=<rel>&download=1`, la même
  route que l'aperçu ; le paramètre `download` fait répondre le proxy en
  `Content-Disposition: attachment` (nom de fichier conservé, accents compris).
- **Mode d'affichage** (`display.mode` dans `config.yml`, réglable sur `/admin`) :
  `3d` charge un viewer three.js sur les cartes, `image` préfère l'aperçu
  statique extrait du fichier. La page **détail** fait exception : un modèle ou
  un G-code y est toujours rendu en 3D, quel que soit le mode.
- **Langues** : interface en français, anglais, allemand et espagnol, via
  `@nuxtjs/i18n`. La langue vit dans un cookie (`easy3d_lang`), pas dans l'URL
  (stratégie `no_prefix`) : les liens internes et les routes `/fichier/<rel>`
  restent identiques. Messages dans `frontend/i18n/locales/*.json`, `fr` faisant
  office de référence. Le sélecteur est dans le pied de la sidebar.
- URL du backend configurable : `NUXT_HPCCAT_API_BASE` (défaut dérivé de
  `NUXT_HPCCAT_API_PORT`, sinon `http://127.0.0.1:8090`).

## Internationalisation

- Messages : `frontend/i18n/locales/<code>.json` (`fr`, `en`, `de`, `es`).
  `fr` est la **référence** : toute clé qu'il déclare doit exister ailleurs.
- `tests/unit/i18n.test.ts` est **bloquant** : listes de clés identiques d'une
  langue à l'autre, mêmes variables (`{count}`, `{when}`…) dans chaque message,
  aucune valeur vide, et toute clé utilisée dans le code doit exister.
- Ce qui n'est **pas** dans les fichiers de langue, car ce n'est pas de la
  traduction mais du formatage : nombres et temps relatifs. `n()` (vue-i18n)
  remplace `toLocaleString('fr-FR')` du viewer, et `useTimeAgo` (au-dessus de
  `utils/format#relativeTime`, pur et testé) rend « il y a 3h » / « 3h ago ».
- Les noms de fichiers et de dossiers ne sont jamais traduits.
- Deux composants tiers ont leur propre i18n, branchée sur la langue courante :
  `UApp :locale` (paquets de `@nuxt/ui/locale`) et md-editor-v3
  (`notes.editor` des fichiers de langue, lu en `?raw` — voir le commentaire
  dans `NotePanel.client.vue`).

## Configuration

| Variable            | Backend / Frontend | Défaut                    |
|---------------------|--------------------|---------------------------|
| `PORT`              | backend            | `8090`                    |
| `MODELS_ROOT`       | backend            | `../models` (relatif repo)|
| `EASY3D_CONFIG`     | backend            | `backend/config.yml`      |
| `EASY3D_MCP_ALLOW_WRITE` | backend       | *désactivé* (`1` = outils destructeurs) |
| `NUXT_HPCCAT_API_BASE` | frontend        | `http://127.0.0.1:8090`   |
| `NUXT_HPCCAT_API_PORT` | frontend        | `8090`                    |
| `NUXT_HPCCAT_WS_BASE`  | frontend        | `ws://127.0.0.1:8090`     |

## Tests

```bash
# backend
cd backend && cargo test

# frontend
cd frontend && bun run test
```

- `backend/src/mcp.rs` teste les **outils** MCP (catalogue, CRUD des notes via le
  CRDT, gating des outils destructeurs, configuration).
- `backend/tests/mcp_http.rs` teste le **transport** MCP à travers le Router axum :
  handshake `initialize`, en-tête de session, `tools/list` (schémas d'entrée),
  `tools/call` en lecture et en écriture, refus sans session.
