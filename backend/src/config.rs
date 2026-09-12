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
        Self::load_from(&Self::config_path())
    }

    /// Charge la configuration depuis un fichier donné.
    ///
    /// Fichier absent **ou** YAML invalide → valeurs par défaut (mode 3D).
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_yaml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("⚠️  Configuration invalide dans {}: {e}. Défauts utilisés.", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// `true` si l'on doit afficher les modèles en image statique.
    pub fn is_image_mode(&self) -> bool {
        self.display.mode == DisplayMode::Image
    }

    /// Chemin **résolu** du dossier des modèles.
    ///
    /// Priorité : `models_root` (absolu, ou relatif à `backend/`), sinon la
    /// variable d'env `MODELS_ROOT`, sinon `../models` depuis `backend/`.
    ///
    /// Vit ici (et non dans `main.rs`) parce que trois endroits doivent
    /// résoudre le même chemin : le démarrage, le rechargement à chaud du
    /// watcher, et `PUT /config` qui doit valider ce que l'on enregistre.
    pub fn resolve_models_root(&self) -> PathBuf {
        if let Some(p) = self.models_root.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            let path = Path::new(p);
            return if path.is_absolute() {
                path.to_path_buf()
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
            };
        }

        std::env::var("MODELS_ROOT").map(PathBuf::from).unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../models")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_le_fichier_de_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yml");
        std::fs::write(&file, "models_root: ../models\ndisplay:\n  mode: \"image\"\n").unwrap();

        let cfg = Config::load_from(&file);
        assert_eq!(cfg.models_root.as_deref(), Some("../models"));
        assert_eq!(cfg.display.mode, DisplayMode::Image);
        assert!(cfg.is_image_mode());
    }

    #[test]
    fn mode_par_defaut_quand_absent() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yml");
        // `display` absent, `models_root` présent.
        std::fs::write(&file, "models_root: /tmp/models\n").unwrap();

        let cfg = Config::load_from(&file);
        assert_eq!(cfg.display.mode, DisplayMode::ThreeD);
        assert!(!cfg.is_image_mode());
    }

    #[test]
    fn fichier_absent_utilise_les_defauts() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_from(&dir.path().join("inexistant.yml"));

        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.display.mode, DisplayMode::ThreeD);
    }

    #[test]
    fn yaml_invalide_retombe_sur_les_defauts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yml");
        // `display` attendu comme table, pas comme séquence.
        std::fs::write(&file, "display: [1, 2]\n").unwrap();

        assert_eq!(Config::load_from(&file), Config::default());
    }

    #[test]
    fn le_mode_se_serialise_en_minuscules() {
        // Le frontend lit `config.display.mode` tel quel ("3d" | "image").
        assert_eq!(serde_json::to_string(&DisplayMode::ThreeD).unwrap(), "\"3d\"");
        assert_eq!(serde_json::to_string(&DisplayMode::Image).unwrap(), "\"image\"");
    }
}
