# easy3d

Catalogue de modèles 3D (STL / 3MF / GCODE) avec backend Rust et frontend Nuxt.

## Structure

```
easy3d/
├── backend/       ← API Rust (axum) : scan + watch de ./models
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── api.rs        ← routes HTTP (/models, /file, /note, /health)
│   │   ├── collab.rs     ← édition collaborative des notes (CRDT Yjs)
│   │   ├── notes.rs      ← notes Markdown (.easy3d-notes/)
│   │   ├── scanner.rs    ← scan récursif + métadonnées
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
- URL du backend configurable : `NUXT_HPCCAT_API_BASE` (défaut dérivé de
  `NUXT_HPCCAT_API_PORT`, sinon `http://127.0.0.1:8090`).

## Configuration

| Variable            | Backend / Frontend | Défaut                    |
|---------------------|--------------------|---------------------------|
| `PORT`              | backend            | `8090`                    |
| `MODELS_ROOT`       | backend            | `../models` (relatif repo)|
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
