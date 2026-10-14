//! Connexion par un fournisseur d'identité OIDC (Keycloak, Entra ID, Auth0…).
//!
//! # Ce que fait ce module
//!
//! Un compte peut se connecter avec un **mot de passe local** *ou* par le
//! fournisseur d'identité de l'organisation : les deux cohabitent, et l'identité
//! obtenue par OIDC ouvre **la même session** que l'autre (le RBAC ne change
//! pas : un rôle reste un rôle).
//!
//! # Choix à connaître avant de toucher au module
//!
//! - **Le flux vit ici, pas dans le navigateur.** Le `client_secret` ne quitte
//!   jamais le backend : le navigateur reçoit un `302` vers le fournisseur, puis
//!   revient sur [`CALLBACK_PATH`] où l'échange code→jeton se fait côté serveur.
//! - **Pas de vérification de l'`id_token`.** Le jeton d'accès est obtenu par
//!   nous (client confidentiel, secret partagé), et les revendications sont
//!   demandées à `userinfo` : cela évite d'embarquer une bibliothèque JWT et une
//!   gestion de clés (JWKS) pour contrôler une signature RS256, pour un résultat
//!   équivalent — le `sub` vient d'une source que nous avons nous-mêmes
//!   authentifiée.
//! - **Le rattachement se verrouille sur le `sub`** au premier login. L'e-mail
//!   ne sert qu'une fois : à retrouver un compte déjà créé par l'administrateur.
//! - **Le provisionnement est explicite** (voir [`config::Provisioning`]) :
//!   personne n'entre « par accident » dans une installation où les comptes sont
//!   créés à la main.
//! - **Rien n'est journalisé qui vienne du fournisseur** (jetons, secret) : seuls
//!   les codes d'erreur et les identifiants de compte le sont.

use crate::api::AppState;
use crate::auth::{self, Auth, db};
use crate::config::{Oidc, Provisioning};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

/// Chemin du retour du fournisseur, **côté interface** : c'est l'URL que le
/// navigateur visite, et que Nitro relaie au backend (voir `server/api/auth/oidc/`).
///
/// Elle doit être déclarée telle quelle dans le fournisseur d'identité, et le
/// backend ne peut la deviner que par `EASY3D_PUBLIC_URL` (ou, à défaut, par
/// l'en-tête `Host` de la requête qui a commencé le flux).
pub const CALLBACK_PATH: &str = "/api/auth/oidc/callback";

/// Durée de vie d'un état de connexion, en secondes.
///
/// Le temps de faire l'aller-retour chez le fournisseur, large : un utilisateur
/// qui doit s'authentifier **plus** une validation à deux facteurs y passe
/// facilement une minute.
const STATE_SECONDS: i64 = 600;

/// Nombre maximal d'états en circulation (garde-fou mémoire).
const MAX_STATES: usize = 256;

/// Durée de mise en cache de la découverte, en secondes.
///
/// Deux allers-retours vers le fournisseur par connexion (début et retour) :
/// la garder évite d'en faire un troisième sans figer les adresses pour autant.
const DISCOVERY_SECONDS: i64 = 600;

/// Délai maximal d'un appel au fournisseur.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Délai entre deux vérifications d'une **même** session SSO, en secondes.
///
/// Cinq minutes : c'est le prix à payer pour qu'une désactivation chez le
/// fournisseur se voie ici, au lieu d'attendre l'expiration de la session
/// (trente jours). Plus court, chaque requête d'un utilisateur connecté par SSO
/// déclencherait un appel sortant ; plus long, une personne désactivée garderait
/// l'accès d'autant.
pub const SSO_CHECK_SECONDS: i64 = 300;

/// Période du balayage de fond, en secondes (voir `main`).
///
/// Indépendante de [`SSO_CHECK_SECONDS`] : le balayage passe souvent, mais ne
/// revérifie chaque session qu'une fois par `SSO_CHECK_SECONDS`. C'est ce qui
/// fait tomber une session **sans trafic** — un onglet ouvert ne fait pas de
/// requête, et il ne doit pas rester ouvert indéfiniment.
pub const SSO_SWEEP_SECONDS: u64 = 30;

// ── Réglages : environnement par-dessus `config.yml` ─────────────────────

/// Réglages OIDC venus de l'**environnement**.
///
/// Ils gagnent sur `config.yml` : un secret client n'a rien à faire dans un
/// fichier versionnable, et un déploiement qui l'a déjà posé dans son
/// `docker-compose.yml` ne doit pas avoir à le recopier dans l'interface.
///
/// L'état est un **champ de [`AppState`]** (rempli par `main`, vide dans les
/// tests) : rien ici ne lit l'environnement, ce qui rend le reste testable.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Env {
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub provisioning: Option<Provisioning>,
    /// URL publique de l'interface (`EASY3D_PUBLIC_URL`), pour construire le
    /// `redirect_uri` quand le backend ne voit que le relais Nitro.
    pub public_url: Option<String>,
}

impl Env {
    /// Aucun réglage : c'est le cas quand tout se règle dans `config.yml`, et
    /// celui des tests.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Lit `EASY3D_OIDC_*` (et `EASY3D_PUBLIC_URL`).
    ///
    /// ⚠️ Une valeur inconnue (`EASY3D_OIDC_PROVISIONING=manualy`) est une
    /// **erreur** : elle doit arrêter le serveur, jamais éteindre le SSO en
    /// silence (même règle que `EASY3D_AUTH`).
    pub fn from_env() -> Result<Self, String> {
        let provisioning = match non_empty("EASY3D_OIDC_PROVISIONING") {
            Some(value) => Some(Provisioning::parse(&value).ok_or_else(|| {
                format!("EASY3D_OIDC_PROVISIONING : valeur inattendue « {value} » (attendu : auto, manual ou approval)")
            })?),
            None => None,
        };

        Ok(Self {
            issuer: non_empty("EASY3D_OIDC_ISSUER"),
            client_id: non_empty("EASY3D_OIDC_CLIENT_ID"),
            // `EASY3D_OIDC_CLIENT_SECRET_FILE` est accepté aussi : un secret
            // passé par l'environnement se lit dans `docker inspect`, `ps e` et
            // l'historique du shell, ce qui n'est pas le cas d'un fichier monté.
            client_secret: auth::env_secret("EASY3D_OIDC_CLIENT_SECRET"),
            scopes: non_empty("EASY3D_OIDC_SCOPES").map(|value| {
                value
                    .split_whitespace()
                    .map(|scope| scope.to_string())
                    .collect()
            }),
            provisioning,
            public_url: non_empty("EASY3D_PUBLIC_URL"),
        })
    }

    /// `true` si l'environnement décrit un fournisseur.
    ///
    /// Les valeurs vont ensemble (émetteur + identifiant + secret) : n'en fournir
    /// qu'une partie est une configuration incomplète, signalée par
    /// [`resolve`] plutôt qu'ignorée.
    pub fn is_set(&self) -> bool {
        self.issuer.is_some()
            || self.client_id.is_some()
            || self.client_secret.is_some()
            || self.scopes.is_some()
            || self.provisioning.is_some()
    }

    /// Champs **figés par l'environnement** : l'interface les montre grisés.
    ///
    /// Ils portent les mêmes noms que ceux du bloc `oidc` de `config.yml`, pour
    /// que le frontend n'ait qu'à les comparer.
    pub fn locked(&self) -> Vec<&'static str> {
        let mut locked = Vec::new();
        if self.is_set() {
            // Le SSO est allumé par l'environnement : le mettre à « non » dans
            // le fichier n'y changerait rien, donc l'interrupteur est figé lui
            // aussi.
            locked.push("enabled");
        }
        for (name, present) in [
            ("issuer", self.issuer.is_some()),
            ("client_id", self.client_id.is_some()),
            ("client_secret", self.client_secret.is_some()),
            ("scopes", self.scopes.is_some()),
            ("provisioning", self.provisioning.is_some()),
        ] {
            if present {
                locked.push(name);
            }
        }
        locked
    }
}

/// Ce qui manque à un SSO activé.
///
/// Un **code**, pas une phrase : le message part dans le journal au démarrage,
/// et l'interface le traduit (elle seule connaît la langue de l'utilisateur).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    Issuer,
    ClientId,
    ClientSecret,
}

impl Missing {
    /// Identifiant stable, tel que l'API le transmet.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Issuer => "issuer",
            Self::ClientId => "client_id",
            Self::ClientSecret => "client_secret",
        }
    }

    /// Nom de la variable d'environnement correspondante.
    pub fn variable(&self) -> &'static str {
        match self {
            Self::Issuer => "EASY3D_OIDC_ISSUER",
            Self::ClientId => "EASY3D_OIDC_CLIENT_ID",
            Self::ClientSecret => "EASY3D_OIDC_CLIENT_SECRET",
        }
    }
}

/// Réglages effectifs : `config.yml`, corrigés par l'environnement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: Vec<String>,
    pub provisioning: Provisioning,
}

/// Réglages effectifs, ou `None` quand le SSO est éteint.
///
/// ⚠️ Un SSO **à moitié configuré** est une erreur, jamais un « éteint »
/// silencieux : voir `EASY3D_OIDC_ISSUER` dans le compose et croire que le SSO
/// marche alors qu'il est éteint est exactement le genre de panne qu'on ne
/// diagnostique pas.
pub fn resolve(config: &Oidc, env: &Env) -> Result<Option<Settings>, Missing> {
    let issuer = env.issuer.clone().or_else(|| non_empty_of(&config.issuer));
    let client_id = env
        .client_id
        .clone()
        .or_else(|| non_empty_of(&config.client_id));
    let client_secret = env
        .client_secret
        .clone()
        .or_else(|| non_empty_of(&config.client_secret));
    let scopes = env
        .scopes
        .clone()
        .unwrap_or_else(|| config.effective_scopes());
    let provisioning = env.provisioning.unwrap_or(config.provisioning);

    if !env.is_set() && !config.enabled {
        return Ok(None);
    }

    Ok(Some(Settings {
        issuer: issuer.ok_or(Missing::Issuer)?,
        client_id: client_id.ok_or(Missing::ClientId)?,
        client_secret: client_secret.ok_or(Missing::ClientSecret)?,
        scopes,
        provisioning,
    }))
}

/// Réglages effectifs de l'installation courante.
pub fn settings(state: &AppState) -> Result<Option<Settings>, Missing> {
    resolve(&state.config().oidc, &state.oidc)
}

/// Phrase de journal (démarrage, ou panne d'un appelant).
pub fn describe(manque: Missing) -> String {
    format!(
        "SSO activé mais « {} » est vide (config.yml ou {})",
        manque.id(),
        manque.variable()
    )
}

/// `true` si le SSO est utilisable (l'interface s'en sert pour son bouton).
pub fn is_enabled(state: &AppState) -> bool {
    state.auth.is_enabled() && matches!(settings(state), Ok(Some(_)))
}

// ── État transitoire du flux ────────────────────────────────────────────

/// Une connexion commencée, en attente du retour du fournisseur.
#[derive(Debug, Clone)]
pub struct Pending {
    /// `redirect_uri` envoyé au fournisseur : renvoyé **à l'identique** à
    /// l'échange du code (les fournisseurs refusent une valeur différente).
    pub redirect_uri: String,
    /// Page demandée avant la connexion (`/dossiers/Maison`).
    pub return_to: String,
    expires_at: i64,
}

/// Découverte gardée en mémoire, avec son émetteur d'origine.
#[derive(Debug, Clone)]
pub struct Cached {
    issuer: String,
    discovery: Discovery,
    fetched_at: i64,
}

