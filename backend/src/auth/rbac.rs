//! Rôles, droits, et les écrans qui les administrent.
//!
//! # Le modèle, en trois phrases
//!
//! 1. Un **rôle** porte une liste de droits ([`permissions::ALL`]) ; un compte
//!    porte des rôles **et**, éventuellement, des droits posés directement sur
//!    lui.
//! 2. Les droits **s'additionnent** : il n'existe aucun refus explicite, donc
//!    rien à ordonner entre les sources. On ajoute un droit à un compte sans
//!    toucher à ses rôles.
//! 3. Le rôle `admin` est un **superutilisateur codé** : il a tous les droits
//!    sans qu'on ait à les lui lister, et il ne peut donc pas se verrouiller
//!    dehors par erreur (les garde-fous empêchent par ailleurs de désactiver ou
//!    supprimer le **dernier** administrateur actif).
//!
//! # Ce que ce module ne fait pas
//!
//! Les droits ne portent **pas** sur des dossiers : ils sont globaux. Restreindre
//! un compte à `Maison/**` obligerait à filtrer le scan, les aperçus, la liste
//! diffusée en WebSocket et les documents CRDT par utilisateur — c'est un autre
//! chantier, décidé comme tel.
//!
//! # Codes d'erreur
//!
//! Comme ailleurs, l'API renvoie des **codes** (`last_admin`, `role_frozen`,
//! `name_taken`…) que l'interface traduit (`admin.accounts.errors.*`, i18n ×4).

use crate::api::AppState;
use crate::auth::{self, Auth, AuthUser, db, permissions, password};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// Un rôle, tel que l'interface l'affiche.
#[derive(Debug, Clone, Serialize)]
pub struct RoleView {
    pub uuid: String,
    pub name: String,
    pub description: String,
    /// Rôle livré (`admin`, `lecteur`) : figé, mais clonable.
    pub builtin: bool,
    pub permissions: Vec<String>,
    /// Nombre de comptes qui le portent — dit ce qu'on laisse derrière soi en le
    /// supprimant.
    pub members: i64,
}

/// Désignation minimale d'un rôle (cases à cocher d'un compte).
#[derive(Debug, Clone, Serialize)]
pub struct RoleRef {
    pub uuid: String,
    pub name: String,
}

/// Un compte, tel que l'écran d'administration le montre.
#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub uuid: String,
    pub username: String,
    pub email: String,
    pub roles: Vec<RoleRef>,
    /// Droits posés **directement** sur ce compte (à cocher tel quel).
    pub direct: Vec<String>,
    /// Droits effectifs (rôles ∪ directs) : ce que le serveur appliquera.
    pub effective: Vec<String>,
    pub disabled: bool,
    /// `true` si le compte est rattaché à une identité du fournisseur d'identité.
    ///
    /// L'interface le montre : en provisionnement `manual`, c'est ce qui
    /// distingue un compte qui peut entrer par le SSO d'un compte qui ne le peut
    /// pas encore.
    pub oidc: bool,
    pub created_at: i64,
}

/// Réponse de `GET /users`.
#[derive(Debug, Clone, Serialize)]
pub struct AccountsResponse {
    pub users: Vec<AccountView>,
    /// Rôles disponibles, pour les cases à cocher.
    pub roles: Vec<RoleRef>,
    /// Rôle attribué aux nouveaux comptes (`None` = aucun).
    pub default_role: Option<String>,
}

/// Réponse de `GET /roles`.
#[derive(Debug, Clone, Serialize)]
pub struct RolesResponse {
    pub roles: Vec<RoleView>,
    pub default_role: Option<String>,
}

/// `GET /permissions` — catalogue des droits.
///
/// Ouvert à tout compte connecté : c'est une liste d'identifiants, dont
/// l'interface a besoin pour construire ses cases à cocher, et qui n'apprend
/// rien sur l'installation.
pub async fn list_permissions() -> Json<Vec<permissions::PermissionInfo>> {
    Json(permissions::describe())
}

