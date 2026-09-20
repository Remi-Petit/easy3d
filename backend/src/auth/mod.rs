//! Comptes utilisateurs : base SQLite, mots de passe, sessions, connexion.
//!
//! # Ce que fait ce module (étape 1)
//!
//! Authentification **facultative** : elle ne s'active qu'avec `EASY3D_AUTH`
//! (voir [`Auth::from_env`]). Tant que la variable est absente, [`Auth::disabled`]
//! est en place, le middleware [`require_auth`] laisse tout passer et le
//! comportement du serveur est **exactement** celui d'avant : c'est ce qui permet
//! à la suite de tests existante de ne pas bouger.
//!
//! Le RBAC (rôles, droits, jetons d'API, OIDC) vient ensuite : les tables
//! existent déjà (`auth::db`), l'objectif de cette étape est qu'une connexion
//! fonctionne et qu'elle soit révocable.
//!
//! # Choix à connaître avant de toucher au module
//!
//! - **Le cookie porte un jeton opaque**, pas l'UUID du compte : fermer une
//!   session ne touche pas au compte, et un jeton ne dit rien de son porteur.
//! - **`SameSite=Lax`, pas de vérification d'`Origin`** : derrière le relais
//!   Nitro, le backend voit l'origine *du relais* et non celle du navigateur —
//!   comparer `Origin` et `Host` ici refuserait **toutes** les requêtes légitimes.
//!   C'est `SameSite=Lax` qui protège des requêtes croisées (il empêche le
//!   navigateur d'envoyer le cookie sur un `POST` venu d'un autre site).
//! - **Argon2 est bloquant** : toute vérification passe par
//!   `tokio::task::spawn_blocking` (voir [`login`]).
//! - **Aucun message ne distingue** « identifiant inconnu », « mot de passe
//!   faux » et « compte désactivé » : un seul code, `invalid_credentials`, et
//!   une vérification Argon2 a lieu dans les trois cas (empreinte factice) pour
//!   que le **temps de réponse** ne trahisse pas l'existence d'un compte.

pub mod db;
pub mod oidc;
pub mod password;
pub mod permissions;
pub mod rbac;
pub mod tokens;