/// État transitoire du SSO, porté par [`Auth`] : rien n'est écrit en base.
///
/// Les états de connexion sont **à usage unique** (contrairement aux tickets
/// WebSocket, qu'un client rouvre avec la même URL) : rejouer un retour du
/// fournisseur ne doit pas rouvrir une session.
#[derive(Debug, Default)]
pub struct Flow {
    states: Mutex<HashMap<String, Pending>>,
    discovery: Mutex<Option<Cached>>,
}

impl Flow {
    /// Enregistre une connexion commencée, en écartant les plus anciennes.
    fn remember(&self, state: &str, pending: Pending) {
        let now = db::now();
        let mut states = self.states.lock().unwrap();
        states.retain(|_, p| p.expires_at > now);
        if states.len() >= MAX_STATES
            && let Some(ancien) = states
                .iter()
                .min_by_key(|(_, p)| p.expires_at)
                .map(|(state, _)| state.clone())
        {
            states.remove(&ancien);
        }
        states.insert(state.to_string(), pending);
    }

    /// Consomme un état : `None` s'il est inconnu, déjà utilisé ou expiré.
    fn take(&self, state: &str) -> Option<Pending> {
        let now = db::now();
        let mut states = self.states.lock().unwrap();
        states.retain(|_, p| p.expires_at > now);
        states.remove(state)
    }

    /// Découverte en cache, si elle concerne le même émetteur et qu'elle est
    /// encore fraîche.
    fn discovery_of(&self, issuer: &str) -> Option<Discovery> {
        let now = db::now();
        let cached = self.discovery.lock().unwrap();
        cached
            .as_ref()
            .filter(|c| c.issuer == issuer && now - c.fetched_at < DISCOVERY_SECONDS)
            .map(|c| c.discovery.clone())
    }

    /// Retient une découverte, en remplaçant celle d'un autre émetteur.
    fn remember_discovery(&self, issuer: &str, discovery: &Discovery) {
        *self.discovery.lock().unwrap() = Some(Cached {
            issuer: issuer.to_string(),
            discovery: discovery.clone(),
            fetched_at: db::now(),
        });
    }

    /// Oublie la découverte : la prochaine demande ira la chercher.
    ///
    /// Utilisé par le contrôle de l'administration, dont le but est justement
    /// de vérifier une adresse que l'on vient de changer.
    fn forget_discovery(&self) {
        *self.discovery.lock().unwrap() = None;
    }
}

// ── Découverte et adresses ──────────────────────────────────────────────

/// Adresses utiles du fournisseur, telles qu'il les annonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Émetteur **annoncé par le document**, comparé à celui de la
    /// configuration (voir [`discover`]).
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
}

/// Adresse du document de découverte, à partir de l'émetteur.
pub fn discovery_url(issuer: &str) -> String {
    format!(
        "{}/.well-known/openid-configuration",
        issuer.trim().trim_end_matches('/')
    )
}

/// Lit le document de découverte.
pub fn parse_discovery(body: &str) -> Result<Discovery, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("découverte OIDC illisible : {e}"))?;
    let field = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| format!("découverte OIDC : « {name} » absent"))
    };
    Ok(Discovery {
        issuer: field("issuer")?,
        authorization_endpoint: field("authorization_endpoint")?,
        token_endpoint: field("token_endpoint")?,
        userinfo_endpoint: field("userinfo_endpoint")?,
    })
}

/// `true` si l'émetteur annoncé est celui que l'on a configuré.
///
/// La comparaison ignore une barre oblique finale (les fournisseurs ne sont pas
/// tous d'accord sur sa présence) mais rien d'autre : `issuer` est une chaîne
/// **exacte** au sens de la spécification, et la normaliser davantage (casse,
/// ordre des segments) ferait accepter un document qui ne vient pas du bon
/// endroit.
pub fn same_issuer(annonce: &str, configure: &str) -> bool {
    let nettoyer = |value: &str| value.trim().trim_end_matches('/').to_string();
    !annonce.trim().is_empty() && nettoyer(annonce) == nettoyer(configure)
}

/// Adresse de connexion chez le fournisseur.
///
/// Fonction **pure** : c'est la partie qu'on veut pouvoir relire et tester sans
/// réseau, une URL d'autorisation ratée se diagnostiquant très mal côté
/// navigateur.
pub fn authorize_url(
    settings: &Settings,
    discovery: &Discovery,
    redirect_uri: &str,
    state: &str,
) -> String {
    let separateur = if discovery.authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    format!(
        "{}{separateur}response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        discovery.authorization_endpoint,
        encode(&settings.client_id),
        encode(redirect_uri),
        encode(&settings.scopes.join(" ")),
        encode(state),
    )
}

/// Corps du `POST` d'échange du code contre un jeton d'accès.
pub fn token_form(settings: &Settings, redirect_uri: &str, code: &str) -> String {
    form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", &settings.client_id),
        ("client_secret", &settings.client_secret),
    ])
}

/// Corps du `POST` de rafraîchissement (voir [`revalidate_sessions`]).
pub fn refresh_form(settings: &Settings, refresh_token: &str) -> String {
    form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", &settings.client_id),
        ("client_secret", &settings.client_secret),
    ])
}

/// Jetons rendus par le fournisseur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access_token: String,
    /// Absent chez certains fournisseurs, et pas systématiquement renvoyé lors
    /// d'un rafraîchissement : garder l'ancien est alors la bonne réponse.
    pub refresh_token: Option<String>,
}

/// Jetons extraits de la réponse du fournisseur.
pub fn parse_token_response(body: &str) -> Result<Tokens, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("réponse du jeton illisible : {e}"))?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "réponse du jeton sans « access_token »".to_string())?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    Ok(Tokens {
        access_token,
        refresh_token,
    })
}

/// Jeton d'accès seul (le rafraîchissement n'intéresse pas l'appelant).
pub fn parse_token(body: &str) -> Result<String, String> {
    parse_token_response(body).map(|tokens| tokens.access_token)
}

/// Identité d'un utilisateur, telle que le fournisseur la décrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Identifiant **stable** chez le fournisseur : la clé du rattachement.
    pub subject: String,
    pub email: String,
    /// Nom d'utilisateur souhaité (déjà nettoyé, pas encore rendu unique).
    pub username: String,
}

/// Lit la réponse de `userinfo`.
///
/// `email` est exigé : c'est lui qui permet de rattacher un compte préparé par
/// l'administrateur, et deux comptes sans adresse seraient indiscernables.
pub fn parse_identity(body: &str) -> Result<Identity, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("réponse userinfo illisible : {e}"))?;
    let text = |name: &str| -> Option<String> {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    let subject = text("sub").ok_or_else(|| "userinfo sans « sub »".to_string())?;
    let email = text("email")
        .map(|email| email.to_lowercase())
        .ok_or_else(|| "oidc_no_email".to_string())?;
    let username = text("preferred_username")
        .map(|name| sanitize_username(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| sanitize_username(email.split('@').next().unwrap_or("")));

    Ok(Identity {
        subject,
        email,
        username,
    })
}

/// Nom d'utilisateur acceptable : les mêmes règles que `rbac::valid_username`
/// (l'interface et le SSO créent des comptes de la même façon).
fn sanitize_username(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Codes destinés à l'utilisateur (affichés par la page de connexion).
///
/// Tout le reste est une panne (fournisseur injoignable, découverte illisible) :
/// elle est journalisée et se traduit par un message générique.
pub fn is_client_code(code: &str) -> bool {
    matches!(
        code,
        "oidc_no_email"
            | "oidc_not_provisioned"
            | "oidc_conflict"
            | "pending_approval"
            | "account_disabled"
            | "invalid_state"
            | "oidc_denied"
    )
}

// ── Provisionnement ─────────────────────────────────────────────────────

/// Compte à qui cette identité donne accès, en le créant ou en le rattachant au
/// besoin.
///
/// Fonction **pure au sens du réseau** : elle ne parle qu'à la base, donc elle
/// se teste entièrement (c'est ici que se joue « qui a le droit d'entrer »).
pub fn provision(
    auth: &Auth,
    settings: &Settings,
    identity: &Identity,
) -> Result<db::User, String> {
    // 1. Déjà rattaché : le cas normal, à partir du deuxième login. Le `sub` est
    //    la seule chose qui compte alors — l'e-mail peut avoir changé chez le
    //    fournisseur.
    if let Some(user) = auth.db(|conn| db::find_by_subject(conn, &identity.subject))? {
        return match user.disabled {
            true => Err("account_disabled".to_string()),
            false => Ok(user),
        };
    }

    // 2. Compte préparé par l'administrateur (ou compte local de même adresse) :
    //    on le rattache. Un compte déjà rattaché à une **autre** identité est
    //    refusé : sans cela, qui contrôle le second fournisseur prendrait le
    //    compte du premier.
    if let Some(user) = auth.db(|conn| db::find_by_email(conn, &identity.email))? {
        if user.oidc_subject.is_some() {
            return Err("oidc_conflict".to_string());
        }
        // **Le rattachement ne touche pas au mot de passe** : l'empreinte reste
        // en base, inerte, et resservira si le compte est détaché
        // (`rbac::detach_oidc`) — c'est l'**entrée** qui est filtrée, pas la
        // donnée (voir `auth::login`).
        //
        // Les sessions ouvertes par ce mot de passe tombent en revanche — sauf
        // celle qu'on ouvre juste après (l'ordre compte : la nouvelle session
        // naît après ce bloc) — et celles d'un administrateur sont épargnées,
        // comme son mot de passe : c'est la porte de secours de l'installation.
        auth.db(|conn| {
            db::set_oidc_subject(conn, &user.uuid, &identity.subject)?;
            if !db::has_role(conn, &user.uuid, db::ADMIN_ROLE)? {
                db::delete_sessions_of(conn, &user.uuid)?;
            }
            Ok(())
        })?;
        return match user.disabled {
            true => Err("account_disabled".to_string()),
            // Relu après l'écriture : le compte qu'on rend doit porter le
            // rattachement tout juste posé (sinon l'appelant travaille sur une
            // version périmée, et le prochain login dépendrait d'un autre
            // `sub`).
            false => auth
                .db(|conn| db::find_by_uuid(conn, &user.uuid))?
                .ok_or_else(|| "compte disparu".to_string()),
        };
    }

    // 3. Inconnu : c'est le mode de provisionnement qui décide.
    match settings.provisioning {
        // Seuls les comptes préparés entrent : l'administrateur garde la main
        // sur qui possède un compte.
        Provisioning::Manual => Err("oidc_not_provisioned".to_string()),
        Provisioning::Auto | Provisioning::Approval => {
            // En mode `approval`, le compte naît **désactivé** : il apparaît
            // dans la page Comptes, où l'administrateur le valide. La connexion
            // est refusée en attendant, avec un code qui le dit.
            let disabled = settings.provisioning == Provisioning::Approval;
            let username = unique_username(auth, &identity.username)?;
            let mut user = db::NewUser::new(&username, &identity.email, None);
            user.oidc_subject = Some(identity.subject.clone());
            let user = crate::auth::rbac::insert_account(auth, user, &[], &[])?;
            if disabled {
                auth.db(|conn| db::set_disabled(conn, &user.uuid, true))?;
                return Err("pending_approval".to_string());
            }
            Ok(user)
        }
    }
}

/// Nom d'utilisateur libre : celui du fournisseur, suffixé si la place est prise.
///
/// La base a la dernière main (contrainte d'unicité), mais autant proposer tout
/// de suite un nom acceptable plutôt que de faire échouer la création.
fn unique_username(auth: &Auth, wanted: &str) -> Result<String, String> {
    let base = if wanted.is_empty() {
        "utilisateur".to_string()
    } else {
        wanted.to_string()
    };
    let mut candidate = base.clone();
    for suffixe in 2..100 {
        let pris = auth
            .db(|conn| db::find_by_login(conn, &candidate))?
            .is_some();
        if !pris {
            return Ok(candidate);
        }
        candidate = format!("{base}-{suffixe}");
    }
    // Cent noms dérivés déjà pris : on laisse l'UUID (unique) décider.
    Ok(format!("{base}-{}", uuid::Uuid::new_v4().simple()))
}

// ── Appels au fournisseur ───────────────────────────────────────────────

/// Réponse brute d'un appel : code HTTP et corps.
type Raw = Result<(u16, String), String>;

/// Envoi d'une requête, représenté comme une valeur pour pouvoir être remplacé
/// par un scénario dans les tests (même procédé que `ai::search_with`).
type Answer = Pin<Box<dyn Future<Output = Raw> + Send>>;

/// Requête sortante du flux OIDC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `GET` avec, éventuellement, un jeton porteur.
    Get { url: String, bearer: Option<String> },
    /// `POST` d'un formulaire (`application/x-www-form-urlencoded`).
    Form { url: String, body: String },
}

/// Client HTTP du flux.
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("client HTTP : {e}"))
}