/// `GET /users` — comptes, rôles disponibles et rôle par défaut.
pub async fn list_users(State(state): State<AppState>) -> Response {
    if !state.auth.is_enabled() {
        // Sans comptes, il n'y a rien à lister — et surtout pas une erreur :
        // c'est une installation normale, l'interface doit simplement ne rien
        // afficher.
        return Json(AccountsResponse {
            users: Vec::new(),
            roles: Vec::new(),
            default_role: None,
        })
        .into_response();
    }

    match state.auth.db(|conn| {
        let users = db::list_users(conn)?;
        let roles: Vec<RoleRef> = db::list_roles(conn)?
            .into_iter()
            .map(|role| RoleRef {
                uuid: role.uuid,
                name: role.name,
            })
            .collect();
        let default_role = db::setting(conn, db::DEFAULT_ROLE)?;

        let mut views = Vec::with_capacity(users.len());
        for user in &users {
            views.push(account_view(conn, user)?);
        }
        Ok(AccountsResponse {
            users: views,
            roles,
            default_role,
        })
    }) {
        Ok(response) => Json(response).into_response(),
        Err(e) => auth::internal(&e),
    }
}

/// Vue complète d'un compte (rôles, droits directs, droits effectifs).
fn account_view(conn: &rusqlite::Connection, user: &db::User) -> Result<AccountView, String> {
    Ok(AccountView {
        uuid: user.uuid.clone(),
        username: user.username.clone(),
        email: user.email.clone(),
        roles: db::roles_named(conn, &user.uuid)?
            .into_iter()
            .map(|(uuid, name)| RoleRef { uuid, name })
            .collect(),
        direct: db::user_permissions(conn, &user.uuid)?,
        effective: if db::has_role(conn, &user.uuid, db::ADMIN_ROLE)? {
            permissions::all_ids().iter().map(|id| id.to_string()).collect()
        } else {
            db::effective_permissions(conn, &user.uuid)?
        },
        disabled: user.disabled,
        oidc: user.oidc_subject.is_some(),
        created_at: user.created_at,
    })
}

/// Demande de création d'un compte.
#[derive(Debug, Deserialize)]
pub struct NewAccount {
    pub username: String,
    pub email: String,
    /// Absent = compte sans mot de passe (connexion par OIDC uniquement).
    pub password: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// `POST /users` — création d'un compte par un administrateur.
pub async fn create_user(
    State(state): State<AppState>,
    Json(request): Json<NewAccount>,
) -> Response {
    let username = request.username.trim().to_string();
    let email = request.email.trim().to_lowercase();
    if username.is_empty() || email.is_empty() {
        return auth::error(StatusCode::BAD_REQUEST, "name_required");
    }

    // Le mot de passe, s'il est fourni, est vérifié **avant** toute écriture :
    // un refus ne doit pas laisser un compte à moitié créé.
    let hash = match request.password.as_deref().filter(|p| !p.is_empty()) {
        Some(password) => {
            if let Err(code) = password::check_length(password) {
                return auth::error(StatusCode::BAD_REQUEST, code);
            }
            match password::hash(password) {
                Ok(hash) => Some(hash),
                Err(e) => return auth::internal(&e),
            }
        }
        None => None,
    };

    let roles = match known_roles(&state, &request.roles) {
        Ok(roles) => roles,
        Err(code) => return auth::error(StatusCode::BAD_REQUEST, code),
    };
    // Sans rôle explicite, on applique le **rôle par défaut** de l'installation :
    // c'est ce qui donne son sens au réglage, et ce qui fera qu'un compte créé
    // par OIDC n'arrive pas sans rien.
    let roles = if roles.is_empty() {
        default_role(&state)
    } else {
        roles
    };
    let direct = match known_permissions(&request.permissions) {
        Ok(ids) => ids,
        Err(code) => return auth::error(StatusCode::BAD_REQUEST, code),
    };

    let user = db::NewUser::new(&username, &email, hash);
    match insert_account(&state.auth, user, &roles, &direct) {
        Ok(created) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "uuid": created.uuid })),
        )
            .into_response(),
        Err(e) => conflict_or_internal(&e),
    }
}

