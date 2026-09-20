//! Schéma et requêtes de la base des comptes (`easy3d.db`, SQLite).
//!
//! **Pourquoi SQLite et pas le YAML de la configuration ?** `config.yml` est
//! réécrit **en entier** par l'interface (`Config::save`, sans fichier
//! temporaire) : il n'a ni transaction, ni contrainte d'unicité, ni verrou. Très
//! bien pour trois valeurs qu'un humain règle une fois, inadapté à des comptes
//! — deux inscriptions simultanées s'y perdraient. Règle retenue : **YAML = ce
//! qu'un humain règle, SQLite = ce que l'application gère.**
//!
//! Le fichier vit **à côté de `config.yml`**, jamais dans `models/` : le watcher
//! le verrait comme un modèle et sa réécriture se rediffuserait en boucle.
//!
//! Deux règles de forme, valables pour tout le sous-système :
//!
//! - **UUID partout** (jamais d'entier auto-incrémenté) : un identifiant qui
//!   passe dans une URL ne dit rien du nombre de comptes, et reste valable après
//!   un export puis un ré-import.
//! - **Aucun `deny`** : les droits s'additionnent (rôles ∪ droits posés sur le
//!   compte), donc pas de colonne « allowed ». Le rôle [`ADMIN_ROLE`] est un
//!   superutilisateur **codé** (`auth::is_admin`) : il n'a pas besoin d'être
//!   listé dans `role_permissions`.
//!
//! Le schéma est créé au premier démarrage et **versionné** (`PRAGMA
//! user_version`) pour que les étapes suivantes s'ajoutent sans migration à la
//! main. Toutes les tables sont créées dès maintenant : elles ne coûtent rien
//! tant que le RBAC et les jetons ne sont pas branchés.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// UUID du rôle livré `admin`.
///
/// Volontairement **constant** : le rôle doit être reconnaissable en base après
/// un redémarrage (c'est lui que `count_admins` compte), et un UUID dérivé
/// d'une fonction de hachage ajouterait une dépendance pour rien.
pub const ADMIN_ROLE: &str = "0199e3d0-0000-7000-8000-000000000001";

/// UUID du rôle livré `lecteur` (tout voir, rien modifier).
pub const READER_ROLE: &str = "0199e3d0-0000-7000-8000-000000000002";

/// Version du schéma. À incrémenter en ajoutant une table ou une colonne.
const SCHEMA_VERSION: i64 = 2;

/// Schéma initial (v1).
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    uuid          TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    email         TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- NULL = compte qui ne se connecte pas par mot de passe (créé par OIDC).
    password_hash TEXT,
    -- Identifiant stable chez le fournisseur OIDC, lié au premier login.
    oidc_subject  TEXT UNIQUE,
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS roles (
    uuid        TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    description TEXT NOT NULL DEFAULT '',
    -- 1 = rôle livré : `admin` et `lecteur` (figés, mais clonables).
    builtin     INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_roles (
    user_uuid TEXT NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    role_uuid TEXT NOT NULL REFERENCES roles(uuid) ON DELETE CASCADE,
    PRIMARY KEY (user_uuid, role_uuid)
);

-- Droits accordés par un rôle, et droits accordés à un compte en propre.
-- Les deux s'additionnent ; il n'existe aucun refus explicite.
CREATE TABLE IF NOT EXISTS role_permissions (
    role_uuid  TEXT NOT NULL REFERENCES roles(uuid) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role_uuid, permission)
);

CREATE TABLE IF NOT EXISTS user_permissions (
    user_uuid  TEXT NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (user_uuid, permission)
);

-- Sessions ouvertes : le cookie porte `token` (opaque, 32 octets aléatoires),
-- jamais l'UUID du compte — révoquer une session ne touche pas au compte.
CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_uuid  TEXT NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    user_agent TEXT NOT NULL DEFAULT '',
    ip         TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_uuid);
CREATE INDEX IF NOT EXISTS sessions_expires ON sessions(expires_at);

-- Jetons d'API (agents, clients MCP) : seul le **hachage** est stocké, le jeton
-- en clair n'existe qu'une fois, dans la réponse qui le crée.
CREATE TABLE IF NOT EXISTS api_tokens (
    uuid         TEXT PRIMARY KEY,
    user_uuid    TEXT NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    expires_at   INTEGER
);

