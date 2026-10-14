//! Bibliothèque `easy3d` : scan de fichiers et surveillance de dossier.
//!
//! - [`api`] : serveur HTTP (axum) exposant les fichiers.
//! - [`scanner`] : scan récursif d'un dossier + métadonnées.
//! - [`watcher`] : description lisible des événements du debouncer.
//! - [`config`] : configuration application (fichier YAML).
//! - [`formats`] : formats de fichiers reconnus (**un fichier par format**) et
//!   registre qui les expose au scan, aux aperçus et aux types MIME.
//! - [`thumbnail`] : cache et génération des aperçus PNG.
//! - [`mcp`] : serveur MCP (agents) monté sur `/mcp`, même process.
//! - [`notes`] : notes Markdown associées aux dossiers et fichiers.
//! - [`collab`] : édition collaborative des notes (CRDT Yjs).
//! - [`auth`] : comptes utilisateurs, sessions (facultatif, `EASY3D_AUTH`).

pub mod ai;
pub mod api;
pub mod auth;
pub mod collab;
pub mod config;
pub mod formats;
pub mod mcp;
pub mod notes;
pub mod scanner;
pub mod thumbnail;
pub mod watcher;

/// Réserve un fichier à son propriétaire (`0600`).
///
/// Deux fichiers de l'installation portent des secrets en clair : la base des
/// comptes (empreintes de mots de passe, sessions, journal) et `config.yml` (clé
/// d'API du fournisseur de modèles, secret client OIDC). Un `umask` à 022 — le
/// défaut courant — les crée en `0644`, donc lisibles par tout compte de la
/// machine. Ce n'est pas une protection contre quelqu'un qui a déjà `root`, mais
/// cela ferme la lecture « par curiosité » d'un compte voisin.
///
/// Sans effet sous Windows, où le modèle de permissions est différent : le
/// contrôle d'accès y vient de la liste de contrôle du dossier parent.
pub(crate) fn restrict_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "⚠️  permissions non restreintes sur {} : {e}",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
