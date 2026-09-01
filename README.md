# easy3d

Catalogue de modèles 3D (STL / 3MF / GCODE) avec backend Rust et frontend Nuxt.

## Structure

```
easy3d/
├── backend/       ← API Rust (axum) : scan + watch de ./models
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── api.rs        ← routes HTTP (/models, /health)
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
  (contenu binaire d'un modèle) et `GET /health` sur `http://127.0.0.1:8090` (ou `PORT`).
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
- **Aperçu 3D** : chaque fichier STL / 3MF affiche une vignette 3D (three.js),
  cliquable pour ouvrir une vue plein écran (triangles + rotation/zoom).
  Les GCODE affichent un badge (pas de maillage).
- URL du backend configurable : `NUXT_HPCCAT_API_BASE` (défaut dérivé de
  `NUXT_HPCCAT_API_PORT`, sinon `http://127.0.0.1:8090`).

## Configuration

| Variable            | Backend / Frontend | Défaut                    |
|---------------------|--------------------|---------------------------|
| `PORT`              | backend            | `8090`                    |
| `MODELS_ROOT`       | backend            | `../models` (relatif repo)|
| `NUXT_HPCCAT_API_BASE` | frontend        | `http://127.0.0.1:8090`   |
| `NUXT_HPCCAT_API_PORT` | frontend        | `8090`                    |

## Tests

```bash
# backend
cd backend && cargo test

# frontend
cd frontend && bun run test
```
