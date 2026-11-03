// **Charge de lecture** : ce que fait l'interface quand on navigue, en boucle.
//
//   docker compose --profile loadtest run --rm k6 run /scripts/load.js
//   docker compose --profile loadtest run --rm -e VUS=50 -e HOLD=3m k6 run /scripts/load.js
//
// Montée en charge → plateau → descente. Les seuils sont des **critères
// d'échec** : k6 sort en 99 si l'un est dépassé, ce qui permet de l'utiliser
// comme test de non-régression (avant/après une optimisation).
//
// ⚠️ À lire avec `docker stats easy3d` ouvert dans un autre terminal : une
// latence stable avec un CPU à 100 % ne dit pas la même chose qu'à 30 %.
import http from 'k6/http';
import { check, sleep } from 'k6';
import { BASE_URL, authHeaders, fileUrl, modelPaths, noteUrl, openSession, pickLight, printInfoUrl } from './lib/api.js';

const VUS = Number(__ENV.VUS || 20);

export const options = {
    scenarios: {
        lecture: {
            executor: 'ramping-vus',
            startVUs: 0,
            stages: [
                { duration: __ENV.RAMP || '20s', target: VUS },
                { duration: __ENV.HOLD || '1m', target: VUS },
                { duration: '15s', target: 0 },
            ],
            gracefulRampDown: '10s',
        },
    },
    thresholds: {
        // Un échec isolé (fichier supprimé pendant le test) est toléré, pas 1 %.
        http_req_failed: ['rate<0.01'],
        'http_req_duration{name:GET /models}': ['p(95)<500'],
        'http_req_duration{name:GET /file}': ['p(95)<1000'],
        'http_req_duration{name:GET /note}': ['p(95)<300'],
        'http_req_duration{name:GET /print-info}': ['p(95)<1000'],
    },
    // Le tableau final : la médiane bouge peu, p(99) raconte les à-coups.
    summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

export function setup() {
    const session = openSession();
    const models = pickLight(modelPaths(session));
    // Les exports de slicer (G-code, 3MF) pèsent des dizaines de Mo : les
    // inclure mesurerait le disque, pas l'API. `ALLOW_HEAVY=1` les remet.
    const light = models.filter((rel) => !/\.(gcode|3mf)$/i.test(rel));
    return { session, models: light.length && !__ENV.ALLOW_HEAVY ? light : models };
}

export default function (data) {
    const headers = authHeaders(data.session);
    const rel = data.models.length ? data.models[Math.floor(Math.random() * data.models.length)] : null;

    // Répartition volontairement proche de l'usage réel : le catalogue est
    // demandé à chaque navigation (et rediffusé par le WebSocket), les fichiers
    // au fil des cartes, les notes et profils de slicer plus rarement.
    const roll = Math.random();
    if (roll < 0.4 || !rel) {
        check(http.get(`${BASE_URL}/models`, { headers, tags: { name: 'GET /models' } }), {
            '/models → 200': (r) => r.status === 200,
        });
    } else if (roll < 0.75) {
        check(http.get(fileUrl(rel), { headers, tags: { name: 'GET /file' } }), {
            '/file → 200': (r) => r.status === 200,
        });
    } else if (roll < 0.9) {
        check(http.get(noteUrl(rel), { headers, tags: { name: 'GET /note' } }), {
            '/note → 200': (r) => r.status === 200,
        });
    } else {
        check(http.get(printInfoUrl(rel), { headers, tags: { name: 'GET /print-info' } }), {
            '/print-info → 200': (r) => r.status === 200,
        });
    }

    // Petit temps de réflexion entre deux actions, comme un visiteur. `-e PAUSE=0`
    // le retire : on mesure alors la saturation, plus un usage réaliste.
    sleep(Number(__ENV.PAUSE || 0.2));
}