-- Réglages gérés par l'application (mode de provisionnement OIDC, rôle par
-- défaut…). Ce qui se règle **à la main** reste dans `config.yml`.
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Schéma v2 : **journal d'audit**.
///
/// Qui a fait quoi, et quand : les connexions (réussies, refusées, bloquées),
/// les jetons créés ou révoqués, et les changements de droits. C'est ce qu'on
/// cherche le jour où un accès surprend — un serveur qui ne garde que des
/// traces de console n'a rien à montrer.
///
/// Ce qui n'y est **pas**, volontairement : ni les valeurs de jeton (jamais
/// stockées en clair), ni le détail du catalogue consulté (bruit inutile).
const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS auth_events (
    uuid       TEXT PRIMARY KEY,
    at         INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    -- Qui agit : NULL quand il n'y a pas de compte (connexion refusée) ou que
    -- l'acteur a disparu depuis.
    actor_uuid TEXT,
    -- Sur quoi / sur qui : identifiant essayé, compte visé, nom de rôle…
    subject    TEXT NOT NULL DEFAULT '',
    -- Précision courte et **stable** (code d'erreur), traduite par l'interface.
    detail     TEXT NOT NULL DEFAULT '',
    ip         TEXT NOT NULL DEFAULT '',
    user_agent TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS auth_events_at ON auth_events(at DESC);
CREATE INDEX IF NOT EXISTS auth_events_actor ON auth_events(actor_uuid);
"#;

/// Horodatage courant, en secondes depuis l'époque.
///
/// Les dates sont stockées en entier (et non en texte ISO) : les comparaisons
/// d'expiration sont celles de SQLite, sans conversion ni dépendance à un fuseau
/// horaire.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Erreur de base, préfixée pour être reconnaissable dans les logs.
fn err(e: rusqlite::Error) -> String {
    format!("base des comptes : {e}")
}

/// Ouvre la base et applique les migrations manquantes.
///
/// Le dossier est créé au besoin (`config/` peut manquer au premier lancement).
/// **Aucun `journal_mode = WAL`** : le fichier peut vivre sur un montage de
/// dossier Docker Desktop (Windows/macOS), où les fichiers annexes du WAL
/// (`-wal`, `-shm`) sont mal supportés ; le trafic d'authentification est de
/// toute façon trop faible pour que le mode WAL apporte quelque chose.
pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("dossier {} : {e}", dir.display()))?;
    }

    let conn = Connection::open(path).map_err(err)?;
    // Les contraintes de clés étrangères ne sont **pas** actives par défaut.
    conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(err)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Chemin par défaut : `easy3d.db` à côté de `config.yml`.
pub fn default_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("easy3d.db")
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(err)?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    // Ré-exécutable sans risque (`IF NOT EXISTS`) : une interruption au milieu
    // d'une migration laisse la base dans un état qui se rattrape au démarrage
    // suivant.
    if version < 1 {
        conn.execute_batch(SCHEMA_V1).map_err(err)?;

        // Une installation neuve part avec `lecteur` comme rôle par défaut : un
        // compte créé sans rôle explicite (par l'administrateur ou par OIDC)
        // sait alors consulter le catalogue, au lieu d'arriver sans aucun droit
        // et sans explication. Le remplissage est **ici**, dans la migration, et
        // non à chaque démarrage : un administrateur qui remet le réglage à
        // « aucun » ne doit pas le voir ressusciter au redémarrage suivant.
        set_setting(conn, DEFAULT_ROLE, Some(READER_ROLE))?;
    }
    if version < 2 {
        conn.execute_batch(SCHEMA_V2).map_err(err)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(err)?;
    Ok(())
}

/// Compte utilisateur, tel qu'il est stocké.
///
/// ⚠️ **Jamais sérialisé tel quel** : il porte le hachage du mot de passe. Ce
/// qui sort de l'API passe par `auth::UserView`, qui n'a pas ce champ (même
/// raison que `config::KEY_PLACEHOLDER` pour la clé d'API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub uuid: String,
    pub username: String,
    pub email: String,
    pub password_hash: Option<String>,
    pub oidc_subject: Option<String>,
    pub disabled: bool,
    pub created_at: i64,
}

/// Compte en cours de création (UUID et date générés par [`NewUser::new`]).
#[derive(Debug, Clone)]
pub struct NewUser {
    pub uuid: String,
    pub username: String,
    pub email: String,
    pub password_hash: Option<String>,
    pub oidc_subject: Option<String>,
    pub created_at: i64,
}

impl NewUser {
    /// Nouveau compte, identifié par un **UUIDv7** : trié par date de création,
    /// donc les parcours en base restent ordonnés comme les insertions.
    pub fn new(username: &str, email: &str, password_hash: Option<String>) -> Self {
        Self {
            uuid: uuid::Uuid::now_v7().to_string(),
            username: username.trim().to_string(),
            email: email.trim().to_lowercase(),
            password_hash,
            oidc_subject: None,
            created_at: now(),
        }
    }
}

const USER_COLUMNS: &str = "uuid, username, email, password_hash, oidc_subject, disabled, created_at";

fn row_to_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        uuid: row.get(0)?,
        username: row.get(1)?,
        email: row.get(2)?,
        password_hash: row.get(3)?,
        oidc_subject: row.get(4)?,
        disabled: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
    })
}

/// Compte correspondant à un identifiant de connexion : **nom d'utilisateur ou
/// adresse e-mail**, sans tenir compte de la casse (les deux colonnes sont
/// déclarées `COLLATE NOCASE`).
pub fn find_by_login(conn: &Connection, login: &str) -> Result<Option<User>, String> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1 OR email = ?1 LIMIT 1"),
        params![login],
        row_to_user,
    )
    .optional()
    .map_err(err)
}

