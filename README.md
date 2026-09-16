# easy3d

[![release](https://img.shields.io/github/v/release/Remi-Petit/easy3d?sort=semver&label=release)](https://github.com/Remi-Petit/easy3d/releases/latest)
[![CI](https://github.com/Remi-Petit/easy3d/actions/workflows/ci.yml/badge.svg)](https://github.com/Remi-Petit/easy3d/actions/workflows/ci.yml)
[![license](https://img.shields.io/github/license/Remi-Petit/easy3d)](LICENSE)

Une solution pour gérer ses modèles 3D de manière **auto-hébergé**.

## Contexte

Il existe déjà plusieurs projets open source permettant de gérer les modèles 3D. 
Avant de faire celui-ci, j'ai notamment utilisé [GyroidVault](https://github.com/TeeCodeDev/GyroidVault) et [STLVault](https://github.com/moddroid94/STLVault).

Il en existe d'autres, dont [Manyfold](https://github.com/manyfold3d/manyfold), mais je n'étais pas totalement satisfait de ces outils. Je voulais quelque chose de simple, qui ne consomme pas trop de ressources et s'intègre facilement à mon espace de travail.

J'ai vibecodé cet outil en faisant bien attention aux tests pour éviter les régressions. Je fais aussi attention à l'architecture du projet pour pouvoir ajouter des fonctionnalités sans trop de soucis. Je prévois de gérer pas mal de langues.

A l'heure actuelle, mon [Beszel](https://github.com/henrygd/beszel) affiche une consommation de 0.02% du CPU et 76.8 Mo de RAM.

## Ce qui le distingue

- **Un seul conteneur, une seule commande** : l'API Rust et l'interface Nuxt
  tiennent dans la même image, en deux process supervisés.
- **Pas de base de données.** Tout est gérer via des fichiers markdown. Le backend détecte les modifications provenant du dossier `models/` et met à jour l'app automatiquement. Il n'est donc pas obligatoire d'ajouter les fichiers depuis l'interface web, on peut très bien n'utiliser que le dossier en question.
Ce n'est pas un système de polling, ça utilise la librairie notify de rust permettant de récupérer l'information en temps réel.
L'avantage, c'est que vous ne dépendez pas de l'outil, c'est l'outil qui s'adapte à vous.
Seule exception : les partages de fichiers virtualisés (Docker Desktop sous Windows/macOS) ne remontent **aucun** événement. Le conteneur y ajoute donc un re-scan périodique (`EASY3D_WATCH_POLL`, 5 s dans le `docker-compose.yml` fourni), qui ne rediffuse que si le catalogue a réellement changé.
- **Aperçus générés par le serveur, sans GPU** : STL / OBJ / 3MF rasterisés en
  CPU, vignette embarquée extraite des G-code. La grille reste légère même avec
  des centaines de fichiers.
- **Notes collaboratives** (CRDT Yjs) : Les utilisateurs peuvent ajouter des notes sur des dossiers / fichiers. Tout est stocké en markdown, pas de base de donnée. Ça a été conçu pour pouvoir fonctionner en temps réel. Via le serveur MCP, l'IA peut lire / ajouter / modifier des notes.
- **Un serveur MCP intégré** : un agent liste les modèles, lit et écrit les notes,
  consulte et modifie la configuration par le même chemin que l'interface, donc
  sans jamais écraser une édition en cours.
<!-- langues:start -->**4 langues** : Français, English, Deutsch, Español — 148 clés, traduites à 100 %.<!-- langues:end -->
- **Testé** : tests Rust (analyse des formats, CRDT, outils et transport MCP),
  tests unitaires du frontend, tests de bout en bout Playwright et une CI qui
  refuse le moindre avertissement du compilateur.

## Démarrage

`docker-compose.yml` :

```yaml
services:
  easy3d:
    image: ghcr.io/remi-petit/easy3d:latest
    container_name: easy3d
    restart: unless-stopped
    environment:
      # Les partages de fichiers Docker Desktop ne remontent aucun événement :
      # ce re-scan rattrape les modèles ajoutés dans ./models sans passer par
      # l'interface. Sur un hôte Linux, les événements suffisent : mettre "0"
      # (le dossier monté y est un vrai système de fichiers).
      EASY3D_WATCH_POLL: "5"
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
il n'y a rien à importer. Ils apparaissent dans l'interface tout seuls — au plus
tard quelques secondes après, même déposés depuis l'explorateur.

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

## Précisions

Vous vous demandez peut-être où sont stockés les informations (images générées, notes markdown) vu qu'il n'y a pas de base de donnée. Tout est ajouté via des dossiers cachés à la racine de vos modèles. Ça ne touche donc jamais directement à vos modèles et ce n'est pas visible directement.

| Contenu | Emplacement |
| --- | --- |
| Images | `models/.easy3d-thumbs` |
| Notes | `models/.easy3d-notes` |


## Développement

Le projet est toujours en cours de développement et est récent. Si vous voyez des bugs ou autre, n'hésitez pas à créer une issue. Il s'agit de mon premier projet open source, soyez indulgent s'il vous plaît :p.

Pour développer : `cargo run` dans `backend/` et `bun run dev` dans `frontend/`
(tests : `cargo test`, `bun run test`, `bun run test:e2e`).
