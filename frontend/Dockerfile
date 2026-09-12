# syntax=docker/dockerfile:1

# Image du frontend easy3d : Nuxt 4 (SSR Nitro) construit par bun, servi par Node.

# ── Construction ───────────────────────────────────────────────────────────
FROM oven/bun:1.3.14-slim AS builder

WORKDIR /build

# Le lockfile est versionné : installation reproductible, et la couche reste
# valide tant que les dépendances ne changent pas.
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

COPY . .
RUN bun run build

# ── Exécution ──────────────────────────────────────────────────────────────
FROM node:22-slim AS runtime

ENV NODE_ENV=production \
    HOST=0.0.0.0 \
    PORT=3000

WORKDIR /app
# `.output` est autonome : serveur Nitro, client compilé et dépendances.
COPY --from=builder --chown=node:node /build/.output ./.output

USER node
EXPOSE 3000

# `GET /` ne dépend pas du backend : la santé du frontend ne doit pas se
# confondre avec celle de l'API (que reflète `/api/health`).
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD node -e "fetch('http://127.0.0.1:3000/').then((r) => process.exit(r.ok ? 0 : 1)).catch(() => process.exit(1))"

CMD ["node", ".output/server/index.mjs"]
