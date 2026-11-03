// Helpers partagés par les scénarios k6 du banc de charge easy3d.
//
// Un scénario = un fichier à la racine de `docker/k6/` (`smoke.js`, `load.js`,
// `ws.js`), lancé par le service Compose `k6` (profil `loadtest`). Tous passent
// par ce module pour deux choses qu'il ne faut pas refaire à la main :
//
// 1. **l'authentification** : quand `EASY3D_AUTH=on`, l'API refuse tout sans
//    session. `/auth/login` vérifie un hachage argon2 (coûteux) et compte les
//    échecs pour verrouiller un compte martelé : on ouvre donc **une seule**
//    session, dans `setup()`, jamais à chaque itération. Marteler la connexion
//    ne mesurerait qu'argon2, pas le catalogue.
// 2. **les chemins d'exemple** : ils viennent du catalogue réel (`/models`).
//    Aucun chemin n'est écrit en dur, donc le banc fonctionne avec n'importe
//    quel dossier de modèles.
import http from 'k6/http';
import { fail } from 'k6';

/// Adresse de l'API **vue depuis le conteneur k6** : le nom de service Compose.
///
/// Les scénarios visent le backend directement. Le relais Nitro
/// (`http://easy3d:3000/api`) n'est pas un substitut : il ne relaie ni `/note`
/// ni `/ws` sous cette forme, donc `load.js` y verrait des 404.
export const BASE_URL = (__ENV.BASE_URL || 'http://easy3d:8090').replace(/\/+$/, '');

/// Identifiants du compte de test. Les valeurs par défaut sont celles que
/// `docker-compose.yml` donne à l'administrateur initial (`EASY3D_ADMIN_*`).
/// Sur une installation avec un autre compte : `-e EASY3D_LOGIN=… -e EASY3D_PASSWORD=…`.
const LOGIN = __ENV.EASY3D_LOGIN || 'admin';
const PASSWORD = __ENV.EASY3D_PASSWORD || 'adminadmin';

/// Ouvre une session et renvoie de quoi la rejouer sur chaque requête.
///
/// `{ cookie, ticket, login }` :
/// - `cookie` : en-tête `Cookie` complet (`easy3d_session=…`), vide quand les
///   comptes sont désactivés — l'API est alors ouverte, il n'y a rien à envoyer ;
/// - `ticket` : droit d'ouvrir un WebSocket, utilisable en `?ticket=` ;
/// - `login` : `true` quand une session a réellement été ouverte.
export function openSession() {
    const info = http.get(`${BASE_URL}/auth/me`);
    if (info.status !== 200) {
        fail(`/auth/me a répondu ${info.status} — l'API est-elle joignable sur ${BASE_URL} ?`);
    }
    if (!info.json('enabled')) {
        return { cookie: '', ticket: null, login: false };
    }

    const res = http.post(
        `${BASE_URL}/auth/login`,
        JSON.stringify({ login: LOGIN, password: PASSWORD }),
        { headers: { 'Content-Type': 'application/json' } },
    );
    if (res.status !== 200) {
        fail(`connexion refusée (${res.status}) — vérifier EASY3D_LOGIN / EASY3D_PASSWORD`);
    }
    const session = (res.cookies.easy3d_session || [])[0];
    if (!session) {
        fail('pas de cookie easy3d_session dans la réponse de /auth/login');
    }
    const cookie = `easy3d_session=${session.value}`;

    // Le relais Nitro ne peut pas transmettre le cookie à sa connexion amont :
    // il demande un ticket court (60 s) et le passe en `?ticket=`. Le ticket
    // n'est **pas** consommé à la première connexion (le client WebSocket doit
    // pouvoir se reconnecter), il expire simplement.
    const ticket = http
        .post(`${BASE_URL}/auth/ws-ticket`, null, { headers: { Cookie: cookie } })
        .json('ticket');

    return { cookie, ticket, login: true };
}

/// En-têtes de requête : le cookie de session, plus ce que le scénario ajoute.
export function authHeaders(session, extra) {
    const headers = Object.assign({ 'User-Agent': 'easy3d-k6' }, extra || {});
    if (session && session.cookie) {
        headers.Cookie = session.cookie;
    }
    return headers;
}

/// Chemins relatifs (`rel`) de tous les fichiers du catalogue.
export function modelPaths(session) {
    const res = http.get(`${BASE_URL}/models`, {
        headers: authHeaders(session),
        tags: { name: 'GET /models' },
    });
    if (res.status !== 200) {
        fail(`/models a répondu ${res.status}`);
    }
    const body = res.json();
    const rels = [];
    for (const file of body.files || []) {
        rels.push(file.rel);
    }
    for (const name of Object.keys(body.folders || {})) {
        // `FolderInfo.files` est **récursif** : les fichiers des sous-dossiers
        // y figurent aussi, inutile de descendre plus bas.
        for (const file of body.folders[name].files || []) {
            rels.push(file.rel);
        }
    }
    return rels;
}

/// Trie les fichiers pour préférer ceux dont la **lecture est courte**.
///
/// Un G-code ou un 3MF pèse des dizaines de mégaoctets : le lire mesure le débit
/// du disque et du réseau, pas le coût de l'API. Les maillages (STL/OBJ) passent
/// devant, les exports de slicer derrière.
export function pickLight(rels) {
    const rank = (rel) => {
        const lower = rel.toLowerCase();
        if (lower.endsWith('.stl') || lower.endsWith('.obj')) {
            return 0;
        }
        if (lower.endsWith('.gcode') || lower.endsWith('.3mf')) {
            return 2;
        }
        return 1;
    };
    return rels.slice().sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

export function fileUrl(rel) {
    return `${BASE_URL}/file?path=${encodeURIComponent(rel)}`;
}

export function noteUrl(rel) {
    return `${BASE_URL}/note?path=${encodeURIComponent(rel)}`;
}

export function printInfoUrl(rel) {
    return `${BASE_URL}/print-info?path=${encodeURIComponent(rel)}`;
}