/// Compte par UUID.
pub fn find_by_uuid(conn: &Connection, uuid: &str) -> Result<Option<User>, String> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE uuid = ?1"),
        params![uuid],
        row_to_user,
    )
    .optional()
    .map_err(err)
}

/// Compte par adresse e-mail (rattachement OIDC, création par l'administrateur).
pub fn find_by_email(conn: &Connection, email: &str) -> Result<Option<User>, String> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE email = ?1"),
        params![email],
        row_to_user,
    )
    .optional()
    .map_err(err)
}

/// Compte par identifiant du fournisseur d'identité (`sub` OIDC).
///
/// C'est **la** clé de reconnaissance une fois le compte rattaché : l'e-mail,
/// lui, peut changer chez le fournisseur sans casser le compte.
pub fn find_by_subject(conn: &Connection, subject: &str) -> Result<Option<User>, String> {
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE oidc_subject = ?1"),
        params![subject],
        row_to_user,
    )
    .optional()
    .map_err(err)
}

/// Rattache un compte à une identité du fournisseur.
///
/// L'écriture n'a lieu que si le compte n'est pas déjà rattaché : un `sub`
/// appartient à un seul compte (contrainte d'unicité), et laisser une écriture
/// l'écraser silencieusement changerait qui peut entrer.
pub fn set_oidc_subject(conn: &Connection, uuid: &str, subject: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE users SET oidc_subject = ?2 WHERE uuid = ?1 AND oidc_subject IS NULL",
        params![uuid, subject],
    )
    .map(|_| ())
    .map_err(err)
}

/// Insère un compte.
///
/// Les conflits (nom ou e-mail déjà pris) remontent tels quels : c'est la base
/// qui garantit l'unicité, pas un `SELECT` préalable qui laisserait une fenêtre
/// entre la vérification et l'écriture.
pub fn insert_user(conn: &Connection, user: &NewUser) -> Result<(), String> {
    conn.execute(
        "INSERT INTO users (uuid, username, email, password_hash, oidc_subject, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            user.uuid,
            user.username,
            user.email,
            user.password_hash,
            user.oidc_subject,
            user.created_at
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// Insère les rôles livrés s'ils manquent (`admin`, `lecteur`).
///
/// Ils sont **figés** : ni renommables ni supprimables, mais clonables pour
/// créer un rôle sur mesure. Les noms sont donc réécrits à chaque démarrage,
/// pour qu'une modification manuelle en base ne laisse pas deux rôles dont l'un
/// prétend être l'`admin`.
pub fn ensure_builtin_roles(conn: &Connection) -> Result<(), String> {
    let roles = [
        (ADMIN_ROLE, "admin", "Accès complet (superutilisateur)"),
        (READER_ROLE, "lecteur", "Consulte le catalogue, sans modification"),
    ];
    for (uuid, name, description) in roles {
        conn.execute(
            "INSERT INTO roles (uuid, name, description, builtin, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(uuid) DO UPDATE SET name = excluded.name,
                                             description = excluded.description,
                                             builtin = 1",
            params![uuid, name, description, now()],
        )
        .map_err(err)?;
    }

    // Les droits du rôle `lecteur` sont **réappliqués** à chaque démarrage : le
    // rôle est figé, donc rien de ce qu'on y modifierait à la main ne doit
    // survivre — c'est ce qui garantit la même définition d'une installation à
    // l'autre. `admin`, lui, n'a pas besoin d'être rempli : il est
    // superutilisateur par construction (voir `rbac::is_admin`).
    set_role_permissions(
        conn,
        READER_ROLE,
        &[crate::auth::permissions::CATALOG_READ.to_string()],
    )?;
    Ok(())
}

/// Noms de tous les rôles, par ordre alphabétique.
pub fn role_names(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT name FROM roles ORDER BY name")
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Attribue un rôle à un compte (sans effet s'il l'a déjà).
pub fn assign_role(conn: &Connection, user_uuid: &str, role_uuid: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO user_roles (user_uuid, role_uuid) VALUES (?1, ?2)",
        params![user_uuid, role_uuid],
    )
    .map(|_| ())
    .map_err(err)
}

/// `true` si le compte porte ce rôle.
pub fn has_role(conn: &Connection, user_uuid: &str, role_uuid: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM user_roles WHERE user_uuid = ?1 AND role_uuid = ?2",
        params![user_uuid, role_uuid],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(err)
}

/// Noms des rôles d'un compte, par ordre alphabétique.
pub fn roles_of(conn: &Connection, user_uuid: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.name FROM roles r
             JOIN user_roles ur ON ur.role_uuid = r.uuid
             WHERE ur.user_uuid = ?1
             ORDER BY r.name",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![user_uuid], |row| row.get::<_, String>(0))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Nombre d'administrateurs **actifs**.
///
/// Sert à deux choses : refuser d'ignorer `EASY3D_ADMIN_*` quand il n'y a
/// vraiment personne, et (étape suivante) empêcher de supprimer ou désactiver le
/// dernier administrateur — le superutilisateur étant codé, un catalogue sans
/// aucun `admin` ne serait plus administrable.
pub fn count_admins(conn: &Connection) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM user_roles ur
         JOIN users u ON u.uuid = ur.user_uuid
         WHERE ur.role_uuid = ?1 AND u.disabled = 0",
        params![ADMIN_ROLE],
        |row| row.get(0),
    )
    .map_err(err)
}

/// Enregistre une session ouverte.
pub fn create_session(
    conn: &Connection,
    token: &str,
    user_uuid: &str,
    expires_at: i64,
    user_agent: &str,
    ip: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sessions (token, user_uuid, created_at, expires_at, user_agent, ip)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![token, user_uuid, now(), expires_at, user_agent, ip],
    )
    .map(|_| ())
    .map_err(err)
}