use crate::api::AppState;
use axum::Json;
use axum::extract::{FromRequestParts, OptionalFromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Nom du cookie de session.
pub const COOKIE_NAME: &str = "easy3d_session";

/// Page de connexion de l'interface : cible des redirections du SSO.
///
/// Le backend ne sert pas cette page (c'est Nitro), mais il connaît son adresse :
/// un retour de fournisseur refusé doit atterrir **quelque part d'utile**, et une
/// page blanche ne dirait pas à l'utilisateur ce qui s'est passé.
pub const LOGIN_PATH: &str = "/login";

/// Durée de vie d'une session.
const SESSION_DAYS: i64 = 30;

/// Tentatives de connexion ratées tolérées avant blocage temporaire.
const MAX_ATTEMPTS: u32 = 5;

/// Durée du blocage, en secondes.
const LOCKOUT_SECONDS: i64 = 60;

/// Nombre maximal d'identifiants suivis par le compteur de tentatives.
///
/// Garde-fou : sans lui, essayer des identifiants au hasard ferait grossir la
/// table indéfiniment (mémoire du serveur).
const MAX_TRACKED: usize = 1000;

/// Durée de vie d'un ticket d'accès WebSocket, en secondes.
///
/// Court **à dessein** : le ticket circule dans une URL (celle de la connexion
/// amont du relais Nitro), là où un cookie ne va jamais. Il ne sert qu'à ouvrir
/// une connexion, et le temps de latence entre l'émission et l'ouverture se
/// compte en millisecondes.
const TICKET_SECONDS: i64 = 60;

/// Nombre maximal de tickets en circulation (garde-fou mémoire).
const MAX_TICKETS: usize = 256;

/// État de l'authentification.
///
/// `db: None` **est** la fonctionnalité éteinte — pas un cas d'erreur : c'est
/// l'état par défaut, et il ne doit rien changer au reste du serveur.
pub struct Auth {
    db: Option<Mutex<Connection>>,
    /// Fichier SQLite (journalisé au démarrage, utilisé par les tests).
    path: PathBuf,
    /// `true` : le cookie de session porte `Secure` (interface en HTTPS).
    cookie_secure: bool,
    session_days: i64,
    /// Connexions ratées, par identifiant.
    attempts: Mutex<HashMap<String, Attempts>>,
    /// Tickets d'accès WebSocket en circulation, par jeton.
    tickets: Mutex<HashMap<String, Ticket>>,
    /// Durée de vie des tickets (modifiable pour les tests).
    ticket_seconds: i64,
    /// État transitoire du SSO : connexions commencées et découverte du
    /// fournisseur (voir [`oidc::Flow`]). Rien n'est écrit en base.
    pub(crate) oidc: oidc::Flow,
}

/// Compteur de tentatives ratées pour un identifiant.
#[derive(Debug, Clone, Copy)]
struct Attempts {
    count: u32,
    /// Début de la fenêtre courante : passé [`LOCKOUT_SECONDS`], le compteur
    /// repart de zéro (sinon un utilisateur distrait serait bloqué à vie).
    first: i64,
}

/// Droit d'ouvrir une connexion WebSocket, sans cookie.
#[derive(Debug, Clone)]
struct Ticket {
    user_uuid: String,
    expires_at: i64,
}

impl Auth {
    /// Authentification **désactivée** : l'état par défaut, sans base ouverte.
    pub fn disabled() -> Self {
        Self {
            db: None,
            path: PathBuf::new(),
            cookie_secure: true,
            session_days: SESSION_DAYS,
            attempts: Mutex::new(HashMap::new()),
            tickets: Mutex::new(HashMap::new()),
            ticket_seconds: TICKET_SECONDS,
            oidc: oidc::Flow::default(),
        }
    }

    /// Ouvre (et crée au besoin) la base des comptes.
    pub fn open(path: &Path, cookie_secure: bool) -> Result<Self, String> {
        let conn = db::open(path)?;
        // Ménage de démarrage : les sessions expirées pendant l'arrêt du
        // serveur n'ont plus à occuper la base — et les jetons d'API arrivés à
        // échéance non plus (leur date les neutralise de toute façon, mais
        // autant ne pas laisser traîner des lignes mortes).
        db::purge_expired_sessions(&conn)?;
        db::purge_expired_tokens(&conn)?;
        Ok(Self {
            db: Some(Mutex::new(conn)),
            path: path.to_path_buf(),
            cookie_secure,
            session_days: SESSION_DAYS,
            attempts: Mutex::new(HashMap::new()),
            tickets: Mutex::new(HashMap::new()),
            ticket_seconds: TICKET_SECONDS,
            oidc: oidc::Flow::default(),
        })
    }

    /// Construit l'état à partir de l'environnement.
    ///
    /// `config_path` sert à situer la base par défaut (`easy3d.db` **à côté** de
    /// `config.yml`, donc dans le dossier monté en conteneur).
    ///
    /// ⚠️ Une erreur ici doit **arrêter le serveur** (voir `main`) : continuer
    /// avec [`Auth::disabled`] après un `EASY3D_AUTH=on` ouvrirait le serveur à
    /// tout le monde à cause d'une faute de frappe.
    pub fn from_env(config_path: &Path) -> Result<Self, String> {
        if !parse_bool(std::env::var("EASY3D_AUTH").ok().as_deref(), "EASY3D_AUTH")? {
            return Ok(Self::disabled());
        }

        let path = match std::env::var("EASY3D_DB") {
            Ok(chemin) if !chemin.trim().is_empty() => PathBuf::from(chemin),
            _ => db::default_path(config_path),
        };

        // `Secure` n'est posé que si l'interface est servie en HTTPS : sur un
        // déploiement local en `http://192.168.1.10`, un cookie `Secure` n'est
        // jamais renvoyé par le navigateur et la connexion boucle sans fin.
        let secure = match std::env::var("EASY3D_COOKIE_SECURE") {
            Ok(valeur) => parse_bool(Some(&valeur), "EASY3D_COOKIE_SECURE")?,
            Err(_) => std::env::var("EASY3D_PUBLIC_URL")
                .unwrap_or_default()
                .starts_with("https://"),
        };

        Self::open(&path, secure)
    }

    /// `true` si l'authentification est active.
    pub fn is_enabled(&self) -> bool {
        self.db.is_some()
    }

    /// Chemin de la base (vide si désactivée).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `true` si le cookie porte l'attribut `Secure`.
    pub fn cookie_secure(&self) -> bool {
        self.cookie_secure
    }

    /// Exécute une requête sous le verrou de la base.
    ///
    /// Une seule connexion protégée par un `Mutex` : le trafic d'authentification
    /// se compte en requêtes par minute, une réserve de connexions n'apporterait
    /// rien. Le verrou ne doit **pas** être tenu pendant un `.await` (le
    /// compilateur le refuse, et c'est heureux : la vérification Argon2 est
    /// justement ce qu'on déporte dans `spawn_blocking`).
    pub fn db<R>(&self, f: impl FnOnce(&Connection) -> Result<R, String>) -> Result<R, String> {
        let guard = self
            .db
            .as_ref()
            .ok_or_else(|| "authentification désactivée".to_string())?
            .lock()
            .map_err(|_| "verrou de la base empoisonné".to_string())?;
        f(&guard)
    }

    /// Ouvre une session et renvoie son jeton.
    ///
    /// Publique : le changement de mot de passe renouvelle la session de
    /// l'appelant (toutes les autres sont fermées au passage).
    pub fn start_session(&self, user_uuid: &str, headers: &HeaderMap) -> Result<String, String> {
        let token = new_token();
        let expires_at = db::now() + self.session_days * 86_400;
        let user_agent = header_value(headers, header::USER_AGENT.as_str(), 200);
        // Derrière le relais Nitro, l'adresse vue par le backend est celle du
        // relais : la colonne n'a de sens que si un proxy place
        // `X-Forwarded-For`, et elle ne sert qu'au diagnostic.
        let ip = header_value(headers, "x-forwarded-for", 45);

        self.db(|conn| {
            db::create_session(conn, &token, user_uuid, expires_at, &user_agent, &ip)?;
            db::purge_expired_sessions(conn)?;
            Ok(())
        })?;
        Ok(token)
    }

    /// `Set-Cookie` d'ouverture de session, avec la durée et les attributs de
    /// cette installation.
    pub fn session_cookie(&self, token: &str) -> String {
        cookie_value(token, self.session_days, self.cookie_secure)
    }

    /// Ferme la session portée par la requête.
    fn end_session(&self, headers: &HeaderMap) -> Result<(), String> {
        let Some(token) = token_from_headers(headers) else {
            return Ok(());
        };
        // Le compte est lu **avant** la suppression : il faut son UUID pour
        // écarter aussi les tickets d'accès qu'il a reçus (une déconnexion doit
        // tout fermer, y compris la connexion WebSocket qui s'ouvrirait juste
        // après).
        let user = self.db(|conn| db::session_user(conn, &token, db::now()))?;
        self.db(|conn| db::delete_session(conn, &token))?;
        if let Some(user) = user {
            self.forget_tickets(&user.uuid);
        }
        Ok(())
    }

    /// Compte connecté, d'après le cookie de session. `None` si personne.
    pub fn resolve(&self, headers: &HeaderMap) -> Option<db::User> {
        if !self.is_enabled() {
            return None;
        }
        let token = token_from_headers(headers)?;
        self.db(|conn| db::session_user(conn, &token, db::now()))
            .ok()
            .flatten()
    }

    /// Compte authentifié par une requête : cookie de session, **jeton d'API**
    /// (`Authorization: Bearer`), ou ticket d'accès WebSocket.
    ///
    /// Trois façons d'être reconnu, parce qu'il y a trois sortes de clients : le
    /// navigateur (cookie), l'agent (jeton), et le relais Nitro qui ouvre une
    /// connexion WebSocket pour le compte du navigateur (ticket).
    pub fn resolve_request(&self, request: &Request) -> Option<db::User> {
        self.resolve(request.headers())
            .or_else(|| self.resolve_bearer(request.headers()))
            .or_else(|| {
                let ticket = query_param(request.uri().query()?, "ticket")?;
                self.resolve_ticket(ticket)
            })
    }

    /// Compte désigné par un en-tête `Authorization: Bearer <jeton>`.
    fn resolve_bearer(&self, headers: &HeaderMap) -> Option<db::User> {
        let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
        let token = value.split_once(' ')?.1.trim();
        self.resolve_token(token)
    }

    /// Délivre un ticket pour ce compte.
    fn issue_ticket(&self, user_uuid: &str) -> String {
        let now = db::now();
        let ticket = new_token();
        let mut tickets = self.tickets.lock().unwrap();
        tickets.retain(|_, t| t.expires_at > now);
        // Garde-fou mémoire : si le plafond est atteint (rafale de connexions
        // WebSocket), on écarte le plus ancien plutôt que de refuser le nouveau.
        if tickets.len() >= MAX_TICKETS
            && let Some(plus_ancien) = tickets
                .iter()
                .min_by_key(|(_, t)| t.expires_at)
                .map(|(jeton, _)| jeton.clone())
        {
            tickets.remove(&plus_ancien);
        }
        tickets.insert(
            ticket.clone(),
            Ticket {
                user_uuid: user_uuid.to_string(),
                expires_at: now + self.ticket_seconds,
            },
        );
        ticket
    }

    /// Compte désigné par un ticket encore valable.
    ///
    /// Le ticket **n'est pas consommé** à la première connexion : `y-websocket`
    /// rouvre la sienne de lui-même après une coupure, toujours avec la même
    /// URL, et un ticket à usage unique casserait cette reconnexion. Il suffit
    /// que sa durée de vie soit courte (voir [`TICKET_SECONDS`]).
    fn resolve_ticket(&self, ticket: &str) -> Option<db::User> {
        let now = db::now();
        let mut tickets = self.tickets.lock().unwrap();
        tickets.retain(|_, t| t.expires_at > now);
        let ticket = tickets.get(ticket)?;
        let user_uuid = ticket.user_uuid.clone();
        drop(tickets);

        // Le compte est relu à chaque fois : un compte désactivé entre-temps ne
        // doit pas continuer à ouvrir des connexions avec un ticket émis avant.
        self.db(|conn| db::find_by_uuid(conn, &user_uuid))
            .ok()
            .flatten()
            .filter(|user| !user.disabled)
    }

    /// Écarte les tickets d'accès d'un compte (déconnexion).
    fn forget_tickets(&self, user_uuid: &str) {
        self.tickets
            .lock()
            .unwrap()
            .retain(|_, ticket| ticket.user_uuid != user_uuid);
    }

    /// Vue d'un compte, telle que l'API la montre.
    fn view(&self, user: &db::User) -> Result<UserView, String> {
        Ok(UserView {
            uuid: user.uuid.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            roles: self.db(|conn| db::roles_of(conn, &user.uuid))?,
            permissions: self.permissions_of(user)?,
        })
    }

    /// `true` si le compte porte le rôle livré `admin`.
    ///
    /// C'est le **superutilisateur codé** : il a tous les droits sans qu'on ait à
    /// les lui accorder, et une installation ne peut donc pas se retrouver sans
    /// personne pour l'administrer.
    pub fn is_admin(&self, user: &db::User) -> Result<bool, String> {
        self.is_admin_uuid(&user.uuid)
    }

    /// Variante par identifiant, pour les appelants qui n'ont pas le compte sous
    /// la main — les sessions MCP, notamment, survivent à la requête qui les a
    /// ouvertes.
    pub fn is_admin_uuid(&self, user_uuid: &str) -> Result<bool, String> {
        self.db(|conn| db::has_role(conn, user_uuid, db::ADMIN_ROLE))
    }

    /// Droits effectifs d'un compte : rôles ∪ droits posés sur le compte.
    ///
    /// Pour un administrateur, la liste est renvoyée **complète** : l'interface
    /// s'en sert pour n'afficher que les actions permises, et une liste vide lui
    /// ferait masquer des actions que le serveur autorise.
    pub fn permissions_of(&self, user: &db::User) -> Result<Vec<String>, String> {
        self.permissions_of_uuid(&user.uuid)
    }

    /// Variante par identifiant (voir [`Auth::is_admin_uuid`]).
    pub fn permissions_of_uuid(&self, user_uuid: &str) -> Result<Vec<String>, String> {
        if self.is_admin_uuid(user_uuid)? {
            return Ok(permissions::all_ids()
                .iter()
                .map(|id| id.to_string())
                .collect());
        }
        self.db(|conn| db::effective_permissions(conn, user_uuid))
    }

    /// `true` si le compte a ce droit.
    pub fn can(&self, user: &db::User, permission: &str) -> Result<bool, String> {
        self.can_uuid(&user.uuid, permission)
    }

    /// Variante par identifiant (voir [`Auth::is_admin_uuid`]).
    pub fn can_uuid(&self, user_uuid: &str, permission: &str) -> Result<bool, String> {
        Ok(self
            .permissions_of_uuid(user_uuid)?
            .iter()
            .any(|id| id == permission))
    }

    /// `true` si cet identifiant a trop échoué récemment.
    ///
    /// Le verrouillage est **par identifiant**, pas par adresse : toutes les
    /// requêtes arrivent du relais Nitro, donc d'une seule adresse. Il protège
    /// donc un compte donné des essais en série, ce qui est le risque réel ici
    /// (l'interface n'est pas exposée directement).
    fn is_locked(&self, key: &str) -> bool {
        let now = db::now();
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, a| now - a.first < LOCKOUT_SECONDS);
        attempts
            .get(key)
            .is_some_and(|a| a.count >= MAX_ATTEMPTS && now - a.first < LOCKOUT_SECONDS)
    }

    /// Enregistre une tentative ratée.
    fn record_failure(&self, key: &str) {
        let now = db::now();
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, a| now - a.first < LOCKOUT_SECONDS);
        if attempts.len() >= MAX_TRACKED && !attempts.contains_key(key) {
            return;
        }
        let entry = attempts.entry(key.to_string()).or_insert(Attempts {
            count: 0,
            first: now,
        });
        if now - entry.first >= LOCKOUT_SECONDS {
            entry.count = 0;
            entry.first = now;
        }
        entry.count += 1;
    }

    /// Oublie les échecs d'un identifiant (connexion réussie).
    fn clear_failures(&self, key: &str) {
        self.attempts.lock().unwrap().remove(key);
    }
}

