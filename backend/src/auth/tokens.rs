//! Jetons d'API : l'accès des **agents** (clients MCP) au catalogue.
//!
//! Un agent — Claude Code, un script, une CI — n'a pas de navigateur, donc pas de
//! cookie de session : il présente un jeton en `Authorization: Bearer …`. Chaque
//! compte crée les siens, les nomme (« portable », « Claude », « CI ») et les
//! révoque quand il veut. Un jeton **hérite des droits du compte** : il n'en
//! donne jamais plus, et la révocation est immédiate.
//!
//! Deux précautions, et une limite assumée :
//!
//! - **seule l'empreinte est stockée** — BLAKE2s-256, un hachage *rapide* : le
//!   jeton est un secret de 32 octets tiré au hasard (pas de dictionnaire à
//!   craindre), et une vérification à chaque requête ne peut pas coûter les 50 ms
//!   d'Argon2 ;
//! - **le jeton n'est montré qu'une fois**, à sa création : ensuite l'interface ne
//!   sait plus que le révoquer, comme pour la clé d'API du fournisseur d'IA ;
//! - il ne périme pas tout seul (`expires_at` reste vide) : c'est la révocation
//!   qui l'arrête.
//!
//! Le préfixe `e3d_` n'est pas décoratif : il rend un jeton reconnaissable dans
//! une configuration ou un journal, et fait dire aux analyseurs de secrets
//! (« gitleaks » et compagnie) que ce n'est pas une chaîne quelconque.

use crate::api::AppState;
use crate::auth::{self, Auth, AuthUser, db};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use blake2::{Blake2s256, Digest};
use serde::{Deserialize, Serialize};

/// Préfixe des jetons, visible en clair.
const PREFIX: &str = "e3d_";

