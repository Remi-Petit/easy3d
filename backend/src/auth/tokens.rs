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

/// Un jeton, tel que l'interface le montre (**jamais** sa valeur).
#[derive(Debug, Clone, Serialize)]
pub struct TokenView {
    pub uuid: String,
    pub name: String,
    pub created_at: i64,
    /// `null` tant qu'il n'a jamais servi — c'est le signe qu'un jeton est
    /// oublié, et qu'on peut le révoquer sans rien casser.
    pub last_used_at: Option<i64>,
}

impl From<&db::ApiToken> for TokenView {
    fn from(token: &db::ApiToken) -> Self {
        Self {
            uuid: token.uuid.clone(),
            name: token.name.clone(),
            created_at: token.created_at,
            last_used_at: token.last_used_at,
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
}

/// Demande de création d'un jeton.
#[derive(Debug, Deserialize)]
pub struct NewToken {
    pub name: String,
}

impl Auth {
    /// Délivre un jeton pour ce compte et renvoie sa valeur en clair, avec la
    /// ligne créée (son UUID est celui que l'interface montrera).
    pub fn issue_token(
        &self,
        user_uuid: &str,
        name: &str,
    ) -> Result<(String, db::ApiToken), String> {
        let token = new_token();
        let hash = hash(&token);
        let uuid = uuid::Uuid::now_v7().to_string();
        let name = name.trim().to_string();
        let created = db::now();

        self.db(|conn| db::insert_token(conn, &uuid, user_uuid, &name, &hash))?;
        Ok((
            token,
            db::ApiToken {
                uuid,
                name,
                created_at: created,
                last_used_at: None,
                expires_at: None,
            },
        ))
    }

    /// Compte associé à un jeton présenté en `Authorization: Bearer`.
    ///
    /// Renvoie `None` si le jeton est inconnu, ou si le compte est désactivé —
    /// couper l'accès à quelqu'un doit arrêter aussi ses agents.
    pub fn resolve_token(&self, token: &str) -> Option<db::User> {
        if !self.is_enabled() || token.trim().is_empty() {
            return None;
        }
        let (ticket, user_uuid) = self
            .db(|conn| db::find_token(conn, &hash(token)))
            .ok()
            .flatten()?;

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
pub async fn create(
    State(state): State<AppState>,
    caller: AuthUser,
    Json(request): Json<NewToken>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return auth::error(StatusCode::BAD_REQUEST, "name_required");
    }

    match state.auth.issue_token(&caller.0.uuid, name) {
        Ok((token, created)) => (
            StatusCode::CREATED,
            Json(CreatedToken {
                token,
                uuid: created.uuid,
                name: created.name,
                created_at: created.created_at,
            }),
        )
            .into_response(),
        Err(e) => auth::internal(&e),
    }
}

/// `DELETE /tokens/{uuid}` — révocation.
pub async fn revoke(
    State(state): State<AppState>,
    caller: AuthUser,
    Path(uuid): Path<String>,
) -> Response {
    match state.auth.revoke_token(&caller.0.uuid, &uuid) {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
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
}