/// Compte authentifié, extrait de la requête par le middleware [`require_auth`].
///
/// Le middleware a déjà fait la lecture en base ; l'extracteur ne fait que
/// reprendre le résultat, sans deuxième requête.
#[derive(Debug, Clone)]
pub struct AuthUser(pub db::User);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    /// Ne peut échouer que hors du routeur protégé (route publique mal
    /// déclarée) : le rejet est donc un 401, jamais un 500.
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or_else(|| (StatusCode::UNAUTHORIZED, "unauthenticated").into_response())
    }
}

impl<S> OptionalFromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Version « au choix » de l'extracteur : utilisée par les routes qui
    /// répondent différemment selon qu'il y a un compte ou non (`/auth/ws-ticket`
    /// doit pouvoir dire « aucun ticket nécessaire » quand l'authentification
    /// est éteinte, ce qui n'est pas une erreur).
    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<AuthUser>().cloned())
    }
}

/// Réponse de `POST /auth/ws-ticket`.
#[derive(Debug, Clone, Serialize)]
pub struct WsTicket {
    /// `None` quand l'authentification est éteinte : le relais se connecte
    /// alors sans ticket, exactement comme avant cette fonctionnalité.
    pub ticket: Option<String>,
}

/// Ce que l'API montre d'un compte.
///
/// ⚠️ Le hachage du mot de passe et l'identifiant OIDC n'y sont **pas** : c'est
/// la même règle que `config::KEY_PLACEHOLDER` pour la clé d'API — ce qui n'a
/// pas à sortir n'est pas dans la structure qui sort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserView {
    pub uuid: String,
    pub username: String,
    pub email: String,
    pub roles: Vec<String>,
    /// Droits effectifs du compte (`catalog.read`, `model.delete`…).
    ///
    /// L'interface s'en sert pour n'afficher que ce qui est permis
    /// (`can('model.delete')`) — sans quoi elle proposerait des boutons que le
    /// serveur refusera, ce qui est la pire façon d'annoncer un droit manquant.
    pub permissions: Vec<String>,
}

