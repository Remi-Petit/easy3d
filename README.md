# easy3d

[![release](https://img.shields.io/github/v/release/Remi-Petit/easy3d?sort=semver&label=release)](https://github.com/Remi-Petit/easy3d/releases/latest)
[![CI](https://github.com/Remi-Petit/easy3d/actions/workflows/ci.yml/badge.svg)](https://github.com/Remi-Petit/easy3d/actions/workflows/ci.yml)

Catalogue de modèles 3D **auto-hébergé** : un dossier de fichiers (STL, OBJ, 3MF,
G-code), une interface pour le parcourir, et un serveur MCP pour qu'un agent IA
puisse le lire — et le documenter.

## À quoi ça sert

easy3d affiche le contenu d'un dossier de modèles 3D : vignettes, métadonnées,
recherche et filtre par format, aperçu 3D interactif dans le navigateur. Chaque
fichier — et chaque dossier — peut porter une **note Markdown**, éditable dans
l'interface et **en temps réel** à plusieurs.

Le serveur surveille le dossier : déposer un fichier le fait apparaître, le
modifier met à jour son aperçu, le renommer emmène sa note. Rien à relancer.

## Ce qui le distingue

- **Un seul conteneur, une seule commande** : l'API Rust et l'interface Nuxt
  tiennent dans la même image, en deux process supervisés.
- **Pas de base de données.** Tes modèles restent des fichiers et tes notes des
  `.md` rangés à côté (`models/.easy3d-notes/`) : la source de vérité est ton
  dossier, lisible et modifiable sans l'outil.
- **Aperçus générés par le serveur, sans GPU** : STL / OBJ / 3MF rasterisés en
  CPU, vignette embarquée extraite des G-code. La grille reste légère même avec
  des centaines de fichiers.
- **Notes collaboratives** (CRDT Yjs) : deux personnes — ou une personne et un
  agent — écrivent la même note sans s'écraser.
- **Ajouter un format = un fichier** : `backend/src/formats/` isole chaque format
  (extensions, type MIME, aperçu, visionneuse) dans son module, et l'API annonce
  la liste au frontend, qui n'a aucune extension codée en dur.
- **Un serveur MCP intégré** : un agent liste les modèles, lit et écrit les notes,
  consulte et modifie la configuration — par le même chemin que l'interface, donc
  sans jamais écraser une édition en cours.
- <!-- langues:start -->**4 langues** : Français, English, Deutsch, Español — 133 clés, traduites à 100 %.<!-- langues:end -->
- **Testé** : tests Rust (analyse des formats, CRDT, outils et transport MCP),
  tests unitaires du frontend, tests de bout en bout Playwright — et une CI qui
  refuse le moindre avertissement du compilateur.

## Démarrage

`docker-compose.yml` :

```yaml
services:
  easy3d:
    image: ghcr.io/remi-petit/easy3d:latest
    container_name: easy3d
    restart: unless-stopped
    ports:
      - "3000:3000" # interface web : catalogue, aperçu 3D, notes
      - "8090:8090" # API HTTP + WebSocket temps réel + serveur MCP (/mcp)
    volumes:
      - ./models:/models # tes modèles : lus, et écrits (aperçus + notes)
      - easy3d-config:/config # configuration, réécrite par la page /admin

volumes:
  easy3d-config:
```

```bash
docker compose up -d
```

- Interface : <http://localhost:3000>
- API + MCP : <http://localhost:8090>

Tes modèles se déposent dans `./models` : le dossier est monté dans le conteneur,
il n'y a rien à importer.

## Serveur MCP (agents IA)

Le serveur MCP est **le backend lui-même** : rien à lancer en plus, il suffit que
le conteneur tourne.

```jsonc
// .vscode/mcp.json  (VS Code / Copilot)
{ "servers": { "easy3d": { "type": "http", "url": "http://localhost:8090/mcp" } } }
```

```bash
# Claude Code
claude mcp add --transport http easy3d http://localhost:8090/mcp

# Claude Desktop (HTTP non natif : passer par un proxy stdio)
npx mcp-remote http://localhost:8090/mcp
```

Un agent peut alors **lister** les modèles et les formats reconnus, **lire**,
**créer**, **modifier** ou **supprimer** une note, et **consulter la
configuration**. La suppression d'une note et le changement de dossier des
modèles sont désactivés par défaut : ajouter `EASY3D_MCP_ALLOW_WRITE: "1"` aux
variables d'environnement pour les autoriser.

À distance, `/mcp` est un endpoint du **backend** (port `8090`) : c'est à ton
reverse proxy de le router, séparément de l'interface — et de décider comment un
agent s'authentifie (le portail web, lui, attend un login navigateur).

L'API n'a pas d'authentification : garde-la sur `localhost`, ou ajoute un jeton
avant de l'exposer.

## IA

Ce projet a été développé **en binôme avec une IA** (GitHub Copilot, dans VS Code) :
l'architecture, le backend Rust, le frontend Nuxt, les tests, l'image Docker, la
CI et la chaîne de release ont été écrits au fil de la conversation. Les choix
ont été tranchés côté humain, et chaque étape validée par l'exécution réelle —
tests, conteneur lancé, workflows vérifiés — y compris les bugs que seul le
lancement révélait.

Pour développer : `cargo run` dans `backend/` et `bun run dev` dans `frontend/`
(tests : `cargo test`, `bun run test`, `bun run test:e2e`).