/// Empreinte d'un jeton, telle qu'elle est stockée.
///
/// Le préfixe fait partie de ce qui est haché : l'empreinte d'un jeton donné ne
/// peut donc pas être confondue avec celle d'une autre chaîne.
pub fn hash(token: &str) -> String {
    Blake2s256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Jeton neuf : `e3d_` suivi de 32 octets aléatoires en hexadécimal.
///
/// Deux UUIDv4 mis bout à bout, comme les jetons de session : c'est déjà du
/// `getrandom`, inutile d'ajouter un générateur pour ça.
pub fn new_token() -> String {
    format!(
        "{PREFIX}{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Longueur maximale d'un nom de jeton.
///
/// Le nom sert à se reconnaître (« CI », « portable ») : au-delà de quelques
/// mots, c'est une note qu'on écrit ailleurs. La borne évite qu'un texte
/// arbitrairement long se retrouve stocké, listé et affiché.
pub const NAME_MAX: usize = 80;

/// Durée de vie maximale acceptée, en jours.
///
/// Dix ans : au-delà, soit on veut un jeton éternel (`0`), soit c'est une faute
/// de frappe. La borne évite aussi de calculer une date absurde.
const MAX_DAYS: i64 = 3650;

/// Date d'expiration d'un jeton, à partir d'une durée en jours.
///
/// `None`, `0` et les valeurs négatives veulent dire **« n'expire jamais »** :
/// c'est le cas de tous les jetons créés avant que ce réglage existe, et le
/// rendre explicite vaut mieux qu'une date par défaut que personne n'attendait.
///
pub fn expiry_from_days(days: Option<i64>, now: i64) -> Option<i64> {
    let jours = days?.clamp(0, MAX_DAYS);
    (jours > 0).then(|| now + jours * 86_400)
}

/// Un jeton, tel que l'interface le montre (**jamais** sa valeur).
#[derive(Debug, Clone, Serialize)]
pub struct TokenView {
    pub uuid: String,
    pub name: String,
    pub created_at: i64,
    /// `null` tant qu'il n'a jamais servi — c'est le signe qu'un jeton est
    /// oublié, et qu'on peut le révoquer sans rien casser.
    pub last_used_at: Option<i64>,
    /// `null` = sans expiration.
    pub expires_at: Option<i64>,
}

impl From<&db::ApiToken> for TokenView {
    fn from(token: &db::ApiToken) -> Self {
        Self {
            uuid: token.uuid.clone(),
            name: token.name.clone(),
            created_at: token.created_at,
            last_used_at: token.last_used_at,
            expires_at: token.expires_at,
        }
    }
}

/// Réponse de la création : le jeton **en clair**, une seule fois.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedToken {
    /// À recopier dans la configuration de l'agent. Il ne sera plus affiché.
    pub token: String,
    pub uuid: String,
    pub name: String,
    pub created_at: i64,
    /// `null` = sans expiration.
    pub expires_at: Option<i64>,
}

/// Demande de création d'un jeton.
#[derive(Debug, Deserialize)]
pub struct NewToken {
    pub name: String,
    /// Durée de vie en jours : `0` (ou absent) = sans expiration.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

impl Auth {
    /// Délivre un jeton pour ce compte et renvoie sa valeur en clair, avec la
    /// ligne créée (son UUID est celui que l'interface montrera).
    ///
    /// `expires_at` vient de [`expiry_from_days`] : la date est décidée par
    /// l'appelant, qui seul sait ce que l'utilisateur a demandé.
    pub fn issue_token(
        &self,
        user_uuid: &str,
        name: &str,
        expires_at: Option<i64>,
    ) -> Result<(String, db::ApiToken), String> {
        let token = new_token();
        let hash = hash(&token);
        let uuid = uuid::Uuid::now_v7().to_string();
        let name = name.trim().to_string();
        let created = db::now();

        self.db(|conn| {
            // Ménage au passage : un jeton expiré n'a plus rien à faire dans la
            // liste, et la ligne ne sert plus à rien. C'est le seul moment où
            // l'occasion se présente sans requête supplémentaire périodique.
            db::purge_expired_tokens(conn)?;
            db::insert_token(conn, &uuid, user_uuid, &name, &hash, expires_at)
        })?;
        Ok((
            token,
            db::ApiToken {
                uuid,
                name,
                created_at: created,
                last_used_at: None,
                expires_at,
            },
        ))
    }

    /// Compte associé à un jeton présenté en `Authorization: Bearer`.
    ///
    /// Renvoie `None` si le jeton est inconnu, **expiré**, ou si le compte est
    /// désactivé — couper l'accès à quelqu'un doit arrêter aussi ses agents.
    pub fn resolve_token(&self, token: &str) -> Option<db::User> {
        if !self.is_enabled() || token.trim().is_empty() {
            return None;
        }
        let (ticket, user_uuid) = self
            .db(|conn| db::find_token(conn, &hash(token)))
            .ok()
            .flatten()?;

        // La date fait foi **à chaque appel**, et pas seulement au ménage : une
        // ligne encore présente (le ménage n'a pas encore tourné) ne doit pas
        // ouvrir l'accès, sinon la durée de vie dépendrait du hasard du
        // nettoyage.
        if ticket.expires_at.is_some_and(|fin| fin <= db::now()) {
            return None;
        }

        let user = self
            .db(|conn| db::find_by_uuid(conn, &user_uuid))
            .ok()
            .flatten()
            .filter(|user| !user.disabled)?;

        // Trace d'usage : une écriture minuscule par appel authentifié, et
        // l'interface peut dire quels jetons servent encore.
        let _ = self.db(|conn| db::touch_token(conn, &ticket.uuid));
        Some(user)
    }

    /// Jetons d'un compte.
    pub fn tokens_of(&self, user_uuid: &str) -> Result<Vec<db::ApiToken>, String> {
        self.db(|conn| db::list_tokens(conn, user_uuid))
    }

    /// Révoque un jeton du compte (et de lui seul).
    pub fn revoke_token(&self, user_uuid: &str, uuid: &str) -> Result<bool, String> {
        self.db(|conn| db::delete_token(conn, user_uuid, uuid))
    }
}

/// `GET /tokens` — les jetons **du compte connecté**.
///
/// Il n'y a pas de droit à exiger : chacun gère les siens. Ceux des autres ne
/// sont visibles nulle part, pas même pour un administrateur (il peut désactiver
/// le compte, ce qui les neutralise tous d'un coup).
pub async fn list(State(state): State<AppState>, caller: AuthUser) -> Response {
    match state.auth.tokens_of(&caller.0.uuid) {
        Ok(tokens) => Json(tokens.iter().map(TokenView::from).collect::<Vec<_>>()).into_response(),
        Err(e) => auth::internal(&e),
    }
}

/// `POST /tokens` — créer un jeton, avec un nom qui dit à quoi il sert.
///
/// `expires_in_days` est facultatif : `0` (ou absent) veut dire « n'expire
/// jamais ». C'est le choix le plus sûr par défaut — un agent qui s'arrête tout
/// seul au bout de trois mois sans que personne ne l'ait demandé serait une
/// surprise désagréable — et l'interface propose les durées usuelles.
pub async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
    headers: axum::http::HeaderMap,
    Json(request): Json<NewToken>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return auth::error(StatusCode::BAD_REQUEST, "name_required");
    }
    // Compté en **caractères** (et non en octets) : la limite doit dire la même
    // chose pour un nom accentué et pour un nom ASCII.
    if name.chars().count() > NAME_MAX {
        return auth::error(StatusCode::BAD_REQUEST, "name_too_long");
    }

    let expires_at = expiry_from_days(request.expires_in_days, db::now());
    match state.auth.issue_token(&caller.0.uuid, name, expires_at) {
        Ok((token, created)) => {
            // Le nom du jeton, jamais le jeton : le journal n'a pas à devenir un
            // second endroit où un secret dort.
            state.auth.log(
                &headers,
                "token_created",
                Some(&caller.0.uuid),
                name,
                &request.expires_in_days.unwrap_or(0).clamp(0, u32::MAX as i64).to_string(),
            );
            (
                StatusCode::CREATED,
                Json(CreatedToken {
                    token,
                    uuid: created.uuid,
                    name: created.name,
                    created_at: created.created_at,
                    expires_at: created.expires_at,
                }),
            )
                .into_response()
        }
        Err(e) => auth::internal(&e),
    }
}

/// `DELETE /tokens/{uuid}` — révocation.
pub async fn revoke(
    State(state): State<AppState>,
    caller: AuthUser,
    headers: axum::http::HeaderMap,
    Path(uuid): Path<String>,
) -> Response {
    match state.auth.revoke_token(&caller.0.uuid, &uuid) {
        Ok(true) => {
            state
                .auth
                .log(&headers, "token_revoked", Some(&caller.0.uuid), &uuid, "");
            Json(serde_json::json!({ "ok": true })).into_response()
        }
        Ok(false) => auth::error(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => auth::internal(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_jeton_est_reconnaissable_et_suffisamment_long() {
        let jetons: Vec<String> = (0..8).map(|_| new_token()).collect();
        for token in &jetons {
            assert!(token.starts_with(PREFIX), "préfixe : {token}");
            assert_eq!(token.len(), PREFIX.len() + 64, "32 octets en hexadécimal");
            assert!(
                token[PREFIX.len()..].chars().all(|c| c.is_ascii_hexdigit()),
                "jeton : {token}"
            );
        }

        let mut uniques = jetons.clone();
        uniques.sort();
        uniques.dedup();
        assert_eq!(uniques.len(), jetons.len());
    }

    #[test]
    fn l_empreinte_est_stable_et_ne_laisse_pas_passer_le_jeton() {
        let token = new_token();
        let empreinte = hash(&token);

        assert_eq!(empreinte.len(), 64, "BLAKE2s-256 en hexadécimal");
        assert_eq!(empreinte, hash(&token), "même jeton, même empreinte");
        assert!(!empreinte.contains(PREFIX), "l'empreinte ne dit rien du jeton");
        assert_ne!(empreinte, hash(&new_token()), "deux jetons, deux empreintes");
    }

    #[test]
    fn la_duree_de_vie_se_traduit_en_date() {
        // Absent, zéro ou négatif : le jeton n'expire jamais. C'est le
        // comportement des jetons créés avant que le réglage existe.
        assert_eq!(expiry_from_days(None, 1_000), None);
        assert_eq!(expiry_from_days(Some(0), 1_000), None);
        assert_eq!(expiry_from_days(Some(-5), 1_000), None);

        assert_eq!(expiry_from_days(Some(1), 1_000), Some(1_000 + 86_400));
        assert_eq!(expiry_from_days(Some(30), 1_000), Some(1_000 + 30 * 86_400));

        // Plafond : au-delà de dix ans, c'est « jamais » ou une faute de
        // frappe — et une date absurde ne doit pas être écrite.
        assert_eq!(
            expiry_from_days(Some(1_000_000), 1_000),
            Some(1_000 + MAX_DAYS * 86_400)
        );
    }

    #[test]
    fn un_jeton_expire_ne_vaut_plus_rien() {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::open(&dir.path().join("easy3d.db"), false).unwrap();
        auth.db(db::ensure_builtin_roles).unwrap();
        let user = db::NewUser::new("remi", "remi@exemple.fr", None);
        auth.db(|conn| db::insert_user(conn, &user)).unwrap();

        // Période passée : le jeton est refusé **même si la ligne est encore
        // là** (le ménage n'a pas forcément tourné).
        let (perime, _) = auth
            .issue_token(&user.uuid, "vieil agent", Some(db::now() - 1))
            .unwrap();
        assert!(auth.resolve_token(&perime).is_none());

        // Encore valable, et sans expiration : les deux passent.
        let (frais, _) = auth
            .issue_token(&user.uuid, "agent", Some(db::now() + 3_600))
            .unwrap();
        assert!(auth.resolve_token(&frais).is_some());
        let (sans_fin, _) = auth.issue_token(&user.uuid, "éternel", None).unwrap();
        assert!(auth.resolve_token(&sans_fin).is_some());

        // Le ménage (à la création suivante) emporte la ligne expirée.
        auth.issue_token(&user.uuid, "suivant", None).unwrap();
        let restants = auth.tokens_of(&user.uuid).unwrap();
        assert_eq!(restants.len(), 3, "la ligne expirée est retirée : {restants:?}");
        assert!(restants.iter().all(|t| t.name != "vieil agent"));
    }
}