/// Réponse de `GET /auth/me`.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// `false` quand l'authentification n'est pas activée : l'interface ne
    /// montre alors ni page de connexion ni menu de compte.
    pub enabled: bool,
    /// `None` quand personne n'est connecté.
    pub user: Option<UserView>,
    /// `Some` quand une connexion par fournisseur d'identité est possible : la
    /// page de connexion affiche alors son bouton (voir [`oidc::Status`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc: Option<oidc::Status>,
}

/// Demande de connexion par mot de passe.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Nom d'utilisateur **ou** adresse e-mail.
    pub login: String,
    pub password: String,
}

/// `GET /auth/me` — état de l'authentification et compte courant.
///
/// Répond toujours **200**, même sans session ni authentification : l'interface
/// appelle cette route au démarrage pour savoir s'il faut une page de connexion,
/// et un 401 y serait un cas d'erreur à gérer pour une réponse qui est, en
/// réalité, une information normale.
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = &state.auth;
    if !auth.is_enabled() {
        return Json(Status {
            enabled: false,
            user: None,
            oidc: None,
        })
        .into_response();
    }

    let user = auth.resolve(&headers).and_then(|user| auth.view(&user).ok());
    Json(Status {
        enabled: true,
        user,
        oidc: oidc::status(&state),
    })
    .into_response()
}

