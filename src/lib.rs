//! Bibliothèque `easy3d` : scan de fichiers et surveillance de dossier.
//!
//! - [`api`] : serveur HTTP (axum) exposant les fichiers.
//! - [`scanner`] : scan récursif d'un dossier + métadonnées.
//! - [`watcher`] : description lisible des événements du debouncer.

pub mod api;
pub mod scanner;
pub mod watcher;
