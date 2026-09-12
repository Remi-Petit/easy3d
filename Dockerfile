# syntax=docker/dockerfile:1

# Image **unique** d'easy3d : l'API Rust (HTTP + WebSocket + MCP) et le frontend
# Nuxt (SSR) tournent dans le même conteneur, supervisés par
# `scripts/docker-entrypoint.sh` — voir ce fichier pour le pourquoi.
#
#   docker compose -f docker-compose.yml up --build
#   → interface : http://localhost:3000
#   → API + MCP : http://localhost:8090   (MCP sur /mcp)

# ── 1. Backend (Rust) ──────────────────────────────────────────────────────
FROM rust:1.98-slim-bookworm AS backend

WORKDIR /build/backend

# Les dépendances changent rarement : on les compile seules, à partir d'une
# crate vide. Tant que `backend/Cargo.toml` ne bouge pas, modifier `backend/src/`
# ne les recompile pas. (`Cargo.*` tolère l'absence de `Cargo.lock`, qui n'est
# pas versionné dans ce dépôt.)
COPY backend/Cargo.* ./
RUN mkdir -p src \
    && printf '' > src/lib.rs \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release

COPY backend/src ./src
# `touch` : les vraies sources doivent paraître plus récentes que la crate vide.
RUN touch src/lib.rs src/main.rs && cargo build --release

# ── 2. Frontend (Nuxt) ─────────────────────────────────────────────────────
FROM oven/bun:1.3.14-slim AS frontend

WORKDIR /build/frontend

# Lockfile versionné : installation reproductible, et la couche reste valide
# tant que les dépendances ne changent pas.
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend/ ./
RUN bun run build

# ── 3. Exécution ───────────────────────────────────────────────────────────
# Base Node (Debian bookworm, comme la compilation du backend : glibc
# compatible) : elle fournit le runtime du serveur SSR et `debian:bookworm-slim`
# apporte `curl` pour le healthcheck.
FROM node:22-bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=backend /build/backend/target/release/easy3d /usr/local/bin/easy3d
COPY --from=frontend --chown=node:node /build/frontend/.output /app/.output
COPY scripts/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Deux process, deux ports — donc deux variables de port **distinctes** : le
# backend lit `PORT`, le serveur Nitro lit `NITRO_PORT` (prioritaire sur `PORT`,
# qu'il partage avec la plupart des applications Node).
ENV PORT=8090 \
    NITRO_PORT=3000 \
    NITRO_HOST=0.0.0.0 \
    MODELS_ROOT=/models \
    NUXT_HPCCAT_API_BASE=http://127.0.0.1:8090

# L'API doit écouter sur toutes les interfaces du conteneur : la publication de
# port (`-p 8090:8090`) ne relaie pas vers la boucle locale. Hors conteneur, la
# valeur par défaut reste 127.0.0.1 (l'API n'a pas d'authentification).
ENV EASY3D_HOST=0.0.0.0

# La configuration doit vivre dans un dossier **existant** : le watcher suit le
# dossier parent du fichier (voir `watch_dir`). Sans ça, le backend chercherait
# `CARGO_MANIFEST_DIR/config.yml`, chemin de compilation absent à l'exécution.
# Le fichier lui-même peut ne pas exister : les défauts s'appliquent, et la page
# `/admin` le crée au premier enregistrement.
ENV EASY3D_CONFIG=/config/config.yml

# Le WebSocket est ouvert par le **navigateur** : il doit viser l'hôte, pas le
# réseau interne du conteneur. À adapter (ou surcharger) si le catalogue n'est
# pas consulté depuis localhost.
ENV NUXT_PUBLIC_HPCCAT_WS_BASE=ws://localhost:8090

# Le dossier des modèles arrive par un volume : on l'écrit (aperçus `.easy3d-thumbs/`,
# notes `.easy3d-notes/`), tout comme `/config` que remplit la page `/admin`.
# Les créer ici avec le bon propriétaire initialise les volumes nommés en
# conséquence (Docker recopie contenu **et** droits du dossier de l'image).
RUN mkdir -p /models /config && chown node:node /models /config

USER node
EXPOSE 8090 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" >/dev/null \
        && curl -fsS "http://127.0.0.1:${NITRO_PORT}/" >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
