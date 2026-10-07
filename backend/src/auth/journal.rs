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
use std::time::{SystemTime, UNIX_EPOCH};

/// Nombre d'événements rendus par défaut.
const DEFAULT_LIMIT: i64 = 100;

/// Nombre d'événements rendus au plus, même si l'appelant en demande plus.
const MAX_LIMIT: i64 = 500;

/// Bornes de lecture d'une demande `?limit=…`.
///
/// Absent, on applique le défaut ; hors bornes, on **ramène** dans l'intervalle
/// au lieu de refuser : c'est une lecture, et répondre « 400 » parce qu'on a
/// demandé dix événements serait plus gênant qu'utile.
fn limit_of(query: &ListQuery) -> i64 {
    query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

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
    match recent(&state, limit_of(&query)) {
        Ok(vues) => Json(vues).into_response(),
        Err(e) => auth::internal(&e),
    }
}

/// Nombre d'événements relus avant filtrage, pour un agent.
///
/// Le journal se lit par le **haut** (les plus récents) : au-delà, ce n'est plus
/// un journal qu'on consulte mais une archive qu'on interroge. La borne est donc
/// assumée — les critères s'appliquent à cette fenêtre, et la réponse dit
/// combien d'événements ont été vus.
pub const SCAN: i64 = 500;

/// Les derniers événements, prêts à être montrés.
///
/// Une seule lecture pour les trois lecteurs du journal — la route, l'outil MCP
/// `list_audit` et l'assistant : mêmes bornes, et la même résolution des noms
/// **à la lecture** (un compte renommé apparaît sous son nom courant, le journal
/// ne recopie pas des noms qui vieilliraient mal).
///
/// Sans comptes (installation ouverte), il n'y a pas de journal : on rend une
/// liste vide plutôt qu'une erreur — il n'y a rien à montrer, ce n'est pas une
/// panne.
pub fn recent(state: &AppState, limit: i64) -> Result<Vec<EventView>, String> {
    if !state.auth.is_enabled() {
        return Ok(Vec::new());
    }

    state.auth.db(|conn| {
        let events = db::list_events(conn, limit.clamp(1, MAX_LIMIT))?;
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
    })
}

/// Critères de lecture d'un agent (outil MCP `list_audit`, assistant).
///
/// Le pendant, côté serveur, des filtres de la page d'audit : les mêmes trois
/// idées (période, types, texte), pour qu'une question posée à l'IA et un
/// filtrage dans l'interface parlent de la même chose.
#[derive(Debug, Clone, Default)]
pub struct Criteria {
    /// Types à retenir (`login_failed`, `token_created`…). Vide = tous.
    pub kinds: Vec<String>,
    /// Instant le plus ancien accepté (secondes epoch, `0` = aucune borne).
    pub since: i64,
    /// Mots à chercher dans le type, le compte, le sujet, la précision, l'adresse.
    pub query: String,
}

impl Criteria {
    /// Lit le journal (fenêtre [`SCAN`]) et applique les critères.
    pub fn read(&self, state: &AppState) -> Result<Vec<EventView>, String> {
        Ok(self.apply(recent(state, SCAN)?))
    }

    /// Applique les critères à une lecture déjà faite.
    ///
    /// Séparé de [`Criteria::read`] pour être testable sans base : c'est ici
    /// qu'est la logique, et c'est elle qui mérite des tests.
    pub fn apply(&self, events: Vec<EventView>) -> Vec<EventView> {
        let text = self.query.trim().to_lowercase();

        events
            .into_iter()
            .filter(|event| event.at >= self.since)
            .filter(|event| self.kinds.is_empty() || self.kinds.contains(&event.kind))
            .filter(|event| {
                text.is_empty()
                    || [
                        event.kind.as_str(),
                        event.actor.as_deref().unwrap_or(""),
                        event.subject.as_str(),
                        event.detail.as_str(),
                        event.ip.as_str(),
                    ]
                    .join(" ")
                    .to_lowercase()
                    .contains(&text)
            })
            .collect()
    }
}