/// Exécute une requête du flux.
async fn send(client: &reqwest::Client, request: Request) -> Raw {
    let builder = match request {
        Request::Get { url, bearer } => {
            let builder = client.get(url);
            match bearer {
                Some(token) => builder.bearer_auth(token),
                None => builder,
            }
        }
        Request::Form { url, body } => client
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body),
    };

    match builder.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            match response.text().await {
                Ok(body) => Ok((status, body)),
                Err(e) => Err(format!("réponse illisible : {e}")),
            }
        }
        Err(e) => Err(format!("appel au fournisseur impossible : {e}")),
    }
}

/// Découverte du fournisseur, gardée quelques minutes en mémoire.
async fn discover<F>(
    auth: &Auth,
    settings: &Settings,
    transport: &mut F,
) -> Result<Discovery, String>
where
    F: FnMut(Request) -> Answer,
{
    if let Some(discovery) = auth.oidc.discovery_of(&settings.issuer) {
        return Ok(discovery);
    }
    let (status, body) = transport(Request::Get {
        url: discovery_url(&settings.issuer),
        bearer: None,
    })
    .await?;
    if status >= 400 {
        return Err(format!("découverte OIDC refusée ({status}) : {body}"));
    }
    let discovery = parse_discovery(&body)?;
    // L'émetteur annoncé doit être celui qu'on a configuré : sans ce contrôle,
    // une redirection de la découverte (DNS, proxy, fournisseur mal recopié)
    // enverrait le secret client et les jetons à quelqu'un d'autre sans que rien
    // ne le dise.
    if !same_issuer(&discovery.issuer, &settings.issuer) {
        return Err(format!(
            "découverte OIDC : l'émetteur annoncé (« {} ») n'est pas celui configuré (« {} »)",
            discovery.issuer, settings.issuer
        ));
    }
    auth.oidc.remember_discovery(&settings.issuer, &discovery);
    Ok(discovery)
}

// ── Routes ──────────────────────────────────────────────────────────────

/// `GET /auth/oidc/start` — début du flux : redirection vers le fournisseur.
#[derive(Debug, Deserialize)]
pub struct StartQuery {
    /// Page à rejoindre après la connexion (facultatif, chemin interne).
    pub redirect: Option<String>,
}

/// `GET /auth/oidc/callback` — retour du fournisseur.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Renseigné par le fournisseur quand il refuse (`access_denied`…).
    pub error: Option<String>,
}

/// Démarre la connexion SSO.
pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StartQuery>,
) -> Response {
    let Ok(client) = client() else {
        return auth::internal("client HTTP indisponible");
    };
    let transport = move |request: Request| -> Answer {
        let client = client.clone();
        Box::pin(async move { send(&client, request).await })
    };
    start_with(&state, &headers, query.redirect.as_deref(), transport).await
}

/// Retour du fournisseur : échange le code, rattache le compte, ouvre la session.
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Ok(client) = client() else {
        return auth::internal("client HTTP indisponible");
    };
    let transport = move |request: Request| -> Answer {
        let client = client.clone();
        Box::pin(async move { send(&client, request).await })
    };
    callback_with(&state, &headers, &query, transport).await
}

/// Version testable de [`start`] (le transport est fourni par l'appelant).
pub async fn start_with<F>(
    state: &AppState,
    headers: &HeaderMap,
    redirect: Option<&str>,
    mut transport: F,
) -> Response
where
    F: FnMut(Request) -> Answer,
{
    if !state.auth.is_enabled() {
        return auth::error(StatusCode::BAD_REQUEST, "auth_disabled");
    }
    let settings = match settings(state) {
        Ok(Some(settings)) => settings,
        Ok(None) => return auth::error(StatusCode::BAD_REQUEST, "oidc_disabled"),
        Err(manque) => return auth::internal(&describe(manque)),
    };

    let discovery = match discover(&state.auth, &settings, &mut transport).await {
        Ok(discovery) => discovery,
        Err(e) => return auth::internal(&e),
    };

    let redirect_uri = format!("{}{CALLBACK_PATH}", base_url(headers, &state.oidc));
    let etat = auth::new_token();
    let cible = return_to(redirect);
    state.auth.oidc.remember(
        &etat,
        Pending {
            redirect_uri: redirect_uri.clone(),
            return_to: cible,
            expires_at: db::now() + STATE_SECONDS,
        },
    );

    (
        StatusCode::FOUND,
        [(
            header::LOCATION,
            authorize_url(&settings, &discovery, &redirect_uri, &etat),
        )],
    )
        .into_response()
}

/// Version testable de [`callback`].
pub async fn callback_with<F>(
    state: &AppState,
    headers: &HeaderMap,
    query: &CallbackQuery,
    mut transport: F,
) -> Response
where
    F: FnMut(Request) -> Answer,
{
    if !state.auth.is_enabled() {
        return auth::error(StatusCode::BAD_REQUEST, "auth_disabled");
    }

    // Le fournisseur peut refuser la connexion (l'utilisateur a cliqué « non »,
    // ou la session chez lui a expiré) : on le dit à la page de connexion.
    if query.error.is_some() || query.code.is_none() {
        return redirect_to(&format!("{}?error=oidc_denied", auth::LOGIN_PATH));
    }

    // L'état est **la** protection contre le CSRF : sans lui, un retour forgé
    // ouvrirait une session à partir d'un code qui n'est pas le nôtre.
    let Some(pending) = query
        .state
        .as_deref()
        .and_then(|etat| state.auth.oidc.take(etat))
    else {
        return redirect_to(&format!("{}?error=invalid_state", auth::LOGIN_PATH));
    };

    let settings = match settings(state) {
        Ok(Some(settings)) => settings,
        Ok(None) => return auth::error(StatusCode::BAD_REQUEST, "oidc_disabled"),
        Err(manque) => return auth::internal(&describe(manque)),
    };
    let discovery = match discover(&state.auth, &settings, &mut transport).await {
        Ok(discovery) => discovery,
        Err(e) => return auth::internal(&e),
    };

    let code = query.code.clone().unwrap_or_default();
    let (status, body) = match transport(Request::Form {
        url: discovery.token_endpoint.clone(),
        body: token_form(&settings, &pending.redirect_uri, &code),
    })
    .await
    {
        Ok(reponse) => reponse,
        Err(e) => return auth::internal(&e),
    };
    if status >= 400 {
        return auth::internal(&format!("échange du code refusé ({status}) : {body}"));
    }
    let tokens = match parse_token_response(&body) {
        Ok(tokens) => tokens,
        Err(e) => return auth::internal(&e),
    };

    let (status, body) = match transport(Request::Get {
        url: discovery.userinfo_endpoint.clone(),
        bearer: Some(tokens.access_token),
    })
    .await
    {
        Ok(reponse) => reponse,
        Err(e) => return auth::internal(&e),
    };
    if status >= 400 {
        return auth::internal(&format!("userinfo refusé ({status}) : {body}"));
    }
    let identite = match parse_identity(&body) {
        Ok(identite) => identite,
        Err(code) => {
            state.auth.log(headers, "sso_refused", None, "", &code);
            return login_error(&code);
        }
    };

    // Provisionnement puis session : à partir d'ici, tout échec est un refus
    // (code) ou une panne (500), jamais une session à moitié ouverte.
    //
    // Le rattachement **ferme les sessions** du compte (sauf celles d'un
    // administrateur) : on note donc si le compte était déjà connu, pour couper
    // aussi ses WebSocket le cas échéant — la fermeture des sessions ne les
    // atteint pas d'elle-même.
    let deja_rattache = state
        .auth
        .db(|conn| db::find_by_subject(conn, &identite.subject))
        .ok()
        .flatten()
        .is_some();
    let user = match provision(&state.auth, &settings, &identite) {
        Ok(user) => user,
        Err(code) => {
            // L'adresse **essayée** est notée : c'est ce qui permet de répondre
            // à « pourquoi cette personne n'arrive pas à entrer » sans qu'elle
            // ait à raconter son parcours.
            state
                .auth
                .log(headers, "sso_refused", None, &identite.email, &code);
            return login_error(&code);
        }
    };
    if !deja_rattache {
        // Premier login (création ou rattachement) : les sessions antérieures du
        // compte ont pu être fermées par le rattachement.
        state.kick_user(&user.uuid);
    }
    let token =
        match state
            .auth
            .start_sso_session(&user.uuid, headers, tokens.refresh_token.as_deref())
        {
            Ok(token) => token,
            Err(e) => return auth::internal(&e),
        };
    state.auth.log(
        headers,
        "sso_login",
        Some(&user.uuid),
        &user.email,
        settings.provisioning.id(),
    );
    println!(
        "SSO : {} ({}) connecté — provisionnement {}",
        user.username,
        user.email,
        settings.provisioning.id()
    );

    (
        StatusCode::FOUND,
        [
            (header::LOCATION, pending.return_to.clone()),
            (
                header::SET_COOKIE,
                state.auth.session_cookie(&token, headers),
            ),
        ],
    )
        .into_response()
}

/// Redirection vers une page de l'interface.
fn redirect_to(url: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, url.to_string())]).into_response()
}

// ── Revalidation des sessions ouvertes par le fournisseur ────────────────

/// Résultat d'un balayage, en **chiffres** (le journal et la console les
/// affichent ; personne n'a besoin d'une phrase construite ici).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sweep {
    /// Sessions dont l'échéance de vérification était passée.
    pub due: usize,
    /// Sessions confirmées par le fournisseur (et rafraîchies).
    pub confirmed: usize,
    /// Sessions refusées : compte fermé localement.
    pub revoked: usize,
    /// Fournisseur injoignable : on n'a **rien** touché (voir plus bas).
    pub unreachable: usize,
}

