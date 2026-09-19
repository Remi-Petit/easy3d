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
Pour certains paramètres, il y a des configurations via un fichier yml, mis à jour en temps réel via la page d'administration.
Seule exception : les partages de fichiers virtualisés (Docker Desktop sous Windows/macOS, montages réseau) ne remontent **aucun** événement. Le dossier est alors re-scanné de temps en temps — la page d'administration propose 5 s par défaut dans ce cas, et laisse la main (voir « Re-scan périodique »).
- **Aperçus générés par le serveur, sans GPU** : STL / OBJ / 3MF rasterisés en
  CPU, vignette embarquée extraite des G-code — et ramenée au style de la maison
  (matière claire sur fond sombre), comme les rendus de maillage. La grille reste
  légère même avec des centaines de fichiers.
- **Notes collaboratives** (CRDT Yjs) : Les utilisateurs peuvent ajouter des notes sur des dossiers / fichiers. Tout est stocké en markdown, pas de base de donnée. Ça a été conçu pour pouvoir fonctionner en temps réel. Via le serveur MCP, l'IA peut lire / ajouter / modifier des notes.
- **Un serveur MCP intégré** : un agent liste les modèles, cherche dans les notes
  et les métadonnées des G-codes, lit et écrit les notes, consulte et modifie la
  configuration par le même chemin que l'interface, donc sans jamais écraser une
  édition en cours.
- **Recherche assistée (IA), optionnelle** : on décrit ce que l'on cherche —
  « une pièce en PETG », « le boîtier de la carte » — et un modèle de langage
  fouille le catalogue : noms, dossiers, notes Markdown et métadonnées annoncées
  par le slicer dans les G-codes (matière, hauteur de couche, temps d'impression…).
  Il ne peut proposer que des fichiers **existants** : chaque chemin est vérifié
  côté serveur. Le fournisseur (OpenAI, Anthropic, Ollama…) et la clé se règlent
  dans l'administration ; tant que rien n'est configuré, le bouton reste grisé.
<!-- langues:start -->**4 langues** : Français, English, Deutsch, Español — 201 clés, traduites à 100 %.<!-- langues:end -->
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

## Recherche assistée (IA)

Désactivée par défaut : dans **Administration → Recherche assistée**, on choisit
un fournisseur (OpenAI, Anthropic, ou un Ollama local), son adresse éventuelle et
sa clé d'API. Le bouton **Tester** demande alors au fournisseur la liste des
modèles que cette clé ouvre, et le modèle se choisit dedans — pas de nom à
connaître par cœur, et la vérification de la clé est immédiate. La liste est
**conservée avec la configuration** : pour changer de modèle plus tard, il suffit
d'ouvrir l'administration, de choisir dans la liste et d'enregistrer, sans
redonner la clé. Rien n'est écrit avant le bouton **Enregistrer** : la clé ne part
qu'une fois, elle est écrite dans `config.yml` **côté serveur**, et elle n'est
jamais renvoyée à l'interface — elle y apparaît sous la forme `***`. Le bouton de
recherche ne s'active qu'une fois un fournisseur enregistré ; sinon il reste
grisé en expliquant où aller.

Une recherche est une petite conversation : le modèle interroge le catalogue par
quelques outils qui tournent **dans le backend** (vue d'ensemble, recherche par
nom, dossier, note ou métadonnée de G-code, lecture d'une note) et conclut par
une liste de propositions, chacune accompagnée de sa raison. Deux garde-fous :
la boucle est plafonnée à six étapes, et les chemins proposés sont confrontés au
catalogue réel — un fichier inventé est écarté.

Côté conteneur, rien de plus à installer : l'appel sortant se fait en HTTPS
(`rustls`), et il n'y a aucune dépendance à OpenSSL.

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