/// `POST /auth/ws-ticket` — droit d'ouvrir une connexion WebSocket.
///
/// Appelée par le relais Nitro, pas par le navigateur : c'est lui qui possède le
/// cookie (il relaie `/ws` et `/collab/*`) et qui ne peut pas le transmettre à
/// la connexion amont. Le ticket ne quitte donc jamais le serveur.
pub async fn ws_ticket(State(state): State<AppState>, user: Option<AuthUser>) -> Response {
    let ticket = user.map(|AuthUser(user)| state.auth.issue_ticket(&user.uuid));
    Json(WsTicket { ticket }).into_response()
}

/// `POST /auth/login` — connexion par mot de passe.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Response {
    let auth = state.auth.clone();
    if !auth.is_enabled() {
        return error(StatusCode::BAD_REQUEST, "auth_disabled");
    }

    let login = request.login.trim().to_lowercase();
    if login.is_empty() || request.password.is_empty() {
        return error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    if auth.is_locked(&login) {
        return error(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }

    let found = match auth.db(|conn| db::find_by_login(conn, &login)) {
        Ok(found) => found,
        Err(e) => return internal(&e),
    };

    // Empreinte à vérifier : celle du compte, ou une empreinte factice quand il
    // n'existe pas. Sans cela, une réponse instantanée pour un identifiant
    // inconnu dirait lesquels existent.
    let stored = found
        .as_ref()
        .and_then(|user| user.password_hash.clone())
        .unwrap_or_else(dummy_hash);
    let password = request.password.clone();
    let verified = tokio::task::spawn_blocking(move || password::verify(&password, &stored))
        .await
        .unwrap_or(false);

    let user = match found {
        Some(user) if verified && !user.disabled => user,
        _ => {
            auth.record_failure(&login);
            return error(StatusCode::UNAUTHORIZED, "invalid_credentials");
        }
    };

    let token = match auth.start_session(&user.uuid, &headers) {
        Ok(token) => token,
        Err(e) => return internal(&e),
    };
    let view = match auth.view(&user) {
        Ok(view) => view,
        Err(e) => return internal(&e),
    };
    auth.clear_failures(&login);

    (
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            cookie_value(&token, auth.session_days, auth.cookie_secure),
        )],
        Json(view),
    )
        .into_response()
}

/// `POST /auth/logout` — ferme la session et efface le cookie.
///
/// Toujours 200, même sans session : une déconnexion est idempotente, et un
/// utilisateur qui a déjà perdu son cookie n'a pas besoin d'un message d'erreur.
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let auth = &state.auth;
    if let Err(e) = auth.end_session(&headers) {
        return internal(&e);
    }
    (
        StatusCode::OK,
        [(
            header::SET_COOKIE,
            clear_cookie_value(auth.cookie_secure),
        )],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// Middleware des routes protégées.
///
/// Quand l'authentification est éteinte, il laisse passer **sans même lire** le
/// cookie : c'est ce qui garantit qu'activer la fonctionnalité ne change rien
/// tant que `EASY3D_AUTH` est absent.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.auth.is_enabled() {
        return next.run(request).await;
    }

    match state.auth.resolve_request(&request) {
        Some(user) => {
            request.extensions_mut().insert(AuthUser(user));
            next.run(request).await
        }
        None => error(StatusCode::UNAUTHORIZED, "unauthenticated"),
    }
}