/// Compte d'une session valide (non expirée, compte toujours actif).
///
/// Le compte désactivé est filtré **ici** : couper l'accès à quelqu'un prend
/// effet à la requête suivante, sans avoir à retrouver ses sessions.
pub fn session_user(conn: &Connection, token: &str, at: i64) -> Result<Option<User>, String> {
    conn.query_row(
        &format!(
            "SELECT {} FROM sessions s
             JOIN users u ON u.uuid = s.user_uuid
             WHERE s.token = ?1 AND s.expires_at > ?2 AND u.disabled = 0",
            USER_COLUMNS
                .split(", ")
                .map(|c| format!("u.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        params![token, at],
        row_to_user,
    )
    .optional()
    .map_err(err)
}

/// Ferme une session (déconnexion). Sans effet si le jeton est inconnu.
pub fn delete_session(conn: &Connection, token: &str) -> Result<(), String> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])
        .map(|_| ())
        .map_err(err)
}

/// Retire les sessions expirées. Renvoie le nombre de lignes supprimées.
pub fn purge_expired_sessions(conn: &Connection) -> Result<usize, String> {
    conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now()])
        .map_err(err)
}

/// Ferme **toutes** les sessions d'un compte (changement de mot de passe,
/// désactivation).
pub fn delete_sessions_of(conn: &Connection, user_uuid: &str) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM sessions WHERE user_uuid = ?1",
        params![user_uuid],
    )
    .map_err(err)
}

/// Clé du réglage « rôle par défaut », dans la table `settings`.
///
/// Il désigne un rôle **de la base** : c'est donc un réglage géré par
/// l'application (choisi dans l'interface), pas une valeur de `config.yml` — un
/// UUID de rôle n'a rien à faire dans un fichier que l'on édite à la main.
pub const DEFAULT_ROLE: &str = "auth.default_role";

/// Un rôle, tel qu'il est stocké.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub uuid: String,
    pub name: String,
    pub description: String,
    /// `true` pour `admin` et `lecteur` : figés (ni renommables ni supprimables),
    /// mais **clonables** — c'est ainsi qu'on part d'un rôle livré pour en faire
    /// un sur mesure.
    pub builtin: bool,
}

const ROLE_COLUMNS: &str = "uuid, name, description, builtin";

fn row_to_role(row: &rusqlite::Row<'_>) -> rusqlite::Result<Role> {
    Ok(Role {
        uuid: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        builtin: row.get::<_, i64>(3)? != 0,
    })
}

/// Tous les rôles : les rôles livrés d'abord (leur ordre d'insertion), puis les
/// autres par ordre alphabétique.
pub fn list_roles(conn: &Connection) -> Result<Vec<Role>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ROLE_COLUMNS} FROM roles ORDER BY builtin DESC, name"
        ))
        .map_err(err)?;
    let rows = stmt.query_map([], row_to_role).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Un rôle par UUID.
pub fn find_role(conn: &Connection, uuid: &str) -> Result<Option<Role>, String> {
    conn.query_row(
        &format!("SELECT {ROLE_COLUMNS} FROM roles WHERE uuid = ?1"),
        params![uuid],
        row_to_role,
    )
    .optional()
    .map_err(err)
}

/// Crée un rôle et renvoie son UUID.
///
/// Le nom est unique (contrainte de la base) : deux rôles homonymes rendraient
/// l'interface et les journaux ambigus.
pub fn insert_role(conn: &Connection, name: &str, description: &str) -> Result<String, String> {
    let uuid = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO roles (uuid, name, description, builtin, created_at)
         VALUES (?1, ?2, ?3, 0, ?4)",
        params![uuid, name.trim(), description.trim(), now()],
    )
    .map_err(err)?;
    Ok(uuid)
}

/// Renomme et redécrit un rôle **non livré**.
pub fn update_role(conn: &Connection, uuid: &str, name: &str, description: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE roles SET name = ?2, description = ?3 WHERE uuid = ?1 AND builtin = 0",
        params![uuid, name.trim(), description.trim()],
    )
    .map(|_| ())
    .map_err(err)
}

/// Supprime un rôle **non livré** (ses liens suivent en cascade).
pub fn delete_role(conn: &Connection, uuid: &str) -> Result<(), String> {
    conn.execute("DELETE FROM roles WHERE uuid = ?1 AND builtin = 0", params![uuid])
        .map(|_| ())
        .map_err(err)
}