/// Crée un compte et lui attribue rôles et droits directs.
///
/// Un seul chemin d'écriture, partagé par `POST /users` et par le
/// **provisionnement OIDC** : deux façons de créer un compte, c'est deux façons
/// de l'oublier à moitié (rôle par défaut non appliqué, par exemple).
///
/// Sans rôle explicite, le **rôle par défaut** de l'installation est appliqué :
/// un compte créé par le fournisseur d'identité n'arrive donc pas sans rien.
pub(crate) fn insert_account(
    auth: &Auth,
    user: db::NewUser,
    roles: &[String],
    direct: &[String],
) -> Result<db::User, String> {
    let roles = if roles.is_empty() {
        default_roles(auth)
    } else {
        roles.to_vec()
    };
    auth.db(|conn| {
        db::insert_user(conn, &user)?;
        db::set_user_roles(conn, &user.uuid, &roles)?;
        db::set_user_permissions(conn, &user.uuid, direct)?;
        db::find_by_uuid(conn, &user.uuid)?.ok_or_else(|| "compte disparu".to_string())
    })
}

/// Demande de modification d'un compte.
///
/// Tous les champs sont facultatifs : l'interface n'envoie que ce qu'elle change,
/// et un champ absent veut dire « ne touche pas » (même convention que la clé
/// d'API dans `config.yml`).
#[derive(Debug, Deserialize, Default)]
pub struct UpdateAccount {
    pub username: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
    pub roles: Option<Vec<String>>,
    pub permissions: Option<Vec<String>>,
    pub disabled: Option<bool>,
}

/// `PUT /users/{uuid}` — modification d'un compte.
pub async fn update_user(
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    Json(request): Json<UpdateAccount>,
) -> Response {
    let Some(target) = (match state.auth.db(|conn| db::find_by_uuid(conn, &uuid)) {
        Ok(user) => user,
        Err(e) => return auth::internal(&e),
    }) else {
        return auth::error(StatusCode::NOT_FOUND, "not_found");
    };

    let hash = match request.password.as_deref().filter(|p| !p.is_empty()) {
        Some(password) => {
            if let Err(code) = password::check_length(password) {
                return auth::error(StatusCode::BAD_REQUEST, code);
            }
            match password::hash(password) {
                Ok(hash) => Some(hash),
                Err(e) => return auth::internal(&e),
            }
        }
        None => None,
    };

    let roles = match request.roles.as_ref() {
        Some(ids) => match known_roles(&state, ids) {
            Ok(roles) => Some(roles),
            Err(code) => return auth::error(StatusCode::BAD_REQUEST, code),
        },
        None => None,
    };
    let direct = match request.permissions.as_ref() {
        Some(ids) => match known_permissions(ids) {
            Ok(ids) => Some(ids),
            Err(code) => return auth::error(StatusCode::BAD_REQUEST, code),
        },
        None => None,
    };

    let result = state.auth.db(|conn| {
        // ── Garde-fous, sous le même verrou que l'écriture ──────────────
        // Une seule connexion, protégée par un `Mutex` : vérifier puis écrire
        // dans le même bloc est donc atomique (aucun autre handler ne peut
        // s'intercaler).
        let was_admin = db::has_role(conn, &uuid, db::ADMIN_ROLE)? && !target.disabled;
        let stays_admin = roles
            .as_ref()
            .map(|roles| roles.iter().any(|role| role == db::ADMIN_ROLE))
            .unwrap_or(!target.disabled && db::has_role(conn, &uuid, db::ADMIN_ROLE)?);
        let will_be_disabled = request.disabled.unwrap_or(target.disabled);
        if was_admin && !(stays_admin && !will_be_disabled) && db::count_admins(conn)? <= 1 {
            return Err("last_admin".to_string());
        }

        if let (Some(username), Some(email)) = (&request.username, &request.email) {
            db::update_identity(conn, &uuid, username, email)?;
        }
        if let Some(hash) = &hash {
            db::set_password_hash(conn, &uuid, hash)?;
        }
        if let Some(disabled) = request.disabled {
            db::set_disabled(conn, &uuid, disabled)?;
            if disabled {
                // Un compte désactivé ne doit plus rien pouvoir : ses sessions
                // ouvertes cessent immédiatement d'être valables (`session_user`
                // filtre les comptes désactivés), mais autant les retirer.
                db::delete_sessions_of(conn, &uuid)?;
            }
        }
        if let Some(roles) = &roles {
            db::set_user_roles(conn, &uuid, roles)?;
        }
        if let Some(direct) = &direct {
            db::set_user_permissions(conn, &uuid, direct)?;
        }
        Ok(())
    });

    match result {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => conflict_or_internal(&e),
    }
}