/// Middleware d'un groupe de routes : exige un droit précis.
///
/// La couche reçoit **l'état et le droit** en même temps (`State<(AppState, &str)>`),
/// ce qui permet d'écrire une ligne par groupe de routes dans la table des
/// routes — un chemin, un droit — sans avoir à fabriquer une couche par droit.
///
/// Elle s'applique **après** [`require_auth`], qui a posé le compte dans les
/// extensions de la requête.
pub async fn require_permission(
    State((state, permission)): State<(AppState, &'static str)>,
    request: Request,
    next: Next,
) -> Response {
    if !state.auth.is_enabled() {
        return next.run(request).await;
    }

    let Some(AuthUser(user)) = request.extensions().get::<AuthUser>().cloned() else {
        return error(StatusCode::UNAUTHORIZED, "unauthenticated");
    };

    match state.auth.can(&user, permission) {
        Ok(true) => next.run(request).await,
        Ok(false) => error(StatusCode::FORBIDDEN, "forbidden"),
        Err(e) => internal(&e),
    }
}

/// Compte administrateur à créer au démarrage, lu dans l'environnement.
#[derive(Debug, Clone)]
pub struct AdminSeed {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// Lit `EASY3D_ADMIN_*`.
///
/// Les deux valeurs vont **ensemble** : n'en fournir qu'une est une
/// configuration incomplète, refusée explicitement plutôt qu'ignorée (un
/// administrateur qui ne se crée pas laisserait un serveur fermé, sans que rien
/// ne dise pourquoi).
pub fn admin_seed_from_env() -> Result<Option<AdminSeed>, String> {
    let email = non_empty_env("EASY3D_ADMIN_EMAIL");
    let password = non_empty_env("EASY3D_ADMIN_PASSWORD");

    match (email, password) {
        (None, None) => Ok(None),
        (Some(email), Some(password)) => {
            let username = non_empty_env("EASY3D_ADMIN_USERNAME").unwrap_or_else(|| {
                email
                    .split('@')
                    .next()
                    .filter(|nom| !nom.is_empty())
                    .unwrap_or("admin")
                    .to_string()
            });
            Ok(Some(AdminSeed {
                username,
                email,
                password,
            }))
        }
        (None, Some(_)) => Err("EASY3D_ADMIN_PASSWORD est défini sans EASY3D_ADMIN_EMAIL".into()),
        (Some(_), None) => Err("EASY3D_ADMIN_EMAIL est défini sans EASY3D_ADMIN_PASSWORD".into()),
    }
}

/// Crée le premier administrateur, **une seule fois**.
///
/// S'il existe déjà un administrateur actif, l'environnement est ignoré (avec un
/// message) : c'est ce qui rend impossible d'écraser un mot de passe — ou de
/// ressusciter un compte — en redémarrant le conteneur avec un `.env` oublié.
pub fn bootstrap_admin(auth: &Auth, seed: Option<AdminSeed>) -> Result<(), String> {
    if !auth.is_enabled() {
        return Ok(());
    }
    auth.db(db::ensure_builtin_roles)?;

    let admins = auth.db(db::count_admins)?;
    let Some(seed) = seed else {
        if admins == 0 {
            eprintln!(
                "⚠️  Authentification active et aucun compte : renseignez \
                 EASY3D_ADMIN_EMAIL et EASY3D_ADMIN_PASSWORD pour créer le premier \
                 administrateur."
            );
        }
        return Ok(());
    };

    if admins > 0 {
        println!(
            "Comptes : un administrateur existe déjà, EASY3D_ADMIN_* est ignoré \
             (création unique — passez par l'interface)."
        );
        return Ok(());
    }

    if let Err(code) = password::check_length(&seed.password) {
        return Err(format!(
            "EASY3D_ADMIN_PASSWORD refusé ({code}) : entre {} et {} caractères.",
            password::MIN_LEN,
            password::MAX_LEN
        ));
    }

    let hash = password::hash(&seed.password)?;
    let user = db::NewUser::new(&seed.username, &seed.email, Some(hash));
    auth.db(|conn| db::insert_user(conn, &user))?;
    auth.db(|conn| db::assign_role(conn, &user.uuid, db::ADMIN_ROLE))?;
    println!(
        "Compte administrateur créé : {} ({}) — le mot de passe vient de \
         EASY3D_ADMIN_PASSWORD, il ne sera plus jamais écrasé.",
        user.username, user.email
    );
    Ok(())
}

/// Jeton de session : 32 octets aléatoires (deux UUIDv4 mis bout à bout).
///
/// Un UUID seul ne porte que 122 bits utiles ; deux en font 244. `uuid` tire son
/// aléa de `getrandom`, donc pas besoin d'ajouter un générateur pour ça.
///
/// Sert aussi d'`state` au flux OIDC ([`oidc`]) : même besoin d'un secret
/// imprévisible, même source.
pub(crate) fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Empreinte factice, calculée une seule fois.
///
/// Elle ne doit jamais correspondre à un compte : elle sert uniquement à faire
/// tourner Argon2 quand l'identifiant est inconnu (voir [`login`]).
fn dummy_hash() -> String {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY
        .get_or_init(|| {
            password::hash("empreinte factice — jamais un compte")
                .unwrap_or_else(|_| "!".to_string())
        })
        .clone()
}

/// Valeur d'un paramètre de requête.
///
/// Analyse volontairement minimale : les tickets sont de l'hexadécimal, il n'y a
/// donc rien à décoder (un `%xx` dans la valeur ne peut pas être un ticket
/// valide, et ne le deviendra pas).
fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|paire| {
        let (cle, valeur) = paire.split_once('=')?;
        (cle == name && !valeur.is_empty()).then_some(valeur)
    })
}

/// Valeur d'un en-tête, tronquée (les en-têtes sont stockés en base).
fn header_value(headers: &HeaderMap, name: &str, max: usize) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(max).collect())
        .unwrap_or_default()
}

/// Jeton de session porté par l'en-tête `Cookie`.
///
/// Analyse volontairement minimale : un seul cookie nous intéresse, et le
/// format d'un `Cookie` est une liste `nom=valeur` séparée par des `;`.
pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name.trim() == COOKIE_NAME && !value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

/// `Set-Cookie` d'ouverture de session.
pub fn cookie_value(token: &str, days: i64, secure: bool) -> String {
    let mut value = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        days * 86_400
    );
    if secure {
        value.push_str("; Secure");
    }
    value
}

/// `Set-Cookie` de fermeture (même attributs, valeur vide, expiration immédiate).
pub fn clear_cookie_value(secure: bool) -> String {
    let mut value = format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    if secure {
        value.push_str("; Secure");
    }
    value
}

/// Interprète une variable d'environnement booléenne.
///
/// Une valeur inconnue est une **erreur**, jamais un « non » silencieux : une
/// faute de frappe (`EASY3D_AUTH=ture`) qui éteindrait l'authentification
/// laisserait le serveur grand ouvert sans que personne ne le voie.
fn parse_bool(raw: Option<&str>, name: &str) -> Result<bool, String> {
    match raw.map(|value| value.trim().to_ascii_lowercase()) {
        None => Ok(false),
        Some(value) if value.is_empty() => Ok(false),
        Some(value) => match value.as_str() {
            "1" | "on" | "true" | "yes" | "oui" => Ok(true),
            "0" | "off" | "false" | "no" | "non" => Ok(false),
            autre => Err(format!(
                "{name} : valeur inattendue « {autre} » (attendu : on/off)"
            )),
        },
    }
}

/// Variable d'environnement non vide, espaces retirés.
fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Erreur destinée à l'interface : un **code** stable, traduit côté Vue
/// (`auth.errors.*`), jamais une phrase figée dans le backend.
///
/// Le code arrive en `&str` (et non `&'static str`) : une partie des refus vient
/// de la base ou d'un appelant, pas d'une constante du binaire.
pub(crate) fn error(status: StatusCode, code: &str) -> Response {
    (status, code.to_string()).into_response()
}