/// Nombre de comptes portant ce rôle.
pub fn count_role_members(conn: &Connection, role_uuid: &str) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM user_roles WHERE role_uuid = ?1",
        params![role_uuid],
        |row| row.get(0),
    )
    .map_err(err)
}

/// Droits accordés par un rôle.
pub fn role_permissions(conn: &Connection, role_uuid: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT permission FROM role_permissions WHERE role_uuid = ?1 ORDER BY permission")
        .map_err(err)?;
    let rows = stmt
        .query_map(params![role_uuid], |row| row.get::<_, String>(0))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Remplace les droits d'un rôle (les anciens sont effacés).
///
/// Suppression **et** insertions dans la même transaction : la liste est soit
/// l'ancienne, soit la nouvelle, jamais un mélange des deux.
pub fn set_role_permissions(
    conn: &Connection,
    role_uuid: &str,
    ids: &[String],
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        "DELETE FROM role_permissions WHERE role_uuid = ?1",
        params![role_uuid],
    )
    .map_err(err)?;
    for id in ids {
        tx.execute(
            "INSERT OR IGNORE INTO role_permissions (role_uuid, permission) VALUES (?1, ?2)",
            params![role_uuid, id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

/// Droits posés **directement** sur un compte.
pub fn user_permissions(conn: &Connection, user_uuid: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT permission FROM user_permissions WHERE user_uuid = ?1 ORDER BY permission")
        .map_err(err)?;
    let rows = stmt
        .query_map(params![user_uuid], |row| row.get::<_, String>(0))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Remplace les droits directs d'un compte.
pub fn set_user_permissions(
    conn: &Connection,
    user_uuid: &str,
    ids: &[String],
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        "DELETE FROM user_permissions WHERE user_uuid = ?1",
        params![user_uuid],
    )
    .map_err(err)?;
    for id in ids {
        tx.execute(
            "INSERT OR IGNORE INTO user_permissions (user_uuid, permission) VALUES (?1, ?2)",
            params![user_uuid, id],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

/// Droits **effectifs** d'un compte : rôles ∪ droits directs.
///
/// C'est la seule requête qui compte pour les gardes : il n'y a pas de refus,
/// donc aucun ordre de priorité à respecter entre les deux sources.
pub fn effective_permissions(conn: &Connection, user_uuid: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT permission FROM role_permissions
             WHERE role_uuid IN (SELECT role_uuid FROM user_roles WHERE user_uuid = ?1)
             UNION
             SELECT permission FROM user_permissions WHERE user_uuid = ?1
             ORDER BY permission",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![user_uuid], |row| row.get::<_, String>(0))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Tous les comptes, par nom d'utilisateur.
pub fn list_users(conn: &Connection) -> Result<Vec<User>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {USER_COLUMNS} FROM users ORDER BY username"))
        .map_err(err)?;
    let rows = stmt.query_map([], row_to_user).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Rôles d'un compte, avec leurs noms (UUID + nom), par ordre alphabétique.
///
/// L'interface a besoin des deux : l'UUID pour cocher la bonne case, le nom pour
/// l'afficher.
pub fn roles_named(conn: &Connection, user_uuid: &str) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT r.uuid, r.name FROM roles r
             JOIN user_roles ur ON ur.role_uuid = r.uuid
             WHERE ur.user_uuid = ?1
             ORDER BY r.name",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![user_uuid], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Remplace les rôles d'un compte.
pub fn set_user_roles(
    conn: &Connection,
    user_uuid: &str,
    role_uuids: &[String],
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute(
        "DELETE FROM user_roles WHERE user_uuid = ?1",
        params![user_uuid],
    )
    .map_err(err)?;
    for role in role_uuids {
        tx.execute(
            "INSERT OR IGNORE INTO user_roles (user_uuid, role_uuid) VALUES (?1, ?2)",
            params![user_uuid, role],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)
}

/// Met à jour le nom et l'adresse e-mail d'un compte.
pub fn update_identity(conn: &Connection, uuid: &str, username: &str, email: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE users SET username = ?2, email = ?3 WHERE uuid = ?1",
        params![uuid, username.trim(), email.trim().to_lowercase()],
    )
    .map(|_| ())
    .map_err(err)
}

/// Remplace l'empreinte du mot de passe.
pub fn set_password_hash(conn: &Connection, uuid: &str, hash: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE users SET password_hash = ?2 WHERE uuid = ?1",
        params![uuid, hash],
    )
    .map(|_| ())
    .map_err(err)
}

/// Désactive ou réactive un compte.
pub fn set_disabled(conn: &Connection, uuid: &str, disabled: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE users SET disabled = ?2 WHERE uuid = ?1",
        params![uuid, i64::from(disabled)],
    )
    .map(|_| ())
    .map_err(err)
}

/// Supprime un compte (ses sessions, rôles et droits suivent en cascade).
pub fn delete_user(conn: &Connection, uuid: &str) -> Result<(), String> {
    conn.execute("DELETE FROM users WHERE uuid = ?1", params![uuid])
        .map(|_| ())
        .map_err(err)
}

/// Lit un réglage applicatif (`None` s'il n'a jamais été posé).
pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(err)
}

/// Écrit un réglage applicatif (ou l'efface, avec `None`).
pub fn set_setting(conn: &Connection, key: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ()),
        None => conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map(|_| ()),
    }
    .map_err(err)
}

// ── Jetons d'API ────────────────────────────────────────────────────────

/// Un jeton d'API, tel qu'il est **stocké** — le jeton lui-même n'y est pas.
///
/// Seul son empreinte est écrite en base : un jeton en clair n'existe qu'une
/// fois, dans la réponse qui le crée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiToken {
    pub uuid: String,
    pub name: String,
    pub created_at: i64,
    /// Dernière utilisation (dit si le jeton sert encore).
    pub last_used_at: Option<i64>,
    pub expires_at: Option<i64>,
}

const TOKEN_COLUMNS: &str = "uuid, name, created_at, last_used_at, expires_at";

fn row_to_token(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApiToken> {
    Ok(ApiToken {
        uuid: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        last_used_at: row.get(3)?,
        expires_at: row.get(4)?,
    })
}

/// Enregistre un jeton (son empreinte, jamais le jeton).
///
/// `expires_at` reste facultatif : `NULL` veut dire « n'expire jamais », ce qui
/// est le cas de tous les jetons créés avant que le réglage existe.
pub fn insert_token(
    conn: &Connection,
    uuid: &str,
    user_uuid: &str,
    name: &str,
    token_hash: &str,
    expires_at: Option<i64>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO api_tokens (uuid, user_uuid, name, token_hash, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![uuid, user_uuid, name.trim(), token_hash, now(), expires_at],
    )
    .map(|_| ())
    .map_err(err)
}

/// Jetons d'un compte, les plus récents d'abord.
pub fn list_tokens(conn: &Connection, user_uuid: &str) -> Result<Vec<ApiToken>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {TOKEN_COLUMNS} FROM api_tokens
             WHERE user_uuid = ?1 ORDER BY created_at DESC"
        ))
        .map_err(err)?;
    let rows = stmt.query_map(params![user_uuid], row_to_token).map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

/// Compte associé à l'empreinte d'un jeton (avec le jeton, pour son identifiant).
pub fn find_token(conn: &Connection, token_hash: &str) -> Result<Option<(ApiToken, String)>, String> {
    conn.query_row(
        &format!("SELECT {TOKEN_COLUMNS}, user_uuid FROM api_tokens WHERE token_hash = ?1"),
        params![token_hash],
        |row| Ok((row_to_token(row)?, row.get::<_, String>(5)?)),
    )
    .optional()
    .map_err(err)
}

/// Note qu'un jeton vient de servir.
pub fn touch_token(conn: &Connection, uuid: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE api_tokens SET last_used_at = ?2 WHERE uuid = ?1",
        params![uuid, now()],
    )
    .map(|_| ())
    .map_err(err)
}

