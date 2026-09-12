# syntax=docker/dockerfile:1

# Image du backend easy3d : API HTTP + WebSocket + serveur MCP.
#
# Même base Debian pour les deux étapes (glibc compatible) ; si ce tag n'existe
# pas pour ta version, `rust:1.98-slim` + `debian:trixie-slim` fonctionne aussi,
# à condition de garder la même release Debian des deux côtés.

# ── Construction ───────────────────────────────────────────────────────────
FROM rust:1.98-slim-bookworm AS builder

WORKDIR /build

# Les dépendances pèsent bien plus lourd que le code et changent rarement : on
# les compile dans une couche à part, à partir d'une crate vide. Tant que
# Cargo.toml ne bouge pas, un changement de `src/` ne les recompile pas.
# (`Cargo.*` couvre Cargo.toml et Cargo.lock, ce dernier n'étant pas versionné.)
COPY Cargo.* ./
RUN mkdir -p src \
    && printf '' > src/lib.rs \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release

COPY src ./src
# `touch` : les vraies sources doivent paraître plus récentes que la crate vide.
RUN touch src/lib.rs src/main.rs && cargo build --release

# ── Exécution ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home easy3d

COPY --from=builder /build/target/release/easy3d /usr/local/bin/easy3d

# Dossier des modèles : à monter. Les aperçus (`.easy3d-thumbs/`) et les notes
# (`.easy3d-notes/`) y sont écrits — il doit donc être accessible en écriture.
ENV MODELS_ROOT=/models \
    PORT=8090

EXPOSE 8090
USER easy3d

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

ENTRYPOINT ["easy3d"]