/// Erreur interne : le détail part dans les logs, pas dans la réponse.
pub(crate) fn internal(message: &str) -> Response {
    eprintln!("⚠️  comptes : {message}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> (tempfile::TempDir, Auth) {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
        (dir, auth)
    }

    fn compte(auth: &Auth, username: &str, email: &str, password: &str) -> db::User {
        auth.db(db::ensure_builtin_roles).unwrap();
        let hash = password::hash(password).unwrap();
        let user = db::NewUser::new(username, email, Some(hash));
        auth.db(|conn| db::insert_user(conn, &user)).unwrap();
        // Relu depuis la base : c'est le compte tel qu'il est stocké, pas la
        // valeur qu'on vient d'envoyer.
        auth.db(|conn| db::find_by_uuid(conn, &user.uuid))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn un_ticket_ouvre_l_acces_pour_son_compte_et_expire() {
        let (_dir, mut auth) = base();
        let user = compte(&auth, "remi", "remi@exemple.fr", "motdepasse");
        let ticket = auth.issue_ticket(&user.uuid);
        assert_eq!(ticket.len(), 64);
        assert_eq!(
            auth.resolve_ticket(&ticket).map(|u| u.uuid),
            Some(user.uuid.clone())
        );
        assert!(auth.resolve_ticket("inconnu").is_none());
        assert!(auth.resolve_ticket("").is_none());

        // Le ticket est réutilisable dans sa fenêtre (y-websocket rouvre sa
        // connexion avec la même URL après une coupure)…
        assert!(auth.resolve_ticket(&ticket).is_some());

        // …mais ne vaut plus rien une fois **sa** fenêtre passée.
        auth.ticket_seconds = -1;
        let perime = auth.issue_ticket(&user.uuid);
        assert!(auth.resolve_ticket(&perime).is_none());
        // Chaque ticket garde l'échéance fixée à son émission : le changement de
        // durée ne s'applique pas rétroactivement (et le nettoyage n'emporte que
        // les tickets réellement expirés).
        assert!(auth.resolve_ticket(&ticket).is_some());
    }

    #[test]
    fn un_ticket_ne_vaut_que_pour_son_compte_et_pour_un_compte_actif() {
        let (_dir, auth) = base();
        let remi = compte(&auth, "remi", "remi@exemple.fr", "motdepasse");
        let autre = compte(&auth, "autre", "autre@exemple.fr", "motdepasse");

        let ticket = auth.issue_ticket(&remi.uuid);
        let ticket_autre = auth.issue_ticket(&autre.uuid);
        assert_ne!(ticket, ticket_autre);

        auth.db(|conn| {
            conn.execute("UPDATE users SET disabled = 1 WHERE uuid = ?1", [&remi.uuid])
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .unwrap();

        // Un compte désactivé après l'émission du ticket ne peut plus s'en
        // servir : le compte est relu à chaque résolution.
        assert!(auth.resolve_ticket(&ticket).is_none());
        assert!(auth.resolve_ticket(&ticket_autre).is_some());
    }

    #[test]
    fn l_authentification_est_eteinte_par_defaut() {
        let auth = Auth::disabled();
        assert!(!auth.is_enabled());
        assert!(auth.path().as_os_str().is_empty());
        // Aucun cookie ne peut être résolu : l'état éteint ne lit pas de base.
        assert!(auth.resolve(&HeaderMap::new()).is_none());
        assert!(auth.db(|_| Ok(())).is_err());
    }

    #[test]
    fn une_valeur_inconnue_n_eteint_pas_l_authentification_en_silence() {
        assert_eq!(parse_bool(Some("on"), "EASY3D_AUTH"), Ok(true));
        assert_eq!(parse_bool(Some("1"), "EASY3D_AUTH"), Ok(true));
        assert_eq!(parse_bool(Some("OFF"), "EASY3D_AUTH"), Ok(false));
        assert_eq!(parse_bool(None, "EASY3D_AUTH"), Ok(false));
        assert_eq!(parse_bool(Some("  "), "EASY3D_AUTH"), Ok(false));
        assert!(parse_bool(Some("ture"), "EASY3D_AUTH").is_err());
    }

    #[test]
    fn le_cookie_de_session_est_protege() {
        let ouvert = cookie_value("jeton", 30, true);
        assert!(ouvert.contains("easy3d_session=jeton"));
        assert!(ouvert.contains("HttpOnly"));
        assert!(ouvert.contains("SameSite=Lax"));
        assert!(ouvert.contains("Path=/"));
        assert!(ouvert.contains("Max-Age=2592000"));
        assert!(ouvert.contains("Secure"));

        // En HTTP (déploiement local), `Secure` doit pouvoir être retiré, sinon
        // le navigateur ne renverrait jamais le cookie.
        assert!(!cookie_value("jeton", 30, false).contains("Secure"));

        // La fermeture garde les mêmes attributs et expire tout de suite.
        let ferme = clear_cookie_value(false);
        assert!(ferme.contains("easy3d_session=;"));
        assert!(ferme.contains("Max-Age=0"));
    }

    #[test]
    fn le_jeton_est_lu_parmi_les_autres_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "autre=1; easy3d_session=abc123; theme=sombre"
                .parse()
                .unwrap(),
        );
        assert_eq!(token_from_headers(&headers).as_deref(), Some("abc123"));

        headers.insert(header::COOKIE, "autre=1".parse().unwrap());
        assert!(token_from_headers(&headers).is_none());

        headers.insert(header::COOKIE, "easy3d_session=".parse().unwrap());
        assert!(token_from_headers(&headers).is_none());

        assert!(token_from_headers(&HeaderMap::new()).is_none());
    }

    #[test]
    fn une_session_ne_vaut_que_pour_son_jeton_et_se_revoque() {
        let (_dir, auth) = base();
        let user = compte(&auth, "remi", "remi@exemple.fr", "motdepasse");

        let mut headers = HeaderMap::new();
        let token = auth.start_session(&user.uuid, &headers).unwrap();
        headers.insert(
            header::COOKIE,
            format!("easy3d_session={token}").parse().unwrap(),
        );
        assert_eq!(auth.resolve(&headers).map(|u| u.uuid), Some(user.uuid.clone()));

        auth.end_session(&headers).unwrap();
        assert!(auth.resolve(&headers).is_none());

        // Un cookie forgé ne vaut rien : le jeton doit être en base.
        headers.insert(header::COOKIE, "easy3d_session=forgé".parse().unwrap());
        assert!(auth.resolve(&headers).is_none());
    }

    #[test]
    fn deux_sessions_ont_des_jetons_differents_et_suffisamment_longs() {
        let jetons: Vec<String> = (0..8).map(|_| new_token()).collect();
        for jeton in &jetons {
            assert_eq!(jeton.len(), 64, "32 octets en hexadécimal");
            assert!(jeton.chars().all(|c| c.is_ascii_hexdigit()));
        }
        let mut uniques = jetons.clone();
        uniques.sort();
        uniques.dedup();
        assert_eq!(uniques.len(), jetons.len());
    }

    #[test]
    fn trop_de_tentatives_rattees_bloquent_temporairement() {
        let (_dir, auth) = base();
        assert!(!auth.is_locked("remi"));
        for _ in 0..MAX_ATTEMPTS {
            assert!(!auth.is_locked("remi"));
            auth.record_failure("remi");
        }
        assert!(auth.is_locked("remi"));

        // Le blocage ne concerne que l'identifiant concerné.
        assert!(!auth.is_locked("autre"));

        // Une connexion réussie remet le compteur à zéro.
        auth.clear_failures("remi");
        assert!(!auth.is_locked("remi"));
    }

    #[test]
    fn la_vue_d_un_compte_ne_porte_aucun_secret() {
        let (_dir, auth) = base();
        let user = compte(&auth, "remi", "remi@exemple.fr", "motdepasse");
        auth.db(|conn| db::assign_role(conn, &user.uuid, db::ADMIN_ROLE))
            .unwrap();

        let vue = auth.view(&user).unwrap();
        assert_eq!(vue.username, "remi");
        assert_eq!(vue.roles, vec!["admin"]);

        let json = serde_json::to_string(&vue).unwrap();
        assert!(!json.contains("password_hash"), "json : {json}");
        assert!(!json.contains("argon2"), "json : {json}");
        assert!(!json.contains("oidc_subject"), "json : {json}");
    }

    #[test]
    fn l_administrateur_de_demarrage_n_est_cree_qu_une_fois() {
        let (_dir, auth) = base();
        let seed = AdminSeed {
            username: "remi".into(),
            email: "remi@exemple.fr".into(),
            password: "motdepasse".into(),
        };

        bootstrap_admin(&auth, Some(seed.clone())).unwrap();
        assert_eq!(auth.db(db::count_admins).unwrap(), 1);

        // Deuxième démarrage avec un **autre** mot de passe : rien ne change.
        let autre = AdminSeed {
            password: "motdepasse2".into(),
            ..seed.clone()
        };
        bootstrap_admin(&auth, Some(autre)).unwrap();
        assert_eq!(auth.db(db::count_admins).unwrap(), 1);

        let user = auth
            .db(|conn| db::find_by_login(conn, "remi"))
            .unwrap()
            .unwrap();
        let hash = user.password_hash.unwrap();
        assert!(
            password::verify("motdepasse", &hash),
            "le mot de passe d'origine doit rester valable"
        );
        assert!(!password::verify("motdepasse2", &hash));
    }

    #[test]
    fn l_administrateur_de_demarrage_refuse_un_mot_de_passe_trop_court() {
        let (_dir, auth) = base();
        let seed = AdminSeed {
            username: "remi".into(),
            email: "remi@exemple.fr".into(),
            password: "court".into(),
        };
        let erreur = bootstrap_admin(&auth, Some(seed)).unwrap_err();
        assert!(erreur.contains("password_too_short"), "erreur : {erreur}");
        assert_eq!(auth.db(db::count_admins).unwrap(), 0);
    }

    #[test]
    fn sans_administrateur_l_amorcage_ne_fait_rien_mais_previent() {
        let (_dir, auth) = base();
        bootstrap_admin(&auth, None).unwrap();
        assert_eq!(auth.db(db::count_admins).unwrap(), 0);
        // Les rôles livrés existent malgré tout : l'interface peut les lister.
        assert_eq!(auth.db(db::role_names).unwrap(), vec!["admin", "lecteur"]);
    }

    #[test]
    fn l_amorcage_est_sans_effet_quand_l_authentification_est_eteinte() {
        let auth = Auth::disabled();
        let seed = AdminSeed {
            username: "remi".into(),
            email: "remi@exemple.fr".into(),
            password: "motdepasse".into(),
        };
        // Aucune base ouverte : l'appel ne doit pas échouer ni écrire quoi que
        // ce soit (le serveur démarre sans comptes, comme avant).
        assert!(bootstrap_admin(&auth, Some(seed)).is_ok());
    }
}
