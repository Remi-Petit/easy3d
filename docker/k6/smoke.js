// **Vérifie que le banc fonctionne** : 5 passages, aucun seuil de performance.
// À lancer après toute modification d'un script, d'une route ou d'un compte.
//
//   docker compose --profile loadtest run --rm k6 run /scripts/smoke.js
//
// Sortie attendue : `checks ✓ 20/20` et `http_req_failed: 0.00%`. Un seuil en
// échec fait sortir k6 en **99** (les autres erreurs : 100 à 110).
import http from 'k6/http';
import { check } from 'k6';
import { BASE_URL, authHeaders, fileUrl, modelPaths, noteUrl, openSession, pickLight, printInfoUrl } from './lib/api.js';

export const options = {
    vus: 1,
    iterations: 5,
    thresholds: {
        checks: ['rate==1'],
        http_req_failed: ['rate==0'],
    },
};

export function setup() {
    const session = openSession();
    return { session, models: pickLight(modelPaths(session)) };
}

export default function (data) {
    const headers = authHeaders(data.session);

    const list = http.get(`${BASE_URL}/models`, {
        headers,
        tags: { name: 'GET /models' },
    });
    check(list, {
        '/models → 200': (r) => r.status === 200,
        '/models → count est un nombre': (r) => typeof r.json('count') === 'number',
    });

    const rel = data.models.length ? data.models[__ITER % data.models.length] : null;
    if (!rel) {
        console.warn('catalogue vide : /file, /note et /print-info ne sont pas exercés');
        return;
    }

    check(http.get(fileUrl(rel), { headers, tags: { name: 'GET /file' } }), {
        '/file → 200': (r) => r.status === 200,
    });
    check(http.get(noteUrl(rel), { headers, tags: { name: 'GET /note' } }), {
        '/note → 200': (r) => r.status === 200,
    });
    // `{}` pour un STL : c'est une réponse valide (« rien à annoncer »).
    check(http.get(printInfoUrl(rel), { headers, tags: { name: 'GET /print-info' } }), {
        '/print-info → 200': (r) => r.status === 200,
    });
}