/// Révoque un jeton **du compte** (renvoie `false` s'il n'existe pas ou
/// appartient à quelqu'un d'autre).
pub fn delete_token(conn: &Connection, user_uuid: &str, uuid: &str) -> Result<bool, String> {
    conn.execute(
        "DELETE FROM api_tokens WHERE uuid = ?1 AND user_uuid = ?2",
        params![uuid, user_uuid],
    )
    .map(|count| count > 0)
    .map_err(err)
}

/// Retire les jetons expirés. Renvoie le nombre de lignes supprimées.
pub fn purge_expired_tokens(conn: &Connection) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM api_tokens WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now()],
    )
    .map_err(err)
}

// ── Journal d'audit ────────────────────────────────────────────────────

/// Nombre d'événements conservés (garde-fou d'écriture).
///
/// Assez pour retrouver « qui s'est connecté la semaine dernière » sur une
/// petite installation, assez peu pour que le fichier reste modeste. Au-delà,
/// les plus anciens partent : un journal sans borne finirait par peser plus
/// lourd que les comptes eux-mêmes.
const MAX_EVENTS: i64 = 5000;

/// Un événement du journal, tel qu'il est stocké.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthEvent {
    pub uuid: String,
    pub at: i64,
    /// Code stable (`login_ok`, `token_created`…) — l'interface traduit.
    pub kind: String,
    pub actor_uuid: Option<String>,
    /// Identifiant essayé, compte visé, nom de rôle…
    pub subject: String,
    /// Précision courte : code d'erreur, mode de provisionnement…
    pub detail: String,
    pub ip: String,
    pub user_agent: String,
}

const EVENT_COLUMNS: &str = "uuid, at, kind, actor_uuid, subject, detail, ip, user_agent";

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuthEvent> {
    Ok(AuthEvent {
        uuid: row.get(0)?,
        at: row.get(1)?,
        kind: row.get(2)?,
        actor_uuid: row.get(3)?,
        subject: row.get(4)?,
        detail: row.get(5)?,
        ip: row.get(6)?,
        user_agent: row.get(7)?,
    })
}