/// Revérifie auprès du fournisseur les sessions ouvertes par le SSO.
///
/// C'est ce qui manquait à l'exigence « désactivé chez le fournisseur ⇒ compte
/// fermé ici » : sans elle, une session locale survivait jusqu'à **trente
/// jours**, quelle que soit la décision de l'annuaire. Chaque session vérifiée
/// porte son `refresh_token`, obtenu au login avec le secret client — le
/// fournisseur ne répond donc que si l'accès existe encore.
///
/// **Fail-open** dès qu'on ne sait pas : fournisseur injoignable, réponse 5xx,
/// corps illisible → la session est laissée en place et l'échéance repoussée
/// (pour ne pas marteler un fournisseur en panne). Seul un **refus explicite**
/// (4xx) ferme quelque chose : c'est le fournisseur qui parle, et il dit non.
///
/// Un **administrateur** rattaché au SSO n'est pas désactivé : ses sessions
/// tombent, mais son compte reste ouvert, car son mot de passe local est la
/// porte de secours de l'installation (voir `auth::login`). Le détail du journal
/// le dit (`admin`).
pub async fn revalidate_sessions(state: &AppState) -> Result<Sweep, String> {
    let client = client()?;
    let transport = move |request: Request| -> Answer {
        let client = client.clone();
        Box::pin(async move { send(&client, request).await })
    };
    revalidate_sessions_with(state, transport).await
}

/// Version testable de [`revalidate_sessions`] (transport fourni).
pub async fn revalidate_sessions_with<F>(
    state: &AppState,
    mut transport: F,
) -> Result<Sweep, String>
where
    F: FnMut(Request) -> Answer,
{
    if !state.auth.is_enabled() {
        return Ok(Sweep::default());
    }
    let settings = match settings(state) {
        Ok(Some(settings)) => settings,
        // SSO éteint : rien à vérifier, et ce n'est pas une erreur.
        Ok(None) => return Ok(Sweep::default()),
        Err(manque) => return Err(describe(manque)),
    };

    let avant = db::now() - SSO_CHECK_SECONDS;
    let sessions = state
        .auth
        .db(|conn| db::sso_sessions_to_check(conn, avant))?;
    let mut sweep = Sweep {
        due: sessions.len(),
        ..Sweep::default()
    };
    if sessions.is_empty() {
        return Ok(sweep);
    }

    // La découverte peut échouer (fournisseur en panne) : on rend l'erreur
    // **sans rien toucher**. Fermer les sessions parce que le fournisseur ne
    // répond pas mettre tout le monde dehors au premier incident réseau.
    let discovery = discover(&state.auth, &settings, &mut transport).await?;

    for (session_uuid, user_uuid, refresh) in sessions {
        let (status, body) = match transport(Request::Form {
            url: discovery.token_endpoint.clone(),
            body: refresh_form(&settings, &refresh),
        })
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                // Réseau injoignable : on ne touche à rien, on réessaiera.
                sweep.unreachable += 1;
                println!("SSO (revalidation) : fournisseur injoignable ({e})");
                let _ = state
                    .auth
                    .db(|conn| db::mark_sso_checked(conn, &session_uuid, db::now(), None));
                continue;
            }
        };

        if status >= 500 {
            // Panne du fournisseur : ce n'est pas une décision, on réessaiera.
            sweep.unreachable += 1;
            let _ = state
                .auth
                .db(|conn| db::mark_sso_checked(conn, &session_uuid, db::now(), None));
            continue;
        }
        if status >= 400 {
            // Toutes les erreurs 4xx ne disent pas la même chose. Celles qui
            // parlent de **nous** (`invalid_client`, `unsupported_grant_type`)
            // signalent une configuration à corriger, pas un compte à fermer :
            // désactiver les comptes sur cette base serait une panne que nous
            // aurions fabriquée. Seul un refus qui vise l'**accès** compte.
            match refresh_refusal(&body) {
                Some(code) => {
                    sweep.revoked += 1;
                    close_sso_access(state, &user_uuid, &format!("refresh_refused:{code}"), &body);
                }
                None => {
                    sweep.unreachable += 1;
                    let apercu: String = body.chars().take(120).collect();
                    println!(
                        "SSO (revalidation) : réponse {status} sans refus d'accès ({apercu}) \
                         — aucun compte touché"
                    );
                    let _ = state
                        .auth
                        .db(|conn| db::mark_sso_checked(conn, &session_uuid, db::now(), None));
                }
            }
            continue;
        }

        // Le fournisseur peut avoir **fait tourner** le jeton de rafraîchissement :
        // garder l'ancien ferait refuser la vérification suivante.
        let nouveau = parse_token_response(&body)
            .ok()
            .and_then(|tokens| tokens.refresh_token);
        state
            .auth
            .db(|conn| db::mark_sso_checked(conn, &session_uuid, db::now(), nouveau.as_deref()))?;
        sweep.confirmed += 1;
    }

    Ok(sweep)
}

/// Erreurs d'un rafraîchissement qui veulent dire « cet accès n'existe plus ».
///
/// Liste courte et **explicite**, parce que les deux camps ne se mélangent pas :
/// `invalid_grant` vient du fournisseur (« ce jeton est révoqué, ce compte est
/// fermé ») ; `invalid_client` ou `unsupported_grant_type` viennent de *notre*
/// configuration. Ce qui n'est pas dans cette liste est traité comme une panne
/// (fail-open) et ne ferme aucun compte — un fournisseur tatillon ne doit pas
/// pouvoir vider l'installation.
const REVOCATION_CODES: &[&str] = &[
    "invalid_grant",
    "invalid_token",
    "expired_token",
    "access_denied",
];

/// Code de refus **d'accès** porté par une réponse d'erreur, s'il y en a un.
pub fn refresh_refusal(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let code = value.get("error")?.as_str()?.trim().to_string();
    REVOCATION_CODES.contains(&code.as_str()).then_some(code)
}

/// Ferme ce qui vient du fournisseur : **toutes** les sessions du compte (et le
/// compte lui-même, sauf pour un administrateur).
///
/// Le compte est désactivé, pas seulement déconnecté : c'est l'exigence —
/// « fermé chez le fournisseur, fermé ici ». Le laisser actif avec ses sessions
/// fermées n'y répondrait qu'à moitié : il suffirait d'attendre une nouvelle
/// connexion SSO… que le fournisseur refuserait, certes, mais le mot de passe
/// local, lui, reste valable (il est conservé pour le détachement).
fn close_sso_access(state: &AppState, user_uuid: &str, detail: &str, body: &str) {
    let admin = state.auth.is_admin_uuid(user_uuid).unwrap_or(false);
    let nom = state
        .auth
        .db(|conn| db::find_by_uuid(conn, user_uuid))
        .ok()
        .flatten()
        .map(|user| user.username)
        .unwrap_or_default();

    let resultat = state.auth.db(|conn| {
        db::delete_sessions_of(conn, user_uuid)?;
        if !admin {
            db::set_disabled(conn, user_uuid, true)?;
        }
        Ok(())
    });
    if let Err(e) = resultat {
        eprintln!("⚠️  SSO (revalidation) : {e}");
    }

    // Le détail porte la décision du fournisseur, tronquée : un corps d'erreur
    // OIDC est court (`invalid_grant`), mais rien n'oblige un fournisseur à
    // rester bref, et le journal n'est pas une archive de réponses HTTP.
    let corps: String = body.chars().take(120).collect();
    let detail = if admin {
        format!("{detail},admin:{corps}")
    } else {
        format!("{detail}:{corps}")
    };
    state.auth.log(
        &HeaderMap::new(),
        "sso_revoked",
        Some(user_uuid),
        &nom,
        &detail,
    );
    println!(
        "SSO (revalidation) : accès refusé par le fournisseur pour {nom} — sessions fermées{}",
        if admin {
            " (administrateur : compte laissé actif, porte de secours)"
        } else {
            " et compte désactivé"
        }
    );
    // Les WebSocket du compte tombent : leurs sessions n'existent plus (voir
    // `api::handle_ws`).
    state.kick_user(user_uuid);
}

/// Refus **destiné à l'utilisateur** : la page de connexion sait les expliquer.
///
/// Les autres codes sont traduits en message générique : mieux vaut « la
/// connexion a échoué » qu'un code interne sur une page publique.
fn login_error(code: &str) -> Response {
    if is_client_code(code) {
        return redirect_to(&format!("{}?error={code}", auth::LOGIN_PATH));
    }
    auth::internal(&format!("connexion SSO refusée : {code}"))
}

/// Base publique de l'interface : `EASY3D_PUBLIC_URL`, sinon l'en-tête `Host`.
///
/// Le `redirect_uri` doit être **exactement** celui que le navigateur visitera
/// (`/api/auth/oidc/callback`), donc l'adresse vue par l'utilisateur — pas celle
/// que le backend voit du relais Nitro, qui est `127.0.0.1:8090`.
pub fn base_url(headers: &HeaderMap, env: &Env) -> String {
    if let Some(url) = env
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        return url.trim_end_matches('/').to_string();
    }
    let proto = header_value(headers, "x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    let host = header_value(headers, "x-forwarded-host")
        .or_else(|| header_value(headers, "host"))
        .unwrap_or_default();
    format!("{proto}://{host}")
}

/// Chemin interne demandé avant la connexion, ou l'accueil.
///
/// Seuls les chemins internes sont acceptés : accepter une adresse complète
/// ferait de cette route un tremplin pour rediriger un utilisateur ailleurs
/// après une connexion réussie (même règle que `safeRedirect` côté interface).
fn return_to(redirect: Option<&str>) -> String {
    match redirect.map(str::trim) {
        Some(path) if path.starts_with('/') && !path.starts_with("//") => path.to_string(),
        _ => "/".to_string(),
    }
}

// ── Petits utilitaires ──────────────────────────────────────────────────

/// Variable d'environnement non vide, espaces retirés.
fn non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Champ facultatif nettoyé.
fn non_empty_of(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Valeur d'un en-tête, nettoyée.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Échappement d'une valeur de requête (équivalent de `encodeURIComponent`).
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            autre => out.push_str(&format!("%{autre:02X}")),
        }
    }
    out
}

/// Corps `application/x-www-form-urlencoded`.
fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(cle, valeur)| format!("{}={}", encode(cle), encode(valeur)))
        .collect::<Vec<_>>()
        .join("&")
}

// ── Ce que l'interface a besoin de savoir ───────────────────────────────

/// Ce que `GET /auth/me` annonce du SSO.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// Nom lisible du fournisseur, pour le bouton de la page de connexion.
    ///
    /// On ne prend que son **hôte** : c'est ce qui distingue deux fournisseurs
    /// (une adresse complète ne tiendrait pas sur un bouton), et le backend n'a
    /// pas d'identité de marque à afficher dont il serait sûr.
    pub label: String,
}

/// État du SSO pour l'interface : `None` s'il est éteint ou mal configuré.
pub fn status(state: &AppState) -> Option<Status> {
    if !state.auth.is_enabled() {
        return None;
    }
    settings(state).ok().flatten().map(|settings| Status {
        label: issuer_label(&settings.issuer),
    })
}

/// Hôte d'un émetteur (`https://sso.exemple.fr/realms/moi` → `sso.exemple.fr`).
fn issuer_label(issuer: &str) -> String {
    let sans_schema = issuer.split_once("://").map_or(issuer, |(_, reste)| reste);
    sans_schema
        .split('/')
        .next()
        .filter(|hote| !hote.is_empty())
        .unwrap_or(issuer)
        .to_string()
}

/// Réponse de `GET /auth/oidc/check`.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
}

/// `GET /auth/oidc/check` — vérifie que le fournisseur répond.
///
/// Une adresse d'émetteur mal tapée ne se voit qu'au moment où quelqu'un essaie
/// de se connecter : c'est trop tard, et le message vient alors du fournisseur,
/// dans une page que l'on ne contrôle pas. Ce contrôle-ci, déclenché par
/// l'administration, fait la découverte **tout de suite** et rapporte les
/// adresses trouvées.
///
/// Réservé à qui peut écrire la configuration (`config.write`) : la requête
/// part vers l'adresse enregistrée, et rien ne justifie qu'un simple lecteur
/// puisse la déclencher.
pub async fn check(State(state): State<AppState>) -> Response {
    let Ok(client) = client() else {
        return auth::internal("client HTTP indisponible");
    };
    let transport = move |request: Request| -> Answer {
        let client = client.clone();
        Box::pin(async move { send(&client, request).await })
    };
    check_with(&state, transport).await
}

