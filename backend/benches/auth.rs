//! Bancs de l'authentification : ce que coûte **chaque** requête.
//!
//!     cargo bench --bench auth
//!
//! `resolve_session` est appelé par le middleware sur toute requête protégée :
//! c'est le seul travail que la charge fait payer à *tous* les endpoints, y
//! compris ceux qui ne lisent qu'un fichier (`/note`, `/print-info`). Le
//! mesurer sépare la faute du **contrôle d'accès** de celle du chemin des
//! données (scan du dossier, lecture disque).
//!
//! Contexte : à 1000 VU, tous les endpoints se dégradent également alors que le
//! CPU reste à ~1,1 cœur — d'où l'hypothèse d'un point de sérialisation. La base
//! des comptes est un **unique `rusqlite::Connection` derrière un `Mutex`**
//! ([`easy3d::auth::Auth::db`]), interrogée ici à chaque requête.
use axum::http::{HeaderMap, HeaderValue, header};
use criterion::{Criterion, criterion_group, criterion_main};
use easy3d::auth::{self, Auth, db};
use std::hint::black_box;

/// Prépare une base temporaire, un compte et sa session.
///
/// Le compte est créé **sans mot de passe** (comme un compte rattaché au SSO) :
/// la fixture n'a pas à payer un hachage argon2, seul le coût de la session est
/// mesuré.
fn banc() -> (tempfile::TempDir, Auth, HeaderMap) {
    let dir = tempfile::tempdir().unwrap();
    let auth = Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
    auth.db(db::ensure_builtin_roles).unwrap();

    let user = db::NewUser::new("banc", "banc@exemple.fr", None);
    auth.db(|conn| db::insert_user(conn, &user)).unwrap();
    let token = auth.start_session(&user.uuid, &HeaderMap::new()).unwrap();

    let mut headers = HeaderMap::new();
    let cookie = format!("{}={}", auth::COOKIE_NAME, token);
    headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());

    (dir, auth, headers)
}

fn session(c: &mut Criterion) {
    let (_dir, auth, headers) = banc();

    // Le chemin heureux : le cookie d'un onglet connecté, vérifié à chaque
    // requête. C'est le coût que paie le catalogue entier.
    c.bench_function("auth/resolve_session", |b| {
        b.iter(|| black_box(auth.resolve_session(black_box(&headers))))
    });

    // Le chemin négatif : un jeton que la base ne connaît pas (session fermée,
    // balayage). Il doit coûter autant que le bon — sinon la différence
    // renseignerait un attaquant sur l'existence d'un jeton.
    let mut inconnu = HeaderMap::new();
    let cookie = format!("{}=inconnu", auth::COOKIE_NAME);
    inconnu.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    c.bench_function("auth/resolve_session_inconnue", |b| {
        b.iter(|| black_box(auth.resolve_session(black_box(&inconnu))))
    });
}

criterion_group!(bancs, session);
criterion_main!(bancs);
