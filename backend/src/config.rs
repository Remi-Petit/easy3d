//! Configuration de l'application, chargée depuis un fichier YAML.
//!
//! Source du fichier (par ordre de priorité) :
//! 1. Variable d'env `EASY3D_CONFIG=<chemin>`.
//! 2. `config.yml` dans le dossier `backend/` (à côté du `Cargo.toml`).
//!
//! En l'absence de fichier, on utilise les valeurs par défaut.
//!
//! Le fichier est **relu à chaud** : il est surveillé par le watcher (voir
//! `main.rs`). Toute modification est appliquée sans redémarrer le serveur, et
//! la nouvelle configuration est diffusée au frontend via le WebSocket.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Mode d'affichage des modèles côté interface.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayMode {
    /// Vue interactive (Three.js / WebGL).
    #[default]
    #[serde(rename = "3d")]
    ThreeD,
    /// Aperçu statique (image) si une image du même nom existe, sinon repli 3D.
    #[serde(rename = "image")]
    Image,
}

/// Options d'affichage.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default)]
    pub mode: DisplayMode,
}

/// Configuration de l'application.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Répertoire des modèles. Sinon : env `MODELS_ROOT`, sinon `../models`.
    #[serde(default)]
    pub models_root: Option<String>,
    /// Options d'affichage côté frontend.
    #[serde(default)]
    pub display: Display,
}

impl Config {
    /// Chemin du fichier de configuration.
    pub fn config_path() -> PathBuf {
        std::env::var("EASY3D_CONFIG").map(PathBuf::from).unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("config.yml")
        })
    }

    /// Charge la configuration depuis le YAML, avec repli sur les défauts.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_yaml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("⚠️  Configuration invalide dans {}: {e}. Défauts utilisés.", path.display());
                Self::default()
            }),
            Err(_) => {
                // Pas de fichier → défauts (mode 3D).
                Self::default()
            }
        }
    }

    /// `true` si l'on doit afficher les modèles en image statique.
    pub fn is_image_mode(&self) -> bool {
        self.display.mode == DisplayMode::Image
    }
}
