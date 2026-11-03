# Banc de charge k6

[k6](https://github.com/grafana/k6) envoie du trafic HTTP/WebSocket à l'API et
rend un rapport de latence. Il tourne **dans le réseau Compose**, à côté du
serveur : aucun outil à installer sur le poste, et le trafic ne traverse ni la
machine hôte ni le proxy.

Ce n'est pas un *microbenchmark* : k6 mesure le **serveur sous charge**
(latence, débit, taux d'échec), pas le coût d'une fonction. Pour ça, un banc
`criterion` dans `backend/benches/` est le bon outil — les deux se complètent :
k6 dit « à 20 visiteurs, p95 = 300 ms », criterion dit « ce rasteriseur prend
12 ms pour 10 000 triangles ».

## Lancer

Le service est derrière un **profil** : `docker compose up` ne le démarre
jamais. **L'application doit tourner** au préalable (`docker compose up -d`) —
k6 ne la démarre pas, sinon un test de charge reconstruirait et redémarrerait
le serveur qu'il mesure. Toujours de la racine du dépôt :

```powershell
# 1. Le banc fonctionne-t-il ? (5 passages, aucun seuil de performance)
docker compose --profile loadtest run --rm k6 run /scripts/smoke.js

# 2. Charge de lecture : montée 0 → 20 VU, plateau d'une minute, descente
docker compose --profile loadtest run --rm k6 run /scripts/load.js

# 3. WebSocket : 10 connexions maintenues 10 s chacune
docker compose --profile loadtest run --rm k6 run /scripts/ws.js
```

`run` exécute k6 puis supprime le conteneur.
**Sortie attendue du `smoke`** : `checks 100.00%` et `http_req_failed 0.00%`.

## Le tableau de bord web

k6 a une interface de suivi en direct (`http://localhost:5665`) : courbes de
latence et de VU pendant que le test tourne, plus parlantes que le résumé final.
Le service l'arme par défaut, mais `docker compose run` ne publie **pas** les
ports : il faut `--service-ports`.

```powershell
docker compose --profile loadtest run --rm --service-ports k6 run /scripts/load.js
# puis http://localhost:5665 dans le navigateur
```

Le tableau de bord n'existe que **pendant** le test : à la fin, il affiche ses
dernières données, puis disparaît avec le conteneur. Pour garder une trace :

```powershell
docker compose --profile loadtest run --rm --service-ports `
  -e K6_WEB_DASHBOARD_EXPORT=/out/rapport.html `
  -v "${PWD}/docker/k6/reports:/out" `
  k6 run /scripts/load.js
# → docker/k6/reports/rapport.html, ouvrable hors ligne
```

⚠️ **Fermer l'onglet du tableau de bord avant la fin du test** : un onglet
resté ouvert garde ses connexions vivantes, et le processus k6 ne se termine
plus (observé : conteneur encore là 4 min après la fin du scénario, port 5665
occupé — le run suivant échoue sur `port is already allocated`). Sinon :
`docker rm -f easy3d-k6-run-…` avant de relancer.

Pour une mesure sans aucun outil en plus : `-e K6_WEB_DASHBOARD=false`.

## Les scénarios

| Fichier | Ce qu'il mesure |
|---|---|
| `smoke.js` | que le banc lui-même fonctionne (auth, chemins, routes) — **pas** de performance |
| `load.js` | lectures mêlées (`/models`, `/file`, `/note`, `/print-info`) sous charge |
| `ws.js` | connexions `/ws` : chacune déclenche un **scan complet** du catalogue |

Le réglage passe par l'environnement, jamais par une modification du fichier :

```powershell
# 50 visiteurs, plateau de 3 minutes
docker compose --profile loadtest run --rm -e VUS=50 -e HOLD=3m k6 run /scripts/load.js

# 100 sockets, 20 s chacun
docker compose --profile loadtest run --rm -e WS_VUS=100 -e WS_HOLD=20 k6 run /scripts/ws.js
```

| Variable | Défaut | Effet |
|---|---|---|
| `BASE_URL` | `http://easy3d:8090` | cible (nom de service Compose) |
| `EASY3D_LOGIN` / `EASY3D_PASSWORD` | `admin` / `adminadmin` | compte des scénarios (compte créé par le compose) ; inutile quand `EASY3D_AUTH` est éteint |
| `VUS` | `20` | visiteurs simultanés au plateau |
| `RAMP` / `HOLD` | `20s` / `1m` | montée / plateau |
| `PAUSE` | `0.2` | temps de réflexion entre deux requêtes (`0` = saturation maximale) |
| `ALLOW_HEAVY` | *(vide)* | `1` inclut les G-code et 3MF (dizaines de Mo : la mesure devient celle du disque) |
| `WS_VUS` / `WS_HOLD` / `WS_DURATION` | `10` / `10` / `30s` | connexions, durée de chacune, durée du scénario |

## Lire le rapport

```
     checks.........................: 100.00% ✓ 200     ✗ 0
     http_req_duration..............: avg=12.4ms min=1.2ms med=8.1ms p(90)=22ms p(95)=31ms p(99)=44ms max=61ms
       { name:GET /file }...........: avg=18.9ms ...
       { name:GET /models }.........: avg=9.6ms  ...
     http_req_failed................: 0.00%   ✓ 0       ✗ 0
     vus............................: 20      min=0     max=20
```

- **`checks`** — les assertions des scénarios (statut 200, `count` présent…).
- **`http_req_duration`** — par **endpoint** grâce aux `tags: { name: … }` : un
  p95 global cache toujours l'endpoint lent.
- **`http_req_failed`** — part des réponses en erreur (4xx/5xx et timeouts).
- **`ws_connecting`** — poignée de main des WebSocket.
- `p(99)` est celle qui raconte les à-coups ; la médiane bouge peu.

Les `thresholds` de chaque fichier sont des **critères d'échec** : si un p95
dépasse la limite, k6 sort en **99**. C'est ce qui rend le banc utilisable
comme test de non-régression — le code de sortie est la réponse, pas le tableau.

## Voir la consommation pendant un test

Dans un **autre** terminal, pendant que k6 tourne :

```powershell
docker stats easy3d
```

`docker stats` additionne les **deux** process du conteneur (l'API Rust et le
serveur Nuxt). Pour les séparer : `docker top easy3d` donne les PID, puis
`docker exec easy3d grep VmRSS /proc/<pid>/status`.

La lecture qui compte : latence **stable** à 30 % de CPU = marge ; latence qui
s'envole avec un CPU à 100 % = le serveur sature (ou attend le disque/argon2).

## Comparer avant / après une optimisation

1. même commande, mêmes `VUS`/`HOLD`/`PAUSE` (les valeurs par défaut conviennent) ;
2. relever `p(95)` par endpoint **et** le CPU moyen (`docker stats`) ;
3. un gain n'est retenu que si la latence baisse **ou** le CPU baisse à latence égale.

## Ce que ça ne couvre pas

- **Le coût de connexion** (`/auth/login` vérifie argon2 et verrouille un compte
  martelé) : la session est ouverte une fois dans `setup()`, pas à chaque
  itération. Un test de connexion à grande échelle aurait d'autres règles.
- **Le frontend** : JS, hydratation et rendu 3D se mesurent avec Lighthouse /
  DevTools, pas avec k6.
- **Le relais Nitro** : les scénarios visent le backend (`easy3d:8090`), qui est
  la source de la latence. Nitro ne relaie pas `/note` sous `/api`, donc le
  prendre pour cible ferait apparaître de faux 404.

## Ajouter un endpoint

Copier `smoke.js`, ajouter la requête avec un `tags: { name: 'GET /chemin' }`
(le nom sert de clé dans les seuils), puis relancer `smoke` pour valider.

## Mesurer une fonction (criterion)

k6 dit **où** ça coûte ; les bancs `criterion` de `backend/benches/` disent
**pourquoi**, dans le processus :

```powershell
cd backend
cargo bench --bench formats     # rendu, encodage PNG, aperçus STL/OBJ
cargo bench --bench catalogue   # scan du dossier + JSON de /models
cargo bench --bench auth        # contrôle d'accès (une requête = une vérification)
```

Ils écrivent leurs rapports dans `backend/target/criterion/` (ignoré par git).
Le premier passage sert de référence : c'est l'écart entre deux passages, à
code identique ou non, qui est l'information.