/// `DELETE /users/{uuid}`
pub async fn delete_user(
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    caller: AuthUser,
) -> Response {
    if caller.0.uuid == uuid {
        // Se supprimer soi-même laisserait l'installation sans témoin de
        // l'opération, et c'est le genre de clic qu'on regrette.
        return auth::error(StatusCode::BAD_REQUEST, "cannot_delete_self");
    }

    let result = state.auth.db(|conn| {
        let Some(target) = db::find_by_uuid(conn, &uuid)? else {
            return Ok(false);
        };
        if db::has_role(conn, &uuid, db::ADMIN_ROLE)? && !target.disabled && db::count_admins(conn)? <= 1
        {
            return Err("last_admin".to_string());
        }
        db::delete_user(conn, &uuid)?;
        Ok(true)
    });

    match result {
        Ok(true) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => auth::error(StatusCode::NOT_FOUND, "not_found"),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => auth::internal(&e),
    }
}

/// `GET /roles` — rôles, leurs droits, et le rôle par défaut.
pub async fn list_roles(State(state): State<AppState>) -> Response {
    if !state.auth.is_enabled() {
        return Json(RolesResponse {
            roles: Vec::new(),
            default_role: None,
        })
        .into_response();
    }

    match state.auth.db(|conn| {
        let mut views = Vec::new();
        for role in db::list_roles(conn)? {
            let permissions = if role.uuid == db::ADMIN_ROLE {
                // Le superutilisateur a tout par construction : on l'affiche
                // coché partout, sinon l'écran donnerait l'impression qu'il n'a
                // aucun droit.
                permissions::all_ids().iter().map(|id| id.to_string()).collect()
            } else {
                db::role_permissions(conn, &role.uuid)?
            };
            views.push(RoleView {
                members: db::count_role_members(conn, &role.uuid)?,
                uuid: role.uuid,
                name: role.name,
                description: role.description,
                builtin: role.builtin,
                permissions,
            });
        }
        Ok(RolesResponse {
            roles: views,
            default_role: db::setting(conn, db::DEFAULT_ROLE)?,
        })
    }) {
        Ok(response) => Json(response).into_response(),
        Err(e) => auth::internal(&e),
    }
}

/// Demande de création d'un rôle.
#[derive(Debug, Deserialize)]
pub struct NewRole {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Rôle à **cloner** (ses droits sont recopiés).
    pub from: Option<String>,
    /// Droits explicites — prioritaires sur ceux du clone.
    pub permissions: Option<Vec<String>>,
}

/// `POST /roles` — création (ou clonage) d'un rôle.
pub async fn create_role(State(state): State<AppState>, Json(request): Json<NewRole>) -> Response {
    let name = request.name.trim().to_string();
    if name.is_empty() {
        return auth::error(StatusCode::BAD_REQUEST, "name_required");
    }

    let result = state.auth.db(|conn| {
        let ids = match (&request.from, &request.permissions) {
            (_, Some(ids)) => known_permissions(ids).map_err(|e| e.to_string())?,
            (Some(from), None) => {
                if db::find_role(conn, from)?.is_none() {
                    return Err("unknown_role".to_string());
                }
                if from == db::ADMIN_ROLE {
                    permissions::all_ids().iter().map(|id| id.to_string()).collect()
                } else {
                    db::role_permissions(conn, from)?
                }
            }
            (None, None) => Vec::new(),
        };
        let uuid = db::insert_role(conn, &name, &request.description)?;
        db::set_role_permissions(conn, &uuid, &ids)?;
        Ok(uuid)
    });

    match result {
        Ok(uuid) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "uuid": uuid })),
        )
            .into_response(),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => conflict_or_internal(&e),
    }
}

/// Demande de modification d'un rôle.
#[derive(Debug, Deserialize)]
pub struct UpdateRole {
    pub name: Option<String>,
    pub description: Option<String>,
    pub permissions: Option<Vec<String>>,
}

