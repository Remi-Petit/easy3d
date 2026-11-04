# Keycloak — fournisseur d'identité **réel** (développement et tests)

Le compose de développement branche easy3d sur `fake-oidc`, un fournisseur
minimal qui **accepte tout sans mot de passe** : il prouve le câblage, pas la
compatibilité avec un vrai OIDC. Ce dossier apporte l'autre moitié : un
**Keycloak** complet, avec sa page de connexion, ses mots de passe et ses jetons,
pour exercer le SSO « en cas concret » sans compte chez un fournisseur.

Ce que Keycloak apporte ici qu'aucun fournisseur simulé ne peut donner :

- une **vraie** page de connexion (thème, messages d'erreur, langue) ;
- `userinfo` avec les revendications réelles de Keycloak (`sub`, `email`,
  `preferred_username`) — c'est ce que lit `auth::oidc::parse_identity` ;
- une **révocation** réelle (déconnexion, jeton périmé) que
  `auth::oidc::revalidate_sessions` doit savoir reconnaître.

Ce qu'il n'apporte pas : les **rôles**. easy3d ne lit pas les rôles du
fournisseur — l'identité vient du SSO, les droits restent dans le RBAC local
(c'est une décision du projet, voir `README.md`).

## Ce que contient le royaume

`realm.json` est **le** fichier de référence : il est importé au démarrage du
conteneur, et c'est lui que l'auto-test vérifie.

| Élément | Valeur |
|---|---|
| Royaume | `easy3d` |
| Émetteur | `<base>/realms/easy3d` |
| Client | `easy3d` (confidentiel, secret `secret-keycloak`) |
| Adresses de retour | `*` (développement : n'importe quel port local) |
| Rôles Keycloak | `admin`, `imprimeur` |
| Console d'administration | `<base>/admin`, compte `admin` / `admin` |

Cinq comptes, choisis pour montrer **qui décide des droits** :

| Compte | Mot de passe | Rôle chez Keycloak | Ce qu'il montre dans easy3d |
|---|---|---|---|
| `admin` | `motdepasse-admin` | `admin` | **rattache** l'administrateur local (même e-mail) : session admin |
| `user` | `motdepasse-user` | — | compte créé d'office, rôle livré `lecteur` |
| `imprimeuse` | `motdepasse-imprimeuse` | `imprimeur` | peut recevoir un rôle **local** (catalogue + envoi) |
| `observatrice` | `motdepasse-observatrice` | — | reste en lecture seule : la comparaison avec `imprimeuse` |
| `superuser` | `motdepasse-superuser` | `admin` | **aucun droit de plus** : le rôle Keycloak n'est pas lu |

⚠️ **Les rôles de ce tableau ne donnent aucun droit dans easy3d** — c'est une
décision du projet : le fournisseur dit **qui** vous êtes, easy3d dit **ce que
vous pouvez faire**. `superuser` porte le rôle `admin` chez Keycloak et n'ouvre
pourtant pas *Administration* ; `imprimeuse` ne devient imprimeuse que parce
qu'un administrateur lui a posé le rôle local correspondant dans
*Administration → Comptes*. La campagne e2e vérifie les deux sens.

`admin@example.com` est la **même adresse** que l'administrateur initial du
compose de développement : la première connexion SSO **rattache** ce compte
local au lieu d'en créer un second (mode `manual` de `auth::oidc::provision`).
Les autres n'existent pas côté easy3d : ils sont créés à la volée par le
provisionnement automatique, avec le rôle livré `lecteur`.

## Démarrage

Le service est derrière le profil Compose `keycloak` — `docker compose up` ne
tire donc pas 500 Mo d'image pour qui n'en a pas besoin.

```bash
docker compose --profile keycloak up -d keycloak      # 30-60 s la première fois
docker compose --profile keycloak logs -f keycloak    # ouvre /admin dans un navigateur
```

Le port publié est `8088` (`KEYCLOAK_PORT`), et l'adresse annoncée par défaut est
`http://host.docker.internal:8088` (`KEYCLOAK_URL`).

### Brancher l'application du compose dessus

```bash
EASY3D_OIDC_ISSUER=http://host.docker.internal:8088/realms/easy3d \
EASY3D_OIDC_CLIENT_ID=easy3d \
EASY3D_OIDC_CLIENT_SECRET=secret-keycloak \
docker compose --profile keycloak up --build
```

Puis <http://localhost:3100> → « Continuer avec host.docker.internal » →
`admin` / `motdepasse-admin` → on revient sur le catalogue, connecté en
administrateur, et `/admin/audit` montre une **Connexion SSO** « compte
préparé ».

### ⚠️ Une seule adresse : `KEYCLOAK_URL`

Keycloak annonce dans sa découverte **son** adresse (émetteur, autorisation,
jeton, `userinfo`), et le backend easy3d **refuse** un document dont l'émetteur
n'est pas exactement `EASY3D_OIDC_ISSUER` (garde-fou contre une redirection DNS
qui enverrait le secret client ailleurs). Les deux réglages doivent donc désigner
la même base, joignable **par le navigateur et par le backend** :

- dans un conteneur, `127.0.0.1` est le conteneur lui-même, pas l'hôte ;
- sur l'hôte, `host.docker.internal` est fourni par Docker Desktop (Docker
  l'écrit dans le fichier `hosts` de Windows/macOS, et le compose le déclare
  pour Linux).

D'où le défaut `http://host.docker.internal:8088`, qui marche des deux côtés.
Pour changer de port, changer **les deux** variables :

```bash
KEYCLOAK_PORT=9090 KEYCLOAK_URL=http://host.docker.internal:9090 \
EASY3D_OIDC_ISSUER=http://host.docker.internal:9090/realms/easy3d \
docker compose --profile keycloak up -d
```

Vérification en une ligne, si le navigateur affiche une erreur de connexion :

```bash
# doit afficher l'émetteur annoncé, tel qu'il est configuré côté easy3d
curl -s http://host.docker.internal:8088/realms/easy3d/.well-known/openid-configuration
```

Si le fichier `hosts` de Windows est **périmé** (l'adresse de la machine a changé
depuis le démarrage de Docker Desktop — l'entrée pointe alors vers une IP qui
n'est plus la sienne), le navigateur ne joint plus `host.docker.internal` alors
que les conteneurs, eux, continuent de le résoudre correctement. Redémarrer
Docker Desktop réécrit l'entrée.

## Rejouer le royaume depuis le fichier

Le royaume n'est importé que sur une base **vide**.

```bash
docker compose --profile keycloak down -v            # efface le royaume importé
docker compose --profile keycloak up -d keycloak     # réimporte realm.json
```

Une modification dans la console d'administration (`/admin`) est donc perdue au
prochain `down -v` — c'est voulu : `realm.json` reste la source de vérité, et
l'auto-test vérifie **ce fichier-là**.

## Tests

| Quoi | Commande | Ce que ça couvre |
|---|---|---|
| Auto-test du royaume | `node docker/keycloak/selftest.mjs` | découverte, comptes, `userinfo`, refus, rafraîchissement, révocation (sans navigateur, en CI) |
| Campagne e2e SSO réel | `cd frontend && bun run test:e2e:sso` | navigateur → Nuxt → backend → Keycloak, de bout en bout |

L'auto-test démarre lui-même son conteneur (projet Compose `easy3d-sso`, port
`18088`) et l'arrête à la fin ; `KEYCLOAK_KEEP=1` le laisse tourner,
`KEYCLOAK_URL=…` réutilise une instance existante.

La campagne e2e fait de même (`frontend/tests/e2e-sso/keycloak.ts`) : projet
`easy3d-sso`, Keycloak recréé à chaque exécution, arrêté à la fin. Elle tourne
le backend **sur l'hôte** (comme la campagne `e2e-auth`) et prend donc
`http://127.0.0.1:<port>` comme base — c'est la même contrainte d'adresse
unique, vue depuis l'hôte.

## Pannes typiques

| Symptôme | Cause |
|---|---|
| `Invalid parameter: redirect_uri` | l'adresse de retour n'est pas déclarée sur le client ; le royaume de dev accepte `*`, un royaume modifié à la main peut ne plus l'accepter |
| `login?error=invalid_state` | le conteneur a été recréé entre le clic et le retour, ou deux onglets ont démarré un flux avec le même état |
| `internal_error` après le retour, journal du backend « découverte OIDC : l'émetteur annoncé … n'est pas celui configuré » | `KEYCLOAK_URL` et `EASY3D_OIDC_ISSUER` ne désignent pas la même base (port, nom d'hôte) |
| Le bouton SSO n'apparaît pas | `EASY3D_OIDC_*` incomplet : le backend refuse un SSO à moitié configuré, et le dit au démarrage |
| Le conteneur ne démarre pas, `port is already allocated` | un autre Keycloak tourne (`docker ps`), ou `KEYCLOAK_PORT` est dans une plage réservée par Windows (`netsh interface ipv4 show excludedportrange protocol=tcp`) |
