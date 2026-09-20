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
  côté serveur. Le fournisseur (OpenAI, Anthropic, Ollama…), son adresse — avec
  un raccourci vers les services qui parlent le même protocole, tenu dans
  `ai-presets.yml` — et la clé se règlent dans l'administration ; tant que rien
  n'est configuré, le bouton reste grisé.
<!-- langues:start -->**4 langues** : Français, English, Deutsch, Español — 356 clés, traduites à 100 %.<!-- langues:end -->
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
      - ./config:/config # config.yml (écrit par /admin) + ai-presets.yml, éditables
```

```bash
mkdir -p config # un dossier à toi : sinon Docker le crée en root (Linux)
docker compose up -d
```

- Interface : <http://localhost:3000>
- API + MCP : <http://localhost:8090>

Tes modèles se déposent dans `./models` : le dossier est monté dans le conteneur,
il n'y a rien à importer. Ils apparaissent dans l'interface tout seuls — au plus
tard quelques secondes après, même déposés depuis l'explorateur.

`./config` contient les deux fichiers de réglages, à ouvrir dans ton éditeur :
`config.yml` (écrit par la page **Administration** — clé d'API comprise) et
`ai-presets.yml` (les adresses connues des fournisseurs d'IA, voir plus bas).
Les deux sont **relus à chaud**. Ces fichiers ne sont pas à versionner :
`config/` est ignoré par git (l'`ai-presets.yml` livré, lui, vit dans
`backend/` et part dans le binaire).

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

### Agents et comptes utilisateurs

Sans `EASY3D_AUTH`, `/mcp` est **ouvert** : garde-le sur `localhost`. Avec les
comptes activés, `/mcp` exige une identité, et un agent n'a pas de navigateur —
il présente donc un **jeton d'API** créé depuis **Mon compte** (`/compte`) :

```jsonc
// .vscode/mcp.json  (VS Code / Copilot)
{
  "servers": {
    "easy3d": {
      "type": "http",
      "url": "http://localhost:8090/mcp",
      "headers": { "Authorization": "Bearer e3d_…" }
    }
  }
}
```

```bash
# Claude Code
claude mcp add --transport http easy3d http://localhost:8090/mcp \
  --header "Authorization: Bearer e3d_…"
```

Un jeton **hérite des droits de son compte**, et chaque outil MCP réclame le droit
correspondant : un compte qui n'a que `catalog.read` peut lister et lire, mais
`get_config` lui répond `droit « config.read » requis pour l'outil « get_config »`.
Révoquer le jeton depuis la même page coupe l'agent immédiatement (401).

## Recherche assistée (IA)

Désactivée par défaut : dans **Administration → Recherche assistée**, on choisit
un fournisseur (OpenAI, Anthropic, ou un Ollama local), son adresse et sa clé
d'API. Les services qui parlent le même protocole sont proposés en un clic —
DeepSeek, OpenRouter, Groq, Mistral… pour OpenAI, ou l'Ollama **de la machine
hôte** quand easy3d tourne dans un conteneur — et le champ reste libre : un
proxy ou une adresse intermédiaire se saisit à la main.

Cette liste est un fichier à part, `ai-presets.yml`, posé à côté de `config.yml`
— donc `./config/ai-presets.yml` dans l'installation Docker, ouvrable dans ton
éditeur. Celui livré (`backend/ai-presets.yml`) est embarqué dans le binaire et
recopié là au premier démarrage s'il manque ; `EASY3D_PRESETS` permet de le poser
ailleurs. On y ajoute un service — une passerelle d'entreprise, un serveur
intermédiaire — sans recompiler, puisqu'il est **relu à chaud** comme la
configuration.

Le bouton **Tester** demande alors au fournisseur la liste des modèles que cette
clé ouvre, et le modèle se choisit dedans — pas de nom à connaître par cœur, et
la vérification de la clé est immédiate. La liste est
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

## Comptes utilisateurs (facultatif)