/// `PUT /roles/{uuid}` — modification d'un rôle **non livré**.
pub async fn update_role(
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    Json(request): Json<UpdateRole>,
) -> Response {
    let result = state.auth.db(|conn| {
        let Some(role) = db::find_role(conn, &uuid)? else {
            return Err("not_found".to_string());
        };
        if role.builtin {
            // `admin` et `lecteur` sont des repères : on les clone pour changer
            // quelque chose, sinon deux installations n'auraient plus la même
            // définition de « lecteur ».
            return Err("role_frozen".to_string());
        }

        if let Some(permissions) = &request.permissions {
            let ids = known_permissions(permissions).map_err(|e| e.to_string())?;
            db::set_role_permissions(conn, &uuid, &ids)?;
        }
        if request.name.is_some() || request.description.is_some() {
            let name = request.name.clone().unwrap_or_else(|| role.name.clone());
            let description = request
                .description
                .clone()
                .unwrap_or_else(|| role.description.clone());
            db::update_role(conn, &uuid, &name, &description)?;
        }
        Ok(())
    });

    match result {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) if e == "not_found" => auth::error(StatusCode::NOT_FOUND, "not_found"),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => conflict_or_internal(&e),
    }
}

/// `DELETE /roles/{uuid}` — supprime un rôle **non livré**.
pub async fn delete_role(State(state): State<AppState>, Path(uuid): Path<String>) -> Response {
    let result = state.auth.db(|conn| {
        let Some(role) = db::find_role(conn, &uuid)? else {
            return Err("not_found".to_string());
        };
        if role.builtin {
            return Err("role_frozen".to_string());
        }
        db::delete_role(conn, &uuid)?;
        // Le rôle par défaut ne doit pas continuer à désigner un rôle disparu :
        // un nouveau compte se retrouverait sans rien, sans que rien ne le dise.
        if db::setting(conn, db::DEFAULT_ROLE)?.as_deref() == Some(uuid.as_str()) {
            db::set_setting(conn, db::DEFAULT_ROLE, None)?;
        }
        Ok(())
    });

    match result {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) if e == "not_found" => auth::error(StatusCode::NOT_FOUND, "not_found"),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => auth::internal(&e),
    }
}

/// Demande de choix du rôle par défaut.
#[derive(Debug, Deserialize)]
pub struct DefaultRole {
    /// `None` = aucun rôle par défaut.
    pub uuid: Option<String>,
}

/// `PUT /roles/default` — rôle attribué aux nouveaux comptes.
///
/// Il ne s'applique **pas** aux comptes existants : changer le défaut ne doit pas
/// redistribuer des droits à qui que ce soit.
pub async fn set_default_role(
    State(state): State<AppState>,
    Json(request): Json<DefaultRole>,
) -> Response {
    match state.auth.db(|conn| {
        match &request.uuid {
            Some(uuid) => {
                if db::find_role(conn, uuid)?.is_none() {
                    return Err("unknown_role".to_string());
                }
                db::set_setting(conn, db::DEFAULT_ROLE, Some(uuid))
            }
            None => db::set_setting(conn, db::DEFAULT_ROLE, None),
        }
    }) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) if is_client_error(&e) => auth::error(StatusCode::BAD_REQUEST, &e),
        Err(e) => auth::internal(&e),
    }
}

/// Demande de changement de son propre mot de passe.
#[derive(Debug, Deserialize)]
pub struct PasswordChange {
    /// Mot de passe actuel — ignoré si le compte n'en a pas encore (compte OIDC).
    pub current: String,
    pub new: String,
}