/// Version testable de [`check`].
pub async fn check_with<F>(state: &AppState, mut transport: F) -> Response
where
    F: FnMut(Request) -> Answer,
{
    let settings = match settings(state) {
        Ok(Some(settings)) => settings,
        Ok(None) => return auth::error(StatusCode::BAD_REQUEST, "oidc_disabled"),
        Err(manque) => return auth::error(StatusCode::BAD_REQUEST, &describe(manque)),
    };

    // On force une découverte fraîche : le bouton sert justement à vérifier une
    // adresse que l'on vient de changer.
    state.auth.oidc.forget_discovery();
    match discover(&state.auth, &settings, &mut transport).await {
        Ok(discovery) => Json(CheckResponse {
            issuer: settings.issuer,
            authorization_endpoint: discovery.authorization_endpoint,
            token_endpoint: discovery.token_endpoint,
            userinfo_endpoint: discovery.userinfo_endpoint,
        })
        .into_response(),
        Err(e) => auth::error(StatusCode::BAD_GATEWAY, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{password, permissions};
    use std::sync::{Arc, Mutex as StdMutex};

    // ── Outils ──────────────────────────────────────────────────────────

    fn reglages(mode: Provisioning) -> Settings {
        Settings {
            issuer: "https://sso.exemple.fr".to_string(),
            client_id: "easy3d".to_string(),
            client_secret: "secret".to_string(),
            scopes: vec!["openid".to_string(), "email".to_string()],
            provisioning: mode,
        }
    }

    fn decouverte() -> Discovery {
        Discovery {
            issuer: "https://sso.exemple.fr".to_string(),
            authorization_endpoint: "https://sso.exemple.fr/auth".to_string(),
            token_endpoint: "https://sso.exemple.fr/token".to_string(),
            userinfo_endpoint: "https://sso.exemple.fr/userinfo".to_string(),
        }
    }

    fn decouverte_json() -> String {
        r#"{"issuer":"https://sso.exemple.fr",
            "authorization_endpoint":"https://sso.exemple.fr/auth",
            "token_endpoint":"https://sso.exemple.fr/token",
            "userinfo_endpoint":"https://sso.exemple.fr/userinfo"}"#
            .to_string()
    }

    /// Base de comptes seule (pour le provisionnement, qui n'a pas besoin d'API).
    fn base() -> (tempfile::TempDir, Auth) {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
        auth.db(db::ensure_builtin_roles).unwrap();
        (dir, auth)
    }

    fn identite(email: &str) -> Identity {
        Identity {
            subject: format!("sub-{email}"),
            email: email.to_string(),
            username: email.split('@').next().unwrap_or("utilisateur").to_string(),
        }
    }

    fn config_sso(mode: Provisioning) -> Oidc {
        Oidc {
            enabled: true,
            issuer: Some("https://sso.exemple.fr".to_string()),
            client_id: Some("easy3d".to_string()),
            client_secret: Some("secret".to_string()),
            provisioning: mode,
            ..Oidc::default()
        }
    }

    /// État complet, SSO allumé, découverte **déjà en cache** : aucun appel au
    /// fournisseur n'est nécessaire pour démarrer un flux.
    fn etat(mode: Provisioning, public_url: bool) -> AppState {
        let (dir, auth) = base();
        // Le dossier doit survivre **à cette fonction**, puisque c'est lui qui
        // porte la base que l'état renvoyé va interroger. Un `TempDir` détruit à
        // la sortie emporte le fichier : sous Unix la suppression réussit, la
        // base n'a plus de dossier où écrire son journal et SQLite répond
        // « attempt to write a readonly database » ; sous Windows la suppression
        // échoue (le fichier est ouvert), ce qui masquait le défaut en local et
        // le faisait apparaître en CI. `keep` laisse donc le dossier en place —
        // le chemin qu'il rend ne sert pas ici, et son abandon se dit `let _ =`
        // (la CI refuse les warnings).
        let _ = dir.keep();
        let mut state = AppState::new(
            std::env::temp_dir(),
            tokio::sync::broadcast::channel(4).0,
            crate::config::Config {
                oidc: config_sso(mode),
                ..crate::config::Config::default()
            },
        )
        .with_auth(auth);
        if public_url {
            state = state.with_oidc(Env {
                public_url: Some("https://easy3d.exemple.fr".to_string()),
                ..Env::empty()
            });
        }
        state
            .auth
            .oidc
            .remember_discovery("https://sso.exemple.fr", &decouverte());
        state
    }

    /// Réponses scriptées : la *n*ᵉ requête reçoit la *n*ᵉ réponse, et tout ce
    /// qui a été demandé est conservé.
    type Journal = Arc<StdMutex<Vec<Request>>>;

    fn transport(reponses: Vec<Raw>) -> (impl FnMut(Request) -> Answer, Journal) {
        let journal: Journal = Arc::new(StdMutex::new(Vec::new()));
        let vues = journal.clone();
        let mut rang = 0usize;
        let transport = move |request: Request| -> Answer {
            vues.lock().unwrap().push(request);
            let reponse = reponses
                .get(rang)
                .cloned()
                .unwrap_or_else(|| Err("requête inattendue".to_string()));
            rang += 1;
            Box::pin(async move { reponse })
        };
        (transport, journal)
    }

    /// Réponses d'un retour de fournisseur réussi.
    fn reponses_du_retour(email: &str) -> Vec<Raw> {
        vec![
            Ok((200, r#"{"access_token":"jeton"}"#.to_string())),
            Ok((
                200,
                format!(r#"{{"sub":"sub-{email}","email":"{email}","preferred_username":"remi"}}"#),
            )),
        ]
    }

    /// Démarre un flux et rend l'`state` à rejouer au retour.
    async fn demarrer(state: &AppState, headers: &HeaderMap, redirect: Option<&str>) -> String {
        let (faux, journal) = transport(vec![]);
        let reponse = start_with(state, headers, redirect, faux).await;
        assert_eq!(reponse.status(), StatusCode::FOUND);
        assert!(
            journal.lock().unwrap().is_empty(),
            "la découverte était en cache : aucun appel ne devait partir"
        );
        let location = reponse
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        location
            .split("state=")
            .nth(1)
            .and_then(|reste| reste.split('&').next())
            .expect("state absent de l'URL d'autorisation")
            .to_string()
    }

    fn entetes() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "interne:8090".parse().unwrap());
        headers
    }

    // ── Découverte et adresses ──────────────────────────────────────────

    #[test]
    fn l_url_d_autorisation_porte_tout_ce_qu_il_faut() {
        let url = authorize_url(
            &reglages(Provisioning::Auto),
            &decouverte(),
            "https://ex.fr/cb",
            "abc",
        );
        assert!(url.starts_with("https://sso.exemple.fr/auth?"), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("client_id=easy3d"), "{url}");
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fex.fr%2Fcb"),
            "{url}"
        );
        assert!(url.contains("scope=openid%20email"), "{url}");
        assert!(url.contains("state=abc"), "{url}");
    }

    #[test]
    fn l_url_d_autorisation_respecte_une_adresse_qui_a_deja_des_parametres() {
        let mut decouverte = decouverte();
        decouverte.authorization_endpoint = "https://sso.exemple.fr/auth?tenant=1".to_string();
        let url = authorize_url(
            &reglages(Provisioning::Auto),
            &decouverte,
            "https://ex.fr/cb",
            "abc",
        );
        assert!(url.contains("?tenant=1&response_type=code"), "{url}");
        assert!(!url.contains("?tenant=1?response_type"), "{url}");
    }

    #[test]
    fn la_decouverte_est_lue_et_les_adresses_manquantes_refusees() {
        assert_eq!(parse_discovery(&decouverte_json()).unwrap(), decouverte());
        assert!(parse_discovery("{}").is_err());
        assert!(parse_discovery("pas du json").is_err());
        // Un champ présent mais vide ne compte pas comme une adresse.
        assert!(
            parse_discovery(
                r#"{"authorization_endpoint":"","token_endpoint":"a","userinfo_endpoint":"b"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn l_identite_exige_un_sub_et_une_adresse() {
        let ok = parse_identity(
            r#"{"sub":"42","email":"Remi@Exemple.FR","preferred_username":"Rémi Petit"}"#,
        )
        .unwrap();
        assert_eq!(ok.subject, "42");
        // L'adresse est normalisée (la base compare sans tenir compte de la
        // casse) et le nom d'utilisateur reste acceptable en URL.
        assert_eq!(ok.email, "remi@exemple.fr");
        assert_eq!(ok.username, "r-mi-petit");

        assert!(parse_identity(r#"{"email":"a@b.c"}"#).is_err());
        assert_eq!(
            parse_identity(r#"{"sub":"42"}"#).unwrap_err(),
            "oidc_no_email"
        );
        // Sans `preferred_username`, on prend la partie locale de l'adresse.
        assert_eq!(
            parse_identity(r#"{"sub":"42","email":"remi@exemple.fr"}"#)
                .unwrap()
                .username,
            "remi"
        );
    }

    // ── Réglages : environnement d'abord ────────────────────────────────

    #[test]
    fn les_reglages_viennent_de_l_environnement_quand_il_est_pose() {
        let config = config_sso(Provisioning::Manual);

        // Sans environnement : le fichier seul.
        let seul = resolve(&config, &Env::empty()).unwrap().unwrap();
        assert_eq!(seul.issuer, "https://sso.exemple.fr");
        assert_eq!(seul.provisioning, Provisioning::Manual);
        assert_eq!(seul.scopes, vec!["openid", "email", "profile"]);

        // Avec environnement : il gagne, champ par champ.
        let env = Env {
            issuer: Some("https://env.exemple.fr".to_string()),
            client_secret: Some("secret-env".to_string()),
            provisioning: Some(Provisioning::Approval),
            ..Env::empty()
        };
        let melange = resolve(&config, &env).unwrap().unwrap();
        assert_eq!(melange.issuer, "https://env.exemple.fr");
        assert_eq!(melange.client_id, "easy3d");
        assert_eq!(melange.client_secret, "secret-env");
        assert_eq!(melange.provisioning, Provisioning::Approval);
        assert!(env.locked().contains(&"enabled"));
        assert!(env.locked().contains(&"issuer"));
        assert!(!env.locked().contains(&"client_id"));

        // Éteint : rien de tout ça ne compte.
        assert!(resolve(&Oidc::default(), &Env::empty()).unwrap().is_none());

        // Un environnement seul suffit à allumer le SSO, même si le fichier dit
        // `enabled: false` (le compose qui pose le secret a le dernier mot).
        let env_complet = Env {
            issuer: Some("https://env.exemple.fr".to_string()),
            client_id: Some("easy3d".to_string()),
            client_secret: Some("secret".to_string()),
            ..Env::empty()
        };
        assert!(resolve(&Oidc::default(), &env_complet).unwrap().is_some());

        // Activé mais incomplet : une erreur typée, pas un « éteint » silencieux.
        let incomplet = Oidc {
            enabled: true,
            issuer: Some("https://sso.exemple.fr".to_string()),
            ..Oidc::default()
        };
        assert_eq!(
            resolve(&incomplet, &Env::empty()).unwrap_err(),
            Missing::ClientId
        );
        assert!(describe(Missing::ClientId).contains("EASY3D_OIDC_CLIENT_ID"));
    }

    #[test]
    fn le_secret_client_suit_la_convention_du_masque() {
        let config = Oidc {
            client_secret: Some("vrai-secret".to_string()),
            ..Oidc::default()
        };
        assert_eq!(
            config.redacted().client_secret.as_deref(),
            Some(crate::config::KEY_PLACEHOLDER)
        );
        // Le masque veut dire « garde celui que tu as », la chaîne vide
        // « efface-le ».
        assert_eq!(
            config.merge_secret(Some(crate::config::KEY_PLACEHOLDER)),
            Some("vrai-secret".to_string())
        );
        assert_eq!(config.merge_secret(None), Some("vrai-secret".to_string()));
        assert_eq!(config.merge_secret(Some("")), None);
        assert_eq!(config.merge_secret(Some("neuf")), Some("neuf".to_string()));
    }

    // ── Provisionnement ─────────────────────────────────────────────────

    #[test]
    fn le_provisionnement_auto_cree_le_compte_avec_le_role_par_defaut() {
        let (_dir, auth) = base();
        auth.db(|conn| db::set_setting(conn, db::DEFAULT_ROLE, Some(db::READER_ROLE)))
            .unwrap();

        let user = provision(
            &auth,
            &reglages(Provisioning::Auto),
            &identite("neuf@exemple.fr"),
        )
        .unwrap();
        assert_eq!(user.email, "neuf@exemple.fr");
        assert_eq!(user.username, "neuf");
        assert_eq!(user.oidc_subject.as_deref(), Some("sub-neuf@exemple.fr"));
        // Sans mot de passe : ce compte ne se connecte que par le fournisseur.
        assert!(user.password_hash.is_none());
        assert_eq!(
            auth.permissions_of(&user).unwrap(),
            vec![permissions::CATALOG_READ.to_string()]
        );

        // Deuxième connexion : le même compte, **sans doublon**, même si
        // l'adresse a changé chez le fournisseur (le `sub` fait foi).
        let autre = Identity {
            subject: "sub-neuf@exemple.fr".to_string(),
            email: "neuf@ailleurs.fr".to_string(),
            username: "neuf".to_string(),
        };
        let relu = provision(&auth, &reglages(Provisioning::Auto), &autre).unwrap();
        assert_eq!(relu.uuid, user.uuid);
        assert_eq!(
            auth.db(db::list_users).unwrap().len(),
            1,
            "un second compte ne doit pas être créé"
        );
    }

    #[test]
    fn le_provisionnement_manuel_rattache_par_adresse_et_refuse_le_reste() {
        let (_dir, auth) = base();
        let prepare = db::NewUser::new(
            "remi",
            "remi@exemple.fr",
            Some(password::hash("motdepasse").unwrap()),
        );
        auth.db(|conn| db::insert_user(conn, &prepare)).unwrap();

        // Connu par son adresse : rattaché, et le `sub` est retenu.
        let user = provision(
            &auth,
            &reglages(Provisioning::Manual),
            &identite("remi@exemple.fr"),
        )
        .unwrap();
        assert_eq!(user.uuid, prepare.uuid);
        assert_eq!(user.oidc_subject.as_deref(), Some("sub-remi@exemple.fr"));
        // **Le rattachement ne détruit rien** : l'empreinte reste en base. Ce qui
        // change, c'est l'**entrée** — `auth::login` refuse désormais le mot de
        // passe d'un compte rattaché (règle non destructive du 2026-10-08), et le
        // mot de passe resservira si le compte est détaché.
        assert!(user.password_hash.is_some());

        // Inconnu : refusé, avec un code que la page de connexion sait expliquer.
        let refus = provision(
            &auth,
            &reglages(Provisioning::Manual),
            &identite("inconnu@exemple.fr"),
        )
        .unwrap_err();
        assert_eq!(refus, "oidc_not_provisioned");
        assert_eq!(auth.db(db::list_users).unwrap().len(), 1);

        // Un compte déjà rattaché à une **autre** identité est refusé : sans
        // cela, qui contrôle le second fournisseur prendrait le compte du
        // premier.
        let autre = Identity {
            subject: "sub-d-un-autre".to_string(),
            email: "remi@exemple.fr".to_string(),
            username: "remi".to_string(),
        };
        assert_eq!(
            provision(&auth, &reglages(Provisioning::Manual), &autre).unwrap_err(),
            "oidc_conflict"
        );
    }

    /// Le rattachement **ferme les sessions ouvertes** (l'entrée change de
    /// nature) mais épargne celles d'un administrateur — comme son mot de passe :
    /// c'est la porte de secours de l'installation, si le fournisseur est en
    /// panne.
    #[test]
    fn le_rattachement_ferme_les_sessions_sauf_celles_d_un_administrateur() {
        let (_dir, auth) = base();
        for (nom, admin) in [("ouvrier", false), ("chef", true)] {
            let prepare = db::NewUser::new(
                nom,
                &format!("{nom}@exemple.fr"),
                Some(password::hash("motdepasse").unwrap()),
            );
            auth.db(|conn| db::insert_user(conn, &prepare)).unwrap();
            if admin {
                auth.db(|conn| {
                    db::set_user_roles(conn, &prepare.uuid, &[db::ADMIN_ROLE.to_string()])
                })
                .unwrap();
            }
            let jeton = auth
                .start_session(&prepare.uuid, &axum::http::HeaderMap::new())
                .unwrap();

            provision(
                &auth,
                &reglages(Provisioning::Manual),
                &identite(&format!("{nom}@exemple.fr")),
            )
            .unwrap();

            let vivante = auth
                .db(|conn| db::session_of(conn, &crate::auth::hash_secret(&jeton), db::now()))
                .unwrap()
                .is_some();
            assert_eq!(vivante, admin, "session de {nom} après rattachement");

            // Et le mot de passe est conservé, dans les deux cas.
            let relu = auth
                .db(|conn| db::find_by_uuid(conn, &prepare.uuid))
                .unwrap()
                .unwrap();
            assert!(relu.password_hash.is_some(), "mot de passe de {nom}");
        }
    }

    #[test]
    fn le_provisionnement_par_validation_cree_un_compte_desactive() {
        let (_dir, auth) = base();
        let code = provision(
            &auth,
            &reglages(Provisioning::Approval),
            &identite("nouveau@exemple.fr"),
        )
        .unwrap_err();
        assert_eq!(code, "pending_approval");

        // Le compte existe bel et bien : la page Comptes sert d'écran de
        // validation, il ne reste qu'à le réactiver.
        let users = auth.db(db::list_users).unwrap();
        assert_eq!(users.len(), 1);
        assert!(users[0].disabled);
        assert_eq!(
            users[0].oidc_subject.as_deref(),
            Some("sub-nouveau@exemple.fr")
        );

        // Tant qu'il est désactivé, le même `sub` est refusé…
        assert_eq!(
            provision(
                &auth,
                &reglages(Provisioning::Approval),
                &identite("nouveau@exemple.fr")
            )
            .unwrap_err(),
            "account_disabled"
        );
        // …et une fois validé, il entre.
        auth.db(|conn| db::set_disabled(conn, &users[0].uuid, false))
            .unwrap();
        assert!(
            provision(
                &auth,
                &reglages(Provisioning::Approval),
                &identite("nouveau@exemple.fr")
            )
            .is_ok()
        );
    }

    #[test]
    fn un_compte_desactive_ne_se_connecte_pas() {
        let (_dir, auth) = base();
        let user = provision(
            &auth,
            &reglages(Provisioning::Auto),
            &identite("remi@exemple.fr"),
        )
        .unwrap();
        auth.db(|conn| db::set_disabled(conn, &user.uuid, true))
            .unwrap();
        assert_eq!(
            provision(
                &auth,
                &reglages(Provisioning::Auto),
                &identite("remi@exemple.fr")
            )
            .unwrap_err(),
            "account_disabled"
        );
    }

    #[test]
    fn le_nom_d_utilisateur_est_rendu_unique() {
        let (_dir, auth) = base();
        let premier = provision(
            &auth,
            &reglages(Provisioning::Auto),
            &identite("remi@exemple.fr"),
        )
        .unwrap();
        assert_eq!(premier.username, "remi");
        // Même partie locale, autre adresse : la base refuserait le doublon,
        // donc on suffixe avant d'écrire.
        let second = provision(
            &auth,
            &reglages(Provisioning::Auto),
            &identite("remi@ailleurs.fr"),
        )
        .unwrap();
        assert_eq!(second.username, "remi-2");
        assert_ne!(second.uuid, premier.uuid);
    }

    // ── Flux complet ────────────────────────────────────────────────────

    #[tokio::test]
    async fn un_flux_complet_ouvre_une_session_et_revient_a_la_page_demandee() {
        let state = etat(Provisioning::Auto, true);
        let headers = entetes();
        let etat_courant = demarrer(&state, &headers, Some("/dossiers/Maison")).await;

        let (faux, journal) = transport(reponses_du_retour("remi@exemple.fr"));
        let query = CallbackQuery {
            code: Some("code-1".to_string()),
            state: Some(etat_courant),
            error: None,
        };
        let reponse = callback_with(&state, &headers, &query, faux).await;

        assert_eq!(reponse.status(), StatusCode::FOUND);
        assert_eq!(
            reponse.headers().get(header::LOCATION).unwrap(),
            "/dossiers/Maison"
        );
        let cookie = reponse
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(cookie.contains("easy3d_session="), "{cookie}");
        assert!(cookie.contains("HttpOnly"), "{cookie}");

        // La session ouverte est bien celle du compte provisionné, avec ses droits.
        let jeton = cookie
            .split("easy3d_session=")
            .nth(1)
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let mut entetes_session = HeaderMap::new();
        entetes_session.insert(
            header::COOKIE,
            format!("easy3d_session={jeton}").parse().unwrap(),
        );
        let connecte = state
            .auth
            .resolve(&entetes_session)
            .expect("session résolue");
        assert_eq!(connecte.email, "remi@exemple.fr");
        // Le compte créé à la volée reçoit le **rôle par défaut** de
        // l'installation (`lecteur` sur une base neuve) : il peut consulter le
        // catalogue dès sa première connexion.
        assert_eq!(
            state.auth.permissions_of(&connecte).unwrap(),
            vec![permissions::CATALOG_READ.to_string()]
        );

        // Ce qui a été demandé au fournisseur : l'échange du code, puis userinfo.
        let journal = journal.lock().unwrap().clone();
        assert_eq!(journal.len(), 2);
        match &journal[0] {
            Request::Form { url, body } => {
                assert_eq!(url, "https://sso.exemple.fr/token");
                assert!(body.contains("grant_type=authorization_code"), "{body}");
                assert!(body.contains("code=code-1"), "{body}");
                // Le `redirect_uri` de l'échange est **celui du début** du flux.
                assert!(
                    body.contains(&format!(
                        "redirect_uri={}",
                        encode("https://easy3d.exemple.fr/api/auth/oidc/callback")
                    )),
                    "{body}"
                );
            }
            autre => panic!("attendu : un POST de formulaire, obtenu {autre:?}"),
        }
        match &journal[1] {
            Request::Get { url, bearer } => {
                assert_eq!(url, "https://sso.exemple.fr/userinfo");
                assert_eq!(bearer.as_deref(), Some("jeton"));
            }
            autre => panic!("attendu : un GET userinfo, obtenu {autre:?}"),
        }

        // Le journal garde la trace de la connexion, avec le mode de
        // provisionnement qui a servi : c'est ce qu'on relit quand on se demande
        // « d'où vient ce compte ? ».
        let events = state.auth.db(|conn| db::list_events(conn, 5)).unwrap();
        assert_eq!(events.len(), 1, "un seul événement : {events:?}");
        assert_eq!(events[0].kind, "sso_login");
        assert_eq!(events[0].subject, "remi@exemple.fr");
        assert_eq!(events[0].detail, "auto");
        assert!(events[0].actor_uuid.is_some());
    }

    #[tokio::test]
    async fn un_refus_de_provisionnement_est_journalise() {
        let state = etat(Provisioning::Manual, true);
        let headers = entetes();
        let etat_courant = demarrer(&state, &headers, None).await;

        let (faux, _) = transport(reponses_du_retour("inconnu@exemple.fr"));
        let query = CallbackQuery {
            code: Some("code-1".to_string()),
            state: Some(etat_courant),
            error: None,
        };
        let reponse = callback_with(&state, &headers, &query, faux).await;
        assert_eq!(
            reponse.headers().get(header::LOCATION).unwrap(),
            "/login?error=oidc_not_provisioned"
        );

        // L'adresse **essayée** est au journal : c'est ce qui permet de répondre
        // à « pourquoi cette personne n'arrive pas à entrer ».
        let events = state.auth.db(|conn| db::list_events(conn, 5)).unwrap();
        assert_eq!(events[0].kind, "sso_refused");
        assert_eq!(events[0].subject, "inconnu@exemple.fr");
        assert_eq!(events[0].detail, "oidc_not_provisioned");
        assert!(events[0].actor_uuid.is_none(), "personne n'est entré");
    }

    #[tokio::test]
    async fn un_etat_rejoue_ne_rouvre_pas_de_session() {
        let state = etat(Provisioning::Auto, true);
        let headers = entetes();
        let etat_courant = demarrer(&state, &headers, None).await;
        let query = CallbackQuery {
            code: Some("code-1".to_string()),
            state: Some(etat_courant),
            error: None,
        };

        let (premier, _) = transport(reponses_du_retour("remi@exemple.fr"));
        let reponse = callback_with(&state, &headers, &query, premier).await;
        assert_eq!(reponse.headers().get(header::LOCATION).unwrap(), "/");

        // Le même retour, rejoué : l'état a été consommé, donc refus — et sans
        // rien redemander au fournisseur.
        let (second, journal) = transport(reponses_du_retour("remi@exemple.fr"));
        let reponse = callback_with(&state, &headers, &query, second).await;
        assert_eq!(
            reponse.headers().get(header::LOCATION).unwrap(),
            "/login?error=invalid_state"
        );
        assert!(journal.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn un_etat_inconnu_ou_un_refus_du_fournisseur_ne_donnent_pas_de_session() {
        let state = etat(Provisioning::Auto, true);
        let headers = entetes();

        let (faux, journal) = transport(vec![]);
        let etranger = CallbackQuery {
            code: Some("code-1".to_string()),
            state: Some("etat-invente".to_string()),
            error: None,
        };
        let reponse = callback_with(&state, &headers, &etranger, faux).await;
        assert_eq!(
            reponse.headers().get(header::LOCATION).unwrap(),
            "/login?error=invalid_state"
        );
        assert!(reponse.headers().get(header::SET_COOKIE).is_none());

        let (faux, _) = transport(vec![]);
        let refuse = CallbackQuery {
            code: None,
            state: None,
            error: Some("access_denied".to_string()),
        };
        let reponse = callback_with(&state, &headers, &refuse, faux).await;
        assert_eq!(
            reponse.headers().get(header::LOCATION).unwrap(),
            "/login?error=oidc_denied"
        );
        assert!(journal.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn une_panne_du_fournisseur_est_une_erreur_serveur_et_rien_d_autre() {
        let state = etat(Provisioning::Auto, true);
        let headers = entetes();
        let etat_courant = demarrer(&state, &headers, None).await;

        let (faux, _) = transport(vec![Ok((500, "boum".to_string()))]);
        let query = CallbackQuery {
            code: Some("code-1".to_string()),
            state: Some(etat_courant),
            error: None,
        };
        let reponse = callback_with(&state, &headers, &query, faux).await;
        assert_eq!(reponse.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(reponse.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn sans_public_url_le_retour_suit_l_en_tete_host() {
        let state = etat(Provisioning::Auto, false);
        let mut headers = entetes();
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        headers.insert("x-forwarded-host", "catalogue.exemple.fr".parse().unwrap());

        let (faux, _) = transport(vec![]);
        let reponse = start_with(&state, &headers, None, faux).await;
        let location = reponse
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            location.contains(&encode(
                "https://catalogue.exemple.fr/api/auth/oidc/callback"
            )),
            "{location}"
        );
        // Aucun `redirect` demandé : on revient à l'accueil, jamais ailleurs.
        let etat_courant = location.split("state=").nth(1).unwrap();
        let pending = state.auth.oidc.take(etat_courant).expect("état retenu");
        assert_eq!(pending.return_to, "/");
    }

    #[tokio::test]
    async fn le_sso_eteint_ne_demarre_rien() {
        let (_dir, auth) = base();
        let state = AppState::new(
            std::env::temp_dir(),
            tokio::sync::broadcast::channel(4).0,
            crate::config::Config::default(),
        )
        .with_auth(auth);
        let (faux, journal) = transport(vec![]);
        let reponse = start_with(&state, &entetes(), None, faux).await;
        assert_eq!(reponse.status(), StatusCode::BAD_REQUEST);
        assert!(journal.lock().unwrap().is_empty());

        // Et l'interface n'affiche même pas de bouton : aucun statut annoncé.
        assert!(status(&state).is_none());
    }

    #[tokio::test]
    async fn le_statut_annonce_l_hote_du_fournisseur() {
        let state = etat(Provisioning::Auto, true);
        assert_eq!(status(&state).unwrap().label, "sso.exemple.fr");
    }

    #[test]
    fn le_retour_reste_interne() {
        assert_eq!(return_to(Some("/dossiers/Maison")), "/dossiers/Maison");
        assert_eq!(return_to(None), "/");
        assert_eq!(return_to(Some("//exemple.fr")), "/");
        assert_eq!(return_to(Some("https://exemple.fr")), "/");
    }

    #[tokio::test]
    async fn le_controle_du_fournisseur_refait_la_decouverte() {
        let state = etat(Provisioning::Auto, true);

        // La découverte est en cache : le contrôle la refait **exprès**, sinon il
        // ne vérifierait qu'une réponse déjà connue.
        let (faux, journal) = transport(vec![Ok((200, decouverte_json()))]);
        let reponse = check_with(&state, faux).await;
        assert_eq!(reponse.status(), StatusCode::OK);
        assert_eq!(
            journal.lock().unwrap().len(),
            1,
            "la découverte doit être refaite"
        );
        // Et la réponse a été retenue : le flux qui suit n'ira pas la rechercher.
        assert!(
            state
                .auth
                .oidc
                .discovery_of("https://sso.exemple.fr")
                .is_some()
        );

        // Fournisseur muet ou illisible : un 502 motivé, jamais une page blanche.
        let (faux, _) = transport(vec![Ok((500, "boum".to_string()))]);
        assert_eq!(
            check_with(&state, faux).await.status(),
            StatusCode::BAD_GATEWAY
        );
        let (faux, _) = transport(vec![Ok((200, "pas du json".to_string()))]);
        assert_eq!(
            check_with(&state, faux).await.status(),
            StatusCode::BAD_GATEWAY
        );

        // Rien à contrôler quand le SSO est éteint.
        let (_dir, auth) = base();
        let eteint = AppState::new(
            std::env::temp_dir(),
            tokio::sync::broadcast::channel(4).0,
            crate::config::Config::default(),
        )
        .with_auth(auth);
        let (faux, _) = transport(vec![]);
        assert_eq!(
            check_with(&eteint, faux).await.status(),
            StatusCode::BAD_REQUEST
        );
    }

    // ── Émetteur de la découverte ────────────────────────────────────────

    /// Sans ce contrôle, une découverte détournée (DNS, proxy, émetteur mal
    /// recopié) enverrait le **secret client** et les jetons à quelqu'un
    /// d'autre, sans que rien ne le dise.
    #[test]
    fn la_decouverte_doit_annoncer_l_emetteur_configure() {
        // `issuer` est exigé par la spécification, et c'est ce qu'on compare.
        assert!(
            parse_discovery(
                r#"{"authorization_endpoint":"a","token_endpoint":"b","userinfo_endpoint":"c"}"#
            )
            .is_err()
        );
        assert_eq!(
            parse_discovery(&decouverte_json()).unwrap().issuer,
            "https://sso.exemple.fr"
        );

        assert!(same_issuer(
            "https://sso.exemple.fr",
            "https://sso.exemple.fr"
        ));
        // La barre oblique finale n'est pas une différence…
        assert!(same_issuer(
            "https://sso.exemple.fr/",
            "https://sso.exemple.fr"
        ));
        // …mais rien d'autre n'est toléré (ni la casse, ni un hôte voisin).
        assert!(!same_issuer(
            "https://SSO.exemple.fr",
            "https://sso.exemple.fr"
        ));
        assert!(!same_issuer(
            "https://sso.autre.fr",
            "https://sso.exemple.fr"
        ));
        assert!(!same_issuer(
            "https://sso.exemple.fr/autre",
            "https://sso.exemple.fr"
        ));
        assert!(!same_issuer("", "https://sso.exemple.fr"));
    }

    #[tokio::test]
    async fn une_decouverte_qui_annonce_un_autre_emetteur_arrete_le_flux() {
        let state = etat(Provisioning::Auto, false);
        // La découverte est en cache (voir `etat`) : on force une relecture.
        state.auth.oidc.forget_discovery();

        let (faux, journal) = transport(vec![Ok((
            200,
            r#"{"issuer":"https://attaquant.exemple.fr",
                "authorization_endpoint":"https://attaquant.exemple.fr/auth",
                "token_endpoint":"https://attaquant.exemple.fr/token",
                "userinfo_endpoint":"https://attaquant.exemple.fr/userinfo"}"#
                .to_string(),
        ))]);
        let reponse = start_with(&state, &entetes(), None, faux).await;

        // 500 : c'est une panne de configuration, pas un refus d'utilisateur, et
        // surtout **aucune redirection** vers l'adresse annoncée.
        assert_eq!(reponse.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            journal.lock().unwrap().len(),
            1,
            "la découverte, et rien d'autre"
        );
    }

    // ── Jetons rendus par le fournisseur ─────────────────────────────────

    #[test]
    fn les_jetons_lus_comprennent_le_rafraichissement_quand_il_est_la() {
        let sans = parse_token_response(r#"{"access_token":"a"}"#).unwrap();
        assert_eq!(sans.refresh_token, None);
        let avec = parse_token_response(r#"{"access_token":"a","refresh_token":"r"}"#).unwrap();
        assert_eq!(avec.refresh_token.as_deref(), Some("r"));
        // Un rafraîchissement vide ne vaut pas un rafraîchissement : le garder
        // tel quel ferait échouer la vérification suivante.
        let vide = parse_token_response(r#"{"access_token":"a","refresh_token":"  "}"#).unwrap();
        assert_eq!(vide.refresh_token, None);
        assert!(parse_token_response(r#"{"refresh_token":"r"}"#).is_err());

        let corps = refresh_form(&reglages(Provisioning::Auto), "r-1");
        assert!(
            corps.contains("grant_type=refresh_token"),
            "corps : {corps}"
        );
        assert!(corps.contains("refresh_token=r-1"), "corps : {corps}");
        assert!(corps.contains("client_secret=secret"), "corps : {corps}");
    }

    // ── Revalidation des sessions ouvertes par le fournisseur ────────────
    //
    // C'est l'exigence « fermé chez le fournisseur ⇒ fermé ici ». Sans elle, une
    // session locale vivait **trente jours** quoi qu'il arrive chez l'annuaire.

    /// Compte rattaché au fournisseur, avec une session SSO **à revérifier**.
    ///
    /// Le « à revérifier » vient d'un vieillissement de la session : une session
    /// fraîche n'interroge pas le fournisseur avant [`SSO_CHECK_SECONDS`], et un
    /// test ne peut pas attendre cinq minutes.
    fn session_sso(state: &AppState, email: &str, refresh: Option<&str>) -> (db::User, String) {
        let user = provision(&state.auth, &reglages(Provisioning::Auto), &identite(email)).unwrap();
        let jeton = state
            .auth
            .start_sso_session(&user.uuid, &entetes(), refresh)
            .unwrap();
        state
            .auth
            .db(|conn| {
                conn.execute("UPDATE sessions SET created_at = created_at - 10000", [])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        (user, jeton)
    }

    /// Le jeton de session est écrit **haché** : la base ne permet pas de rejouer
    /// une session.
    fn session_vivante(state: &AppState, jeton: &str) -> bool {
        state
            .auth
            .db(|conn| db::session_of(conn, &crate::auth::hash_secret(jeton), db::now()))
            .unwrap()
            .is_some()
    }

    fn compte_actif(state: &AppState, uuid: &str) -> bool {
        !state
            .auth
            .db(|conn| db::find_by_uuid(conn, uuid))
            .unwrap()
            .unwrap()
            .disabled
    }

    /// Date de la dernière vérification auprès du fournisseur, telle qu'elle est
    /// en base (`None` si la session n'a jamais été vérifiée).
    ///
    /// On regarde la **date** plutôt que « reste-t-il quelque chose à
    /// vérifier ? » : les horodatages sont en secondes, donc cette seconde
    /// question dépendrait du moment précis où le test s'exécute.
    fn derniere_verification(state: &AppState) -> Option<i64> {
        state
            .auth
            .db(|conn| {
                conn.query_row("SELECT sso_checked_at FROM sessions LIMIT 1", [], |row| {
                    row.get::<_, Option<i64>>(0)
                })
                .map_err(|e| e.to_string())
            })
            .unwrap()
    }

    #[tokio::test]
    async fn un_refus_du_fournisseur_ferme_les_sessions_et_desactive_le_compte() {
        let state = etat(Provisioning::Auto, false);
        let (user, jeton) = session_sso(&state, "remi@exemple.fr", Some("refresh-1"));

        let (faux, journal) =
            transport(vec![Ok((400, r#"{"error":"invalid_grant"}"#.to_string()))]);
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(
            sweep,
            Sweep {
                due: 1,
                confirmed: 0,
                revoked: 1,
                unreachable: 0,
            }
        );

        // La requête part en `refresh_token`, avec le secret client : c'est le
        // seul juge de « ce compte existe encore chez moi ».
        match &journal.lock().unwrap()[0] {
            Request::Form { body, .. } => {
                assert!(body.contains("grant_type=refresh_token"), "corps : {body}");
                assert!(body.contains("refresh_token=refresh-1"), "corps : {body}");
                assert!(body.contains("client_secret=secret"), "corps : {body}");
            }
            autre => panic!("requête inattendue : {autre:?}"),
        }

        // Le compte est **désactivé**, pas seulement déconnecté…
        assert!(!compte_actif(&state, &user.uuid));
        // …et la session locale ne vaut plus rien, même en présentant son jeton.
        assert!(!session_vivante(&state, &jeton));

        // Le journal le raconte, avec la décision et le corps tronqué.
        let evenements = crate::auth::journal::recent(&state, 10).unwrap();
        let refus = evenements
            .iter()
            .find(|e| e.kind == "sso_revoked")
            .expect("l'événement doit être journalisé");
        assert_eq!(refus.subject, "remi");
        assert!(
            refus.detail.starts_with("refresh_refused:invalid_grant"),
            "{refus:?}"
        );
    }

    /// Panne **n'est pas** décision : on ne met personne dehors parce que le
    /// fournisseur ne répond pas.
    #[tokio::test]
    async fn un_fournisseur_injoignable_ne_ferme_rien() {
        let state = etat(Provisioning::Auto, false);
        let (remi, jeton_remi) = session_sso(&state, "remi@exemple.fr", Some("refresh-1"));
        let (bob, jeton_bob) = session_sso(&state, "bob@exemple.fr", Some("refresh-2"));

        // Deux formes de panne : l'appel ne part pas, puis le fournisseur répond
        // 5xx. L'ordre des sessions n'est pas garanti, mais les deux mènent au
        // même résultat.
        let (faux, _) = transport(vec![
            Err("échec réseau".to_string()),
            Ok((503, "indisponible".to_string())),
        ]);
        let debut = db::now();
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(sweep.due, 2);
        assert_eq!(sweep.unreachable, 2);
        assert_eq!(sweep.revoked, 0);
        assert_eq!(sweep.confirmed, 0);

        // Rien n'a bougé : sessions valides, comptes actifs.
        assert!(session_vivante(&state, &jeton_remi));
        assert!(session_vivante(&state, &jeton_bob));
        assert!(compte_actif(&state, &remi.uuid));
        assert!(compte_actif(&state, &bob.uuid));

        // L'échéance a tout de même été repoussée : on ne martèle pas un
        // fournisseur en panne à chaque tour de balayage.
        assert!(
            derniere_verification(&state).is_some_and(|quand| quand >= debut),
            "la tentative doit être datée"
        );
    }

    #[tokio::test]
    async fn une_revalidation_reussie_repousse_l_echeance_et_garde_le_nouveau_jeton() {
        let state = etat(Provisioning::Auto, false);
        let (user, jeton) = session_sso(&state, "remi@exemple.fr", Some("refresh-1"));

        let (faux, journal) = transport(vec![Ok((
            200,
            r#"{"access_token":"j2","refresh_token":"refresh-2"}"#.to_string(),
        ))]);
        let debut = db::now();
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(sweep.confirmed, 1);
        assert_eq!(sweep.revoked, 0);
        assert_eq!(journal.lock().unwrap().len(), 1);

        // Le fournisseur a **fait tourner** le jeton : garder l'ancien ferait
        // refuser la vérification suivante.
        let enregistre: String = state
            .auth
            .db(|conn| {
                conn.query_row("SELECT refresh_token FROM sessions", [], |row| row.get(0))
                    .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(enregistre, "refresh-2");

        // La session locale est **la même** (on ne déconnecte personne), le
        // compte reste actif, et la vérification est datée.
        assert!(session_vivante(&state, &jeton));
        assert!(compte_actif(&state, &user.uuid));
        assert!(
            derniere_verification(&state).is_some_and(|quand| quand >= debut),
            "la vérification doit être datée"
        );
    }

    /// Un administrateur rattaché n'est **pas** désactivé : son mot de passe
    /// local est la porte de secours de l'installation si le fournisseur devient
    /// inutilisable (même exception que dans `auth::login`).
    #[tokio::test]
    async fn un_administrateur_rattache_garde_son_compte_quand_le_fournisseur_le_refuse() {
        let state = etat(Provisioning::Auto, false);
        let (user, jeton) = session_sso(&state, "chef@exemple.fr", Some("refresh-1"));
        state
            .auth
            .db(|conn| db::set_user_roles(conn, &user.uuid, &[db::ADMIN_ROLE.to_string()]))
            .unwrap();

        // Un refus d'**accès** : c'est le code qui le dit, pas le statut (un 401
        // peut aussi annoncer un `client_secret` périmé, qui n'est pas la faute
        // de l'utilisateur).
        let (faux, _) = transport(vec![Ok((401, r#"{"error":"invalid_token"}"#.to_string()))]);
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(sweep.revoked, 1);

        // Ses sessions tombent…
        assert!(!session_vivante(&state, &jeton));
        assert!(
            state
                .auth
                .db(|conn| db::list_sessions(conn, &user.uuid, db::now()))
                .unwrap()
                .is_empty()
        );
        // …mais le compte reste **actif**, et le journal dit pourquoi.
        assert!(compte_actif(&state, &user.uuid));
        let evenements = crate::auth::journal::recent(&state, 10).unwrap();
        let refus = evenements.iter().find(|e| e.kind == "sso_revoked").unwrap();
        assert!(refus.detail.contains("admin"), "{refus:?}");
    }

    /// Une revalidation qui n'est pas un refus d'accès ne ferme rien.
    ///
    /// `unsupported_grant_type` dit que le fournisseur n'implémente pas le
    /// rafraîchissement (ou que la configuration l'empêche) : c'est notre
    /// problème, pas celui de l'utilisateur — et fermer les comptes sur cette
    /// réponse vide l'installation en un quart d'heure.
    #[tokio::test]
    async fn une_erreur_de_configuration_du_fournisseur_ne_ferme_aucun_compte() {
        let state = etat(Provisioning::Auto, false);
        let (user, jeton) = session_sso(&state, "remi@exemple.fr", Some("refresh-1"));

        let (faux, _) = transport(vec![Ok((
            400,
            r#"{"error":"unsupported_grant_type"}"#.to_string(),
        ))]);
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(sweep.revoked, 0);
        assert_eq!(sweep.unreachable, 1);

        assert!(session_vivante(&state, &jeton));
        assert!(compte_actif(&state, &user.uuid));
        // Et aucune trace de révocation n'est écrite : le journal ne doit pas
        // raconter un acte qui n'a pas eu lieu.
        let evenements = crate::auth::journal::recent(&state, 10).unwrap();
        assert!(evenements.iter().all(|e| e.kind != "sso_revoked"));

        // Une réponse illisible (pas de JSON) est traitée de la même façon.
        let (faux, _) = transport(vec![Ok((400, "boum".to_string()))]);
        assert_eq!(
            revalidate_sessions_with(&state, faux)
                .await
                .unwrap()
                .revoked,
            0
        );

        // Les codes qui, eux, parlent d'un accès, sont reconnus.
        assert_eq!(
            refresh_refusal(r#"{"error":"invalid_grant"}"#).as_deref(),
            Some("invalid_grant")
        );
        assert_eq!(
            refresh_refusal(r#"{"error":"unsupported_grant_type"}"#),
            None
        );
        assert_eq!(refresh_refusal("pas du json"), None);
        assert_eq!(refresh_refusal(""), None);
    }

    /// Une session ouverte par mot de passe n'a pas de `refresh_token` : elle
    /// n'est jamais soumise au fournisseur.
    #[tokio::test]
    async fn une_session_locale_n_est_jamais_reverifiee_auprès_du_fournisseur() {
        let state = etat(Provisioning::Auto, false);
        let user = provision(
            &state.auth,
            &reglages(Provisioning::Auto),
            &identite("remi@exemple.fr"),
        )
        .unwrap();
        let jeton = state.auth.start_session(&user.uuid, &entetes()).unwrap();
        state
            .auth
            .db(|conn| {
                conn.execute("UPDATE sessions SET created_at = created_at - 10000", [])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .unwrap();

        // Transport vide : le moindre appel partirait en « requête inattendue »,
        // et le test échouerait sur `unreachable`.
        let (faux, journal) = transport(vec![]);
        let sweep = revalidate_sessions_with(&state, faux).await.unwrap();
        assert_eq!(sweep, Sweep::default(), "aucune session à revérifier");
        assert!(journal.lock().unwrap().is_empty(), "aucun appel sortant");
        assert!(session_vivante(&state, &jeton));
    }
}