Désactivée par défaut : sans `EASY3D_AUTH`, l'application est ouverte et se
comporte exactement comme avant — aucune base n'est créée, aucun écran de
connexion n'apparaît. Pour l'activer :

```yaml
# docker-compose.yml
environment:
  EASY3D_AUTH: "on"
  EASY3D_ADMIN_EMAIL: vous@exemple.fr
  EASY3D_ADMIN_PASSWORD: "un mot de passe de 8 caractères minimum"
```

Ces deux dernières ne servent qu'à la **première** création de l'administrateur :
dès qu'un administrateur existe, elles sont ignorées et le mot de passe ne peut
plus être écrasé en redémarrant le conteneur (il se change dans l'interface).

| Réglage | Défaut | Rôle |
| --- | --- | --- |
| `EASY3D_AUTH` | *(éteint)* | active la gestion des comptes |
| `EASY3D_DB` | `config/easy3d.db` | base des comptes (SQLite) |
| `EASY3D_ADMIN_EMAIL` / `EASY3D_ADMIN_PASSWORD` | — | premier administrateur |
| `EASY3D_COOKIE_SECURE` | automatique | attribut `Secure` du cookie (posé d'office en HTTPS) |
| `EASY3D_PUBLIC_URL` | — | adresse publique de l'interface (adresse de retour du SSO) |
| `EASY3D_OIDC_ISSUER` / `_CLIENT_ID` / `_CLIENT_SECRET` | — | connexion par SSO (voir plus bas) |
| `EASY3D_OIDC_SCOPES` | `openid email profile` | portées demandées |
| `EASY3D_OIDC_PROVISIONING` | `auto` | `auto`, `manual` ou `approval` |

Les comptes vivent dans `config/easy3d.db`, **hors** du dossier des modèles — le
watcher le verrait comme un modèle. C'est de la **donnée** que l'application gère
(comptes, rôles, sessions) et non de la configuration qu'on édite : d'où SQLite
plutôt qu'un YAML, qui n'a ni transaction ni contrainte d'unicité.

### Rôles et droits

`admin` et `lecteur` sont livrés **figés** — on les clone pour partir de quelque
chose — et les rôles sur mesure se créent dans **Administration → Comptes**, avec
leurs droits cochés un par un :

| Domaine | Droits |
| --- | --- |
| Catalogue | voir et télécharger, ajouter des fichiers, renommer, supprimer, écrire les notes |
| Recherche assistée | lancer une recherche, choisir le fournisseur et sa clé |
| Réglages | voir, modifier |
| Comptes | voir les comptes, les modifier, voir les rôles, les modifier |

Les droits **s'additionnent** : ceux des rôles, plus ceux cochés directement sur
un compte. Le rôle `admin` est un superutilisateur — il a tout sans qu'on ait à le
lui accorder — et le dernier administrateur actif ne peut être ni désactivé, ni
supprimé. Le **rôle par défaut**, attribué aux comptes créés sans rôle, se choisit
dans le même écran — `lecteur` au premier démarrage.

Les mêmes droits s'appliquent **partout** : les pages de l'interface, l'API et
chaque outil du serveur MCP. Un agent muni d'un jeton ne peut donc rien faire de
plus que le compte qui l'a créé.

### Jetons d'API

Dans **Mon compte**, chacun crée ses propres jetons (pour un agent comme Claude
Code, un script ou une CI) et les révoque d'un clic. La valeur du jeton n'est
affichée **qu'une fois**, à sa création : le serveur n'en garde qu'une empreinte,
comme pour un mot de passe. Un jeton se présente ensuite en en-tête
`Authorization: Bearer e3d_…`, et sa révocation prend effet à l'appel suivant.

### Connexion par un fournisseur d'identité (SSO)

Un compte peut se connecter **par mot de passe ou par le fournisseur d'identité**
de l'organisation (Keycloak, Entra ID, Auth0, Google…) : les deux cohabitent, et
l'identité obtenue par le fournisseur ouvre exactement la même session et les
mêmes droits. Tout se règle dans **Administration → Fournisseur d'identité** — ou
par l'environnement, qui gagne alors sur `config.yml` et dont les champs
apparaissent grisés dans l'interface :

```yaml
# docker-compose.yml
environment:
  EASY3D_OIDC_ISSUER: https://sso.exemple.fr/realms/mon-service
  EASY3D_OIDC_CLIENT_ID: easy3d
  EASY3D_OIDC_CLIENT_SECRET: "le secret du client"
  EASY3D_PUBLIC_URL: https://catalogue.exemple.fr
```

Côté fournisseur, il n'y a qu'une chose à déclarer : l'**adresse de retour**
`<EASY3D_PUBLIC_URL>/api/auth/oidc/callback` (elle est rappelée dans l'interface).
La configuration est ensuite découverte à partir de l'émetteur
(`/.well-known/openid-configuration`), et le bouton **Tester** vérifie tout de
suite que l'adresse est la bonne — autrement, l'erreur n'apparaîtrait qu'au
premier utilisateur qui essaie de se connecter.

Ce qui se passe à la **première** connexion se choisit (réglage
`provisioning`) :

| Mode | Ce qui se passe |
| --- | --- |
| `auto` | Le compte est créé à la volée, avec le rôle par défaut. Simple — mais c'est l'annuaire du fournisseur qui décide qui entre. |
| `manual` | Seuls les comptes **déjà créés** entrent : l'administrateur les prépare ici (même adresse e-mail), et la première connexion les rattache. |
| `approval` | Le compte est créé **désactivé** : la page Comptes sert d'écran de validation. |

Le rattachement se **verrouille sur l'identifiant du fournisseur** (`sub`) dès la
première connexion : l'adresse e-mail peut ensuite changer chez le fournisseur
sans casser le compte, et une autre identité ne peut pas prendre sa place. Le
secret client n'est jamais renvoyé à l'interface (il y apparaît sous la forme
`***`), et l'échange de code se fait **côté serveur** — le navigateur ne voit
jamais ce secret. Le SSO ne remplace pas le mot de passe local : un compte peut
avoir les deux, ou aucun mot de passe du tout (créé par le SSO, ou préparé pour
lui).

### Session

Un cookie `HttpOnly` + `SameSite=Lax` (et `Secure` en HTTPS) porte un jeton
opaque ; se déconnecter ferme la session **côté serveur**, et changer de mot de
passe ferme toutes les autres. Cinq tentatives ratées bloquent un identifiant une
minute, et « identifiant inconnu » et « mot de passe faux » donnent le même
message — savoir lesquels existent est déjà une information.

Le catalogue et les notes circulent en WebSocket : le relais du frontend demande
alors au backend un **ticket court** au nom de la session, et ouvre la connexion
amont avec. Le ticket ne quitte jamais le serveur, et la reconnexion interne du
client de notes fonctionne sans rien changer côté navigateur.

## Précisions

Vous vous demandez peut-être où sont stockés les informations (images générées, notes markdown) : tout est ajouté dans des dossiers cachés à la racine de vos modèles, donc ça ne touche jamais directement à vos modèles et ce n'est pas visible. La seule base de données est celle des **comptes utilisateurs**, et elle n'existe que si vous activez `EASY3D_AUTH`.

| Contenu | Emplacement |
| --- | --- |
| Images | `models/.easy3d-thumbs` |
| Notes | `models/.easy3d-notes` |
| Comptes (si activés) | `config/easy3d.db` |


## Développement

Le projet est toujours en cours de développement et est récent. Si vous voyez des bugs ou autre, n'hésitez pas à créer une issue. Il s'agit de mon premier projet open source, soyez indulgent s'il vous plaît :p.

Pour développer : `cargo run` dans `backend/` et `bun run dev` dans `frontend/`
(tests : `cargo test`, `bun run test`, `bun run test:e2e`).
