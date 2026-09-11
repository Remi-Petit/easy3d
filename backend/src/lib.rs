//! Bibliothèque `easy3d` : scan de fichiers et surveillance de dossier.
//!
//! - [`api`] : serveur HTTP (axum) exposant les fichiers.
//! - [`scanner`] : scan récursif d'un dossier + métadonnées.
//! - [`watcher`] : description lisible des événements du debouncer.
//! - [`config`] : configuration application (fichier YAML).
//! - [`render`] : génération d'aperçus PNG côté backend (STL / OBJ / 3MF).
//! - [`notes`] : notes Markdown associées aux dossiers et fichiers.
//! - [`collab`] : édition collaborative des notes (CRDT Yjs).

pub mod api;
pub mod collab;
pub mod config;
pub mod notes;
pub mod render;
pub mod scanner;
pub mod watcher;