/// Écrit un événement, et écarte les plus anciens si le journal est plein.
///
/// L'écriture ne doit **jamais** faire échouer l'action qu'elle raconte : un
/// journal est un témoin, pas une condition. Les appelants ignorent donc le
/// résultat (`let _ = …`), et une erreur ici se contente d'un avertissement.
///
/// ⚠️ Les dates sont en **secondes**, comme partout ailleurs : deux événements de
/// la même seconde peuvent donc revenir dans n'importe quel ordre entre eux.
/// C'est sans conséquence pour ce qu'on vient y chercher (qui, quoi, quand), et
/// cela évite une colonne de plus pour un ordre que personne ne lit.
pub fn log_event(
    conn: &Connection,
    kind: &str,
    actor_uuid: Option<&str>,
    subject: &str,
    detail: &str,
    ip: &str,
    user_agent: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO auth_events (uuid, at, kind, actor_uuid, subject, detail, ip, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            uuid::Uuid::now_v7().to_string(),
            now(),
            kind,
            actor_uuid,
            subject,
            detail,
            ip,
            user_agent
        ],
    )
    .map_err(err)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM auth_events", [], |row| row.get(0))
        .map_err(err)?;
    if total > MAX_EVENTS {
        conn.execute(
            "DELETE FROM auth_events WHERE uuid IN (
                 SELECT uuid FROM auth_events ORDER BY at ASC, uuid ASC LIMIT ?1
             )",
            params![total - MAX_EVENTS],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Derniers événements, du plus récent au plus ancien.
pub fn list_events(conn: &Connection, limit: i64) -> Result<Vec<AuthEvent>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {EVENT_COLUMNS} FROM auth_events ORDER BY at DESC, uuid DESC LIMIT ?1"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 500)], row_to_event)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("easy3d.db")).unwrap();
        (dir, conn)
    }

    #[test]
    fn la_base_est_creee_et_le_schema_n_est_applique_qu_une_fois() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("easy3d.db");

        let conn = open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        drop(conn);

        // Réouvrir ne doit rien casser (et ne doit pas rejouer le schéma).
        let conn = open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn une_base_neuve_donne_le_role_lecteur_par_defaut() {
        let (_dir, conn) = base();
        assert_eq!(
            setting(&conn, DEFAULT_ROLE).unwrap().as_deref(),
            Some(READER_ROLE)
        );

        // Un réglage vidé volontairement ne revient pas : seul le remplissage
        // initial compte, pas chaque démarrage.
        set_setting(&conn, DEFAULT_ROLE, None).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(setting(&conn, DEFAULT_ROLE).unwrap(), None);
    }

    #[test]
    fn les_roles_livres_sont_crees_et_stables() {
        let (_dir, conn) = base();
        ensure_builtin_roles(&conn).unwrap();
        ensure_builtin_roles(&conn).unwrap();
        assert_eq!(role_names(&conn).unwrap(), vec!["admin", "lecteur"]);
    }

    #[test]
    fn un_compte_se_retrouve_par_son_nom_ou_par_son_email() {
        let (_dir, conn) = base();
        let user = NewUser::new("Remi", "Remi@Exemple.fr", None);
        // L'e-mail est normalisé en minuscules à la création.
        assert_eq!(user.email, "remi@exemple.fr");
        insert_user(&conn, &user).unwrap();

        for login in ["Remi", "remi", "REMI", "remi@exemple.fr", "REMI@EXEMPLE.FR"] {
            let found = find_by_login(&conn, login).unwrap();
            assert_eq!(found.map(|u| u.uuid), Some(user.uuid.clone()), "login : {login}");
        }
        assert!(find_by_login(&conn, "inconnu").unwrap().is_none());
    }

    #[test]
    fn le_nom_et_l_email_sont_uniques() {
        let (_dir, conn) = base();
        insert_user(&conn, &NewUser::new("remi", "a@exemple.fr", None)).unwrap();

        // Deuxième compte avec le même nom (casse différente comprise).
        assert!(insert_user(&conn, &NewUser::new("REMI", "b@exemple.fr", None)).is_err());
        // …et avec le même e-mail.
        assert!(insert_user(&conn, &NewUser::new("autre", "a@exemple.fr", None)).is_err());
    }

    #[test]
    fn une_session_expiree_ou_un_compte_desactive_ne_donnent_plus_acces() {
        let (_dir, conn) = base();
        let user = NewUser::new("remi", "remi@exemple.fr", None);
        insert_user(&conn, &user).unwrap();

        let maintenant = now();
        create_session(&conn, "jeton", &user.uuid, maintenant + 60, "", "").unwrap();
        assert!(session_user(&conn, "jeton", maintenant).unwrap().is_some());

        // Expirée.
        assert!(
            session_user(&conn, "jeton", maintenant + 61)
                .unwrap()
                .is_none()
        );

        // Compte désactivé : la session en cours cesse de valoir.
        conn.execute("UPDATE users SET disabled = 1 WHERE uuid = ?1", params![user.uuid])
            .unwrap();
        assert!(session_user(&conn, "jeton", maintenant).unwrap().is_none());

        // Le ménage emporte les sessions expirées, pas les autres.
        conn.execute("UPDATE users SET disabled = 0 WHERE uuid = ?1", params![user.uuid])
            .unwrap();
        create_session(&conn, "perime", &user.uuid, maintenant - 1, "", "").unwrap();
        assert_eq!(purge_expired_sessions(&conn).unwrap(), 1);
        assert!(session_user(&conn, "jeton", maintenant).unwrap().is_some());
    }

    #[test]
    fn un_compte_desactive_ne_compte_pas_comme_administrateur() {
        let (_dir, conn) = base();
        ensure_builtin_roles(&conn).unwrap();
        let user = NewUser::new("remi", "remi@exemple.fr", None);
        insert_user(&conn, &user).unwrap();
        assert_eq!(count_admins(&conn).unwrap(), 0);

        assign_role(&conn, &user.uuid, ADMIN_ROLE).unwrap();
        assign_role(&conn, &user.uuid, ADMIN_ROLE).unwrap(); // idempotent
        assert_eq!(count_admins(&conn).unwrap(), 1);
        assert_eq!(roles_of(&conn, &user.uuid).unwrap(), vec!["admin"]);

        conn.execute("UPDATE users SET disabled = 1 WHERE uuid = ?1", params![user.uuid])
            .unwrap();
        assert_eq!(count_admins(&conn).unwrap(), 0);
    }

    #[test]
    fn les_sessions_d_un_compte_se_ferment_ensemble() {
        let (_dir, conn) = base();
        let user = NewUser::new("remi", "remi@exemple.fr", None);
        insert_user(&conn, &user).unwrap();

        let now = now();
        create_session(&conn, "a", &user.uuid, now + 60, "", "").unwrap();
        create_session(&conn, "b", &user.uuid, now + 60, "", "").unwrap();
        assert_eq!(delete_sessions_of(&conn, &user.uuid).unwrap(), 2);
        assert!(session_user(&conn, "a", now).unwrap().is_none());
    }

    #[test]
    fn le_chemin_par_defaut_est_a_cote_de_la_configuration() {
        let path = default_path(Path::new("/config/config.yml"));
        assert_eq!(path, PathBuf::from("/config/easy3d.db"));
    }

    #[test]
    fn le_journal_rend_les_evenements_du_plus_recent_au_plus_ancien() {
        let (_dir, conn) = base();
        log_event(
            &conn,
            "login_failed",
            None,
            "inconnu",
            "invalid_credentials",
            "10.0.0.1",
            "curl",
        )
        .unwrap();
        log_event(&conn, "login_ok", Some("u-1"), "remi", "password", "10.0.0.1", "firefox")
            .unwrap();
        log_event(&conn, "token_created", Some("u-1"), "ci", "30", "", "").unwrap();

        let events = list_events(&conn, 10).unwrap();
        assert_eq!(events.len(), 3);
        // Le plus récent d'abord : c'est ce qu'on vient lire.
        assert_eq!(events[0].kind, "token_created");
        assert_eq!(events[2].kind, "login_failed");
        assert_eq!(events[0].actor_uuid.as_deref(), Some("u-1"));
        assert_eq!(events[0].subject, "ci");
        assert_eq!(events[0].detail, "30");
        // Une tentative refusée n'a pas d'acteur : elle dit **qui a été essayé**.
        assert_eq!(events[2].actor_uuid, None);
        assert_eq!(events[2].subject, "inconnu");
        assert_eq!(events[2].ip, "10.0.0.1");

        // La lecture reste bornée : on ne demande jamais « tout le journal ».
        assert_eq!(list_events(&conn, 5000).unwrap().len(), 3);
    }

    #[test]
    fn le_journal_ne_grandit_pas_sans_fin() {
        let (_dir, conn) = base();
        // Une rafale de tentatives ratées, dans **une** transaction : c'est ce
        // que produirait une attaque, et le journal doit rester borné.
        conn.execute("BEGIN", []).unwrap();
        for i in 0..(MAX_EVENTS + 5) {
            log_event(&conn, "login_failed", None, &format!("tentative {i}"), "", "", "")
                .unwrap();
        }
        conn.execute("COMMIT", []).unwrap();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM auth_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, MAX_EVENTS, "le journal est plafonné");

        // Les plus anciens sont partis, les derniers sont là. L'ordre **entre**
        // événements de la même seconde n'est pas garanti : on ne teste donc que
        // l'appartenance.
        let gardes = list_events(&conn, 500).unwrap();
        assert!(
            gardes
                .iter()
                .any(|e| e.subject == format!("tentative {}", MAX_EVENTS + 4))
        );
        assert!(gardes.iter().all(|e| e.subject != "tentative 0"));
    }

    #[test]
    fn un_evenement_rate_n_empeche_pas_l_action() {
        let (_dir, conn) = base();
        // Table absente (base d'avant la v2) : l'appelant ignore l'échec, et
        // l'action racontée continue. C'est le contrat de `log_event`.
        conn.execute("DROP TABLE auth_events", []).unwrap();
        assert!(log_event(&conn, "login_ok", Some("u"), "remi", "", "", "").is_err());
    }
}
