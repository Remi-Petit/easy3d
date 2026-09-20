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
