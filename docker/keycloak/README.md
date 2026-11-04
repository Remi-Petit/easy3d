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

Keycloak fait partie du `docker compose up` par défaut : c'est le fournisseur
**principal** de l'application de développement. Le fournisseur fictif tourne en
même temps et s'affiche à côté (`EASY3D_OIDC_PROVIDERS`) : la page de connexion
propose donc **deux** boutons, et l'on choisit au moment de se connecter — pas au
moment de démarrer la pile.

```bash
docker compose up --build                             # application + Keycloak + fake-oidc
docker compose logs -f keycloak                       # le démarrage, puis le flux
```

Le port publié est `8088` (`KEYCLOAK_PORT`), et l'adresse annoncée par défaut est
`http://localhost:8088` (`KEYCLOAK_URL`).

### Brancher l'application dessus

C'est **déjà le cas** : le service `easy3d` annonce
`EASY3D_OIDC_ISSUER=http://localhost:8088/realms/easy3d`, et la page de
connexion propose « Continuer avec keycloak-dev ».

Puis <http://localhost:3100> → « Continuer avec keycloak-dev » → le compte
voulu → `motdepasse-…` (affiché sur la page) → on revient sur le catalogue,
connecté en tant que ce compte. `/admin/audit` montre une **Connexion SSO** dont
le détail dit si le compte a été créé ou rattaché.

⚠️ **Un compte déjà rattaché à un autre fournisseur ne peut pas l'être à
Keycloak.** Si la base de développement a servi avec `fake-oidc`, les comptes
`admin` et `user` y portent l'identité de l'ancien fournisseur : la connexion
Keycloak échoue alors en `oidc_conflict` (« déjà rattaché à une autre
identité »). Deux sorties : les détacher (*Administration → Comptes → Détacher
du SSO*, avec le mot de passe local `adminadmin`), ou repartir d'une base neuve
(`rm config/easy3d.db` puis redémarrer).

### Choisir son compte depuis l'application (`/dev/login`)

`NUXT_PUBLIC_DEV_LOGIN=1` (déjà posé par le compose) ouvre une **page de choix
des comptes** (<http://localhost:3100/dev/login>) : l'option SSO de la page de
connexion s'appelle alors « Continuer avec keycloak-dev » et mène à cette page, qui liste
le royaume — identifiant, adresse, rôles **Keycloak** et mot de passe à saisir.

Ce qu'elle fait, et ce qu'elle ne fait pas :

- elle **ne connecte personne** : chaque ligne démarre un vrai flux OIDC
  (`/api/auth/oidc/start?login_hint=…`), le fournisseur pré-remplit
  l'identifiant, et le mot de passe se saisit **chez lui** ;
- les rôles affichés sont ceux de Keycloak — ils **n'accordent rien** dans
  easy3d (voir plus haut) ; les droits se règlent dans *Administration →
  Comptes* ;
- la liste vient du fichier `realm.json` monté dans le conteneur
  (`./docker/keycloak:/dev/keycloak:ro`, lu via `NUXT_DEV_REALM`) : une seule
  liste, la même que celle importée par Keycloak ;
- sans le drapeau, la page **et** sa route répondent 404 (ce n'est pas un
  panneau caché, c'est du code de développement).

Côté développement « nu », sans Docker : `NUXT_PUBLIC_DEV_LOGIN=1` (ou la ligne
dans `frontend/.env`) suffit — le royaume est alors lu dans le dépôt
(`../docker/keycloak/realm.json`, relativement à `frontend/`).

### ⚠️ Une seule adresse : `KEYCLOAK_URL`

Keycloak annonce dans sa découverte **son** adresse (émetteur, autorisation,
jeton, `userinfo`), et le backend easy3d **refuse** un document dont l'émetteur
n'est pas exactement `EASY3D_OIDC_ISSUER` (garde-fou contre une redirection DNS
qui enverrait le secret client ailleurs). Les deux réglages doivent donc désigner
la même base, joignable **par le navigateur et par le backend**.

D'où le défaut `http://localhost:8088` : c'est le seul nom que le **navigateur**
résout toujours. Dans le conteneur, `localhost` désignerait le conteneur
lui-même — le service `easy3d` le fait donc pointer sur l'hôte :

```yaml
extra_hosts:
  - "localhost:host-gateway"
```

(rien dans l'image n'utilise le nom `localhost` pour elle-même : les deux
process se parlent en `127.0.0.1`, numérique).

⚠️ **Ne pas revenir à `host.docker.internal`** : Docker Desktop l'écrit dans le
fichier `hosts` de Windows au démarrage, mais ne le réécrit **pas** quand
l'adresse de la machine change (constaté : entrée `192.168.0.15` alors que la
machine était en `.14` — les conteneurs résolvaient, le navigateur non).

Pour changer de port, changer **les deux** variables :

```bash
KEYCLOAK_PORT=9090 KEYCLOAK_URL=http://localhost:9090 \
EASY3D_OIDC_ISSUER=http://localhost:9090/realms/easy3d \
docker compose up -d
```

Vérification en une ligne, si le navigateur affiche une erreur de connexion :

```bash
# doit afficher l'émetteur annoncé, tel qu'il est configuré côté easy3d
curl -s http://localhost:8088/realms/easy3d/.well-known/openid-configuration
```

## Rejouer le royaume depuis le fichier

Le royaume n'est importé que sur une base **vide**.

```bash
docker compose down -v            # efface le royaume importé
docker compose up -d keycloak     # réimporte realm.json
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
