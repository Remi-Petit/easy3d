//! Journal d'audit : **qui** a fait quoi, et quand.
//!
//! C'est le témoin de l'installation : connexions (réussies, refusées,
//! bloquées), connexions par le fournisseur d'identité, jetons créés ou
//! révoqués, et changements de droits. Le jour où un accès surprend, c'est la
//! seule chose à consulter — un serveur qui n'écrit que sur la console n'a rien
//! à montrer une fois redémarré.
//!
//! Ce qui n'y est **pas**, volontairement :
//!
//! - aucune valeur de secret (jeton, mot de passe, secret client) : le journal
//!   ne doit pas devenir un second endroit où ils dorment ;
//! - pas de trace de navigation dans le catalogue : trop de bruit pour ce qu'on
//!   y chercherait ;
//! - pas de motif d'écriture bloquant : un événement qui ne peut pas être écrit
//!   laisse un avertissement et l'action continue (un journal est un témoin, pas
//!   une condition).
//!
//! L'écriture se fait par [`Auth::log`](crate::auth::Auth::log) : elle range
//! l'adresse et le client HTTP de la requête au passage.

use crate::api::AppState;
use crate::auth::{self, db};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// Nombre d'événements rendus par défaut.
const DEFAULT_LIMIT: i64 = 100;

/// Demande de lecture (`GET /journal?limit=…`).
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
}

/// Un événement, tel que l'interface l'affiche.
///
/// Les **codes** (`kind`, `detail`) partent tels quels : c'est l'interface qui
/// les traduit, comme partout ailleurs.
#[derive(Debug, Clone, Serialize)]
pub struct EventView {
    pub at: i64,
    pub kind: String,
    /// Nom du compte concerné, quand il existe encore.
    ///
    /// Le nom, et non l'UUID : un journal se lit par un humain, et l'UUID n'est
    /// là que pour recouper. `None` quand il n'y avait pas de compte (tentative
    /// refusée) ou qu'il a été supprimé depuis — c'est aussi une information.
    pub actor: Option<String>,
    pub subject: String,
    pub detail: String,
    pub ip: String,
    pub user_agent: String,
}

/// `GET /journal` — derniers événements, du plus récent au plus ancien.
///
/// Réservé à qui peut **voir les comptes** (`users.read`) : c'est un journal de
/// sécurité, il n'a pas à être lisible par un simple lecteur du catalogue.
pub async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> Response {
    if !state.auth.is_enabled() {
        return Json(Vec::<EventView>::new()).into_response();
    }

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500);
    match state.auth.db(|conn| {
        // Les noms sont résolus **à la lecture** : un compte renommé apparaît
        // sous son nom courant, et le journal ne recopie pas des noms qui
        // vieilliraient mal.
        let events = db::list_events(conn, limit)?;
        let mut vues = Vec::with_capacity(events.len());
        for event in events {
            let actor = match event.actor_uuid.as_deref() {
                Some(uuid) => db::find_by_uuid(conn, uuid)?.map(|user| user.username),
                None => None,
            };
            vues.push(EventView {
                at: event.at,
                kind: event.kind,
                actor,
                subject: event.subject,
                detail: event.detail,
                ip: event.ip,
                user_agent: event.user_agent,
            });
        }
        Ok(vues)
    }) {
        Ok(vues) => Json(vues).into_response(),
        Err(e) => auth::internal(&e),
    }
}