/// `POST /auth/password` — chacun change son propre mot de passe.
///
/// Le mot de passe actuel est exigé : une session ouverte sur un navigateur
/// laissé sans surveillance ne doit pas permettre de s'approprier le compte.
///
/// Les sessions sont **toutes** fermées, y compris celle qui fait la demande,
/// puis une nouvelle est ouverte et renvoyée au navigateur : un mot de passe
/// changé doit rendre inopérants les jetons de session qui traînaient, sans pour
/// autant déconnecter celui qui vient de le changer.
pub async fn change_password(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    caller: AuthUser,
    Json(request): Json<PasswordChange>,
) -> Response {
    let stored = caller.0.password_hash.clone();
    let current = request.current.clone();
    let ok = match &stored {
        Some(hash) => {
            let hash = hash.clone();
            tokio::task::spawn_blocking(move || password::verify(&current, &hash))
                .await
                .unwrap_or(false)
        }
        // Compte sans mot de passe (créé pour OIDC) : le premier qu'on choisit
        // s'installe sans vérification — il n'y a rien à vérifier.
        None => true,
    };
    if !ok {
        return auth::error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }

    if let Err(code) = password::check_length(&request.new) {
        return auth::error(StatusCode::BAD_REQUEST, code);
    }
    let hash = match password::hash(&request.new) {
        Ok(hash) => hash,
        Err(e) => return auth::internal(&e),
    };

    let uuid = caller.0.uuid.clone();
    if let Err(e) = state.auth.db(|conn| {
        db::set_password_hash(conn, &uuid, &hash)?;
        db::delete_sessions_of(conn, &uuid)?;
        Ok(())
    }) {
        return auth::internal(&e);
    }

    let token = match state.auth.start_session(&uuid, &headers) {
        Ok(token) => token,
        Err(e) => return auth::internal(&e),
    };

    (
        StatusCode::OK,
        [(
            axum::http::header::SET_COOKIE,
            state.auth.session_cookie(&token),
        )],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// Rôle attribué par défaut aux nouveaux comptes, s'il est encore valide.
///
/// Le réglage peut désigner un rôle supprimé depuis : on l'ignore alors, plutôt
/// que de faire échouer la création d'un compte pour une valeur devenue caduque.
fn default_role(state: &AppState) -> Vec<String> {
    default_roles(&state.auth)
}

/// Variante par état d'authentification, pour le provisionnement OIDC (qui
/// n'applique rien lui-même : il passe par [`insert_account`]).
fn default_roles(auth: &Auth) -> Vec<String> {
    auth.db(|conn| {
        let Some(uuid) = db::setting(conn, db::DEFAULT_ROLE)? else {
            return Ok(None);
        };
        Ok(db::find_role(conn, &uuid)?.map(|role| role.uuid))
    })
    .ok()
    .flatten()
    .into_iter()
    .collect()
}

/// Vérifie une liste d'UUID de rôles : tous doivent exister.
fn known_roles(state: &AppState, ids: &[String]) -> Result<Vec<String>, &'static str> {
    state
        .auth
        .db(|conn| {
            for id in ids {
                if db::find_role(conn, id)?.is_none() {
                    return Ok(false);
                }
            }
            Ok(true)
        })
        .map_err(|_| "internal_error")?
        .then(|| ids.to_vec())
        .ok_or("unknown_role")
}

/// Filtre une liste de droits : tout identifiant inconnu fait échouer l'écriture.
///
/// On refuse plutôt que d'ignorer : un droit qu'on croit avoir accordé et que le
/// serveur n'applique pas est bien pire qu'un message d'erreur.
fn known_permissions(ids: &[String]) -> Result<Vec<String>, &'static str> {
    if ids.iter().all(|id| permissions::is_known(id)) {
        Ok(ids.to_vec())
    } else {
        Err("unknown_permission")
    }
}

/// `true` si le message d'erreur est un refus destiné à l'utilisateur.
fn is_client_error(message: &str) -> bool {
    matches!(
        message,
        "last_admin"
            | "cannot_delete_self"
            | "unknown_permission"
            | "unknown_role"
            | "role_frozen"
            | "name_required"
    )
}

/// Erreur d'écriture : conflit d'unicité (409) ou panne (500).
///
/// Un nom d'utilisateur ou de rôle déjà pris est un **conflit**, pas une erreur
/// du serveur : c'est `conflict` que l'interface doit traduire en « ce nom est
/// déjà utilisé ».
fn conflict_or_internal(message: &str) -> Response {
    let is_conflict = message.contains("UNIQUE constraint failed");
    let code = if message.contains("users.username") {
        "username_taken"
    } else if message.contains("users.email") {
        "email_taken"
    } else if message.contains("roles.name") {
        "name_taken"
    } else {
        "internal_error"
    };

    if is_conflict {
        auth::error(StatusCode::CONFLICT, code)
    } else {
        auth::internal(message)
    }
}