/// Instant le plus ancien des `hours` dernières heures (secondes epoch).
pub fn since_hours(hours: u32) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    now - i64::from(hours) * 3600
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_lecture_est_bornee_des_deux_cotes() {
        let demande = |limit| ListQuery { limit };

        // Absent : le défaut, pas « tout ».
        assert_eq!(limit_of(&demande(None)), DEFAULT_LIMIT);

        // Une valeur raisonnable passe telle quelle.
        assert_eq!(limit_of(&demande(Some(42))), 42);
        assert_eq!(limit_of(&demande(Some(1))), 1);
        assert_eq!(limit_of(&demande(Some(MAX_LIMIT))), MAX_LIMIT);

        // Zéro et le négatif sont ramenés à un minimum d'un événement : la
        // réponse reste exploitable, sans erreur à traduire côté interface.
        assert_eq!(limit_of(&demande(Some(0))), 1);
        assert_eq!(limit_of(&demande(Some(-10))), 1);

        // Une demande démesurée est plafonnée : le journal ne peut pas être
        // aspiré d'un coup.
        assert_eq!(limit_of(&demande(Some(10_000))), MAX_LIMIT);
    }

    /// Un événement minimal : ces tests ne portent que sur le filtrage.
    fn evenement(at: i64, kind: &str, actor: Option<&str>, detail: &str) -> EventView {
        EventView {
            at,
            kind: kind.to_string(),
            actor: actor.map(str::to_string),
            subject: "sujet".to_string(),
            detail: detail.to_string(),
            ip: "10.0.0.1".to_string(),
            user_agent: String::new(),
        }
    }

    fn kinds_of(events: &[EventView]) -> Vec<String> {
        events.iter().map(|event| event.kind.clone()).collect()
    }

    #[test]
    fn sans_critere_on_garde_tout_ce_qui_a_ete_lu() {
        let events = vec![
            evenement(100, "login_ok", Some("remi"), ""),
            evenement(200, "login_failed", None, "invalid_credentials"),
        ];
        assert_eq!(Criteria::default().apply(events).len(), 2);
    }

    #[test]
    fn les_criteres_filtrent_par_periode_type_et_texte() {
        let events = vec![
            evenement(100, "login_ok", Some("remi"), ""),
            evenement(200, "login_failed", None, "invalid_credentials"),
            evenement(300, "token_created", Some("remi"), "30"),
            evenement(400, "role_deleted", Some("root"), "invite"),
        ];

        // Période : la borne est l'instant le plus ancien **accepté**.
        let recents = Criteria {
            since: 300,
            ..Default::default()
        }
        .apply(events.clone());
        assert_eq!(kinds_of(&recents), vec!["token_created", "role_deleted"]);

        // Types : un « ou », comme les puces de la page d'audit.
        let types = Criteria {
            kinds: vec!["login_failed".to_string(), "token_created".to_string()],
            ..Default::default()
        }
        .apply(events.clone());
        assert_eq!(kinds_of(&types), vec!["login_failed", "token_created"]);

        // Texte : le compte, la précision, l'adresse — sans tenir compte de la casse.
        let par_acteur = Criteria {
            query: "REMI".to_string(),
            ..Default::default()
        }
        .apply(events.clone());
        assert_eq!(kinds_of(&par_acteur), vec!["login_ok", "token_created"]);

        let par_detail = Criteria {
            query: "invalid".to_string(),
            ..Default::default()
        }
        .apply(events.clone());
        assert_eq!(kinds_of(&par_detail), vec!["login_failed"]);

        // Un type que le journal n'écrit pas ne rend rien — et surtout pas tout.
        let inconnu = Criteria {
            kinds: vec!["login_removed".to_string()],
            ..Default::default()
        }
        .apply(events.clone());
        assert!(inconnu.is_empty());

        // Les trois ensemble.
        let croises = Criteria {
            kinds: vec!["login_failed".to_string(), "token_created".to_string()],
            since: 250,
            query: "remi".to_string(),
        }
        .apply(events);
        assert_eq!(kinds_of(&croises), vec!["token_created"]);
    }

    #[test]
    fn le_since_des_heures_recule_dans_le_temps() {
        let borne = since_hours(24);
        let maintenant = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // À quelques secondes près (le test dure moins que ça).
        assert!((maintenant - borne - 86_400).abs() < 5, "borne : {borne}");
    }
}
