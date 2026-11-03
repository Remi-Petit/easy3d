// **Connexions WebSocket** : le catalogue temps réel (`/ws`).
//
//   docker compose --profile loadtest run --rm k6 run /scripts/ws.js
//   docker compose --profile loadtest run --rm -e WS_VUS=50 -e WS_HOLD=20 k6 run /scripts/ws.js
//
// Chaque connexion coûte un **scan complet du catalogue** avant le premier
// message (le backend envoie un snapshot à l'ouverture) : c'est ce que mesure ce
// scénario, pas le trafic de mise à jour — celui-ci n'arrive que si un fichier
// bouge pendant le test.
import http from 'k6/http';
import ws from 'k6/ws';
import { check } from 'k6';
import { BASE_URL, authHeaders, openSession } from './lib/api.js';

const WS_URL = (__ENV.WS_BASE || BASE_URL.replace(/^http/, 'ws')).replace(/\/+$/, '');
const HOLD_SECONDS = Number(__ENV.WS_HOLD || 10);

export const options = {
    scenarios: {
        sockets: {
            executor: 'constant-vus',
            vus: Number(__ENV.WS_VUS || 10),
            duration: __ENV.WS_DURATION || '30s',
        },
    },
    thresholds: {
        ws_connecting: ['p(95)<1000'],
        checks: ['rate>0.99'],
    },
};

export function setup() {
    return openSession();
}

/// Ticket frais pour **une** connexion.
///
/// Un ticket vit 60 s (`TICKET_SECONDS` côté backend) : le redemander à chaque
/// itération évite qu'un test de plusieurs minutes se mette à échouer au bout
/// de la première minute. Il n'est pas à usage unique, mais rien n'oblige à le
/// réutiliser.
function freshTicket(session) {
    if (!session.login) {
        return '';
    }
    return (
        http
            .post(`${BASE_URL}/auth/ws-ticket`, null, { headers: authHeaders(session) })
            .json('ticket') || ''
    );
}

export default function (session) {
    const ticket = freshTicket(session);
    const url = ticket ? `${WS_URL}/ws?ticket=${ticket}` : `${WS_URL}/ws`;
    let received = 0;

    const res = ws.connect(url, {}, (socket) => {
        socket.on('message', () => {
            received += 1;
        });
        socket.on('error', (e) => {
            // Remonté en erreur k6 plutôt qu'avalé : un socket qui casse doit
            // faire baisser `checks`, donc franchir le seuil.
            throw new Error(`WebSocket : ${e}`);
        });
        socket.setTimeout(() => socket.close(), HOLD_SECONDS * 1000);
    });

    check(res, {
        'connexion acceptée (101)': (r) => r && r.status === 101,
    });
    check(received, {
        'snapshot initial reçu': (n) => n >= 1,
    });
}
