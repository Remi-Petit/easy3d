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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Mode d'affichage des modèles côté interface.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// Surveillance du dossier des modèles.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    /// **Re-scan périodique**, en secondes.
    ///
    /// - absent (`None`) : automatique — voir [`watch_poll_recommendation`] ;
    /// - `0` : désactivé ;
    /// - `n` : re-scan toutes les `n` secondes.
    ///
    /// `None` et `0` ne veulent donc pas dire la même chose : « je ne me
    /// prononce pas » d'un côté, « surtout pas » de l'autre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_seconds: Option<u64>,
}

impl Watch {
    /// `true` si rien n'est fixé : `save` laisse alors le YAML sans ce bloc.
    fn is_auto(&self) -> bool {
        self.poll_seconds.is_none()
    }
}

/// Valeur renvoyée **à la place** de la clé d'API dans les réponses HTTP.
///
/// Elle dit au navigateur « une clé est enregistrée » sans la révéler. En sens
/// inverse, une clé reçue sous cette forme est comprise comme « ne touche pas à
/// celle que tu as » : c'est ce que renvoie le formulaire d'administration quand
/// l'utilisateur ne la retape pas.
pub const KEY_PLACEHOLDER: &str = "***";

/// Recherche assistée par un modèle de langage.
///
/// Absente (tous les champs vides), la recherche IA est **indisponible** : le
/// bouton de la barre de recherche reste grisé. Rien n'est écrit dans le YAML
/// tant que l'utilisateur n'a rien configuré.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ai {
    /// Fournisseur : `openai`, `anthropic`, `ollama` (voir [`crate::ai::PROVIDERS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// URL de base de l'API. Vide = celle du fournisseur.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Modèle à interroger. Vide = celui du fournisseur.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Clé d'API — un **secret** : jamais renvoyée telle quelle (voir
    /// [`Ai::redacted`]), et jamais journalisée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Modèles que le fournisseur a annoncés la dernière fois qu'on l'a
    /// interrogé.
    ///
    /// Conservés pour éviter de retrouver une liste vide (un seul choix, donc)
    /// à chaque visite de l'administration : on choisit alors un autre modèle
    /// sans avoir à réinterroger le fournisseur.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

/// Nombre maximal de modèles conservés (garde-fou d'écriture).
const MODELS_MAX: usize = 500;

impl Ai {
    /// `true` si un fournisseur est choisi : la recherche IA est utilisable.
    pub fn is_configured(&self) -> bool {
        self.provider
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
    }

    /// `true` si rien n'est fixé : `save` laisse alors le YAML sans ce bloc.
    fn is_empty(&self) -> bool {
        self.provider.is_none()
            && self.base_url.is_none()
            && self.model.is_none()
            && self.api_key.is_none()
            && self.models.is_empty()
    }

    /// Liste de modèles propre : espaces retirés, doublons et vides écartés,
    /// longueur bornée.
    ///
    /// Elle vient du formulaire : autant ne pas écrire dans `config.yml` ce
    /// qu'on refuserait d'afficher.
    pub fn normalized_models(incoming: &[String]) -> Vec<String> {
        let mut clean: Vec<String> = Vec::new();
        for model in incoming {
            let name = model.trim();
            if !name.is_empty() && !clean.iter().any(|known| known == name) {
                clean.push(name.to_string());
            }
            if clean.len() == MODELS_MAX {
                break;
            }
        }
        clean
    }

    /// Copie où la clé est remplacée par [`KEY_PLACEHOLDER`] — ce que l'API peut
    /// montrer. La configuration reste la même pour tout le reste.
    pub fn redacted(&self) -> Self {
        Self {
            api_key: self.api_key.as_ref().map(|_| KEY_PLACEHOLDER.to_string()),
            ..self.clone()
        }
    }

    /// Clé effective après réception d'un formulaire :
    ///
    /// - absente ou placeholder → on garde celle déjà enregistrée ;
    /// - chaîne vide → l'utilisateur l'a effacée ;
    /// - autre → nouvelle clé.
    pub fn merge_key(&self, incoming: Option<&str>) -> Option<String> {
        match incoming.map(str::trim) {
            None | Some(KEY_PLACEHOLDER) => self.api_key.clone(),
            Some("") => None,
            Some(value) => Some(value.to_string()),
        }
    }
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
    /// Surveillance du dossier des modèles.
    #[serde(default, skip_serializing_if = "Watch::is_auto")]
    pub watch: Watch,
    /// Recherche assistée par un modèle de langage.
    #[serde(default, skip_serializing_if = "Ai::is_empty")]
    pub ai: Ai,
}

impl Config {
    /// Copie destinée à sortir du backend (HTTP, WebSocket) : la clé d'API y est
    /// masquée.
    pub fn redacted(&self) -> Self {
        Self {
            ai: self.ai.redacted(),
            ..self.clone()
        }
    }
}

impl Config {
    /// Chemin du fichier de configuration.
    pub fn config_path() -> PathBuf {
        std::env::var("EASY3D_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("config.yml"))
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
                eprintln!(
                    "⚠️  Configuration invalide dans {}: {e}. Défauts utilisés.",
                    path.display()
                );
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// `true` si l'on doit afficher les modèles en image statique.
    pub fn is_image_mode(&self) -> bool {
        self.display.mode == DisplayMode::Image
    }

    /// Écrit la configuration dans `path`.
    ///
    /// Le YAML est régénéré depuis la structure : les commentaires du fichier
    /// d'origine ne survivent pas, d'où l'en-tête explicite. Appelé par
    /// `PUT /config` et par les outils MCP de configuration.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body = serde_yaml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let yaml = format!(
            "# easy3d — configuration de l'application\n\
             #\n\
             # Fichier réécrit par l'interface d'administration (PUT /config) :\n\
             # les commentaires d'origine sont remplacés par cet en-tête.\n\
             # Relu à chaud par le serveur, qui surveille ce fichier.\n\n{body}"
        );
        std::fs::write(path, yaml)
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
        if let Some(p) = self
            .models_root
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let path = Path::new(p);
            return if path.is_absolute() {
                path.to_path_buf()
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
            };
        }

        std::env::var("MODELS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../models"))
    }
}

/// Intervalle recommandé quand le système de fichiers ne peut pas remonter
/// d'événements fiables.
pub const RECOMMENDED_POLL_SECONDS: u64 = 5;

/// `true` si ce type de système de fichiers cache les événements du noyau.
///
/// Deux familles sont concernées : les montages **virtualisés** (partages de
/// fichiers Docker Desktop : `virtiofs`, `grpcfuse`…) et les montages **réseau**
/// (`nfs`, `cifs`…). Dans les deux cas le noyau n'assiste pas aux écritures —
/// celles qui viennent de l'explorateur de l'hôte, surtout — donc `inotify` n'a
/// rien à signaler, ni à la racine ni dans les sous-dossiers.
fn is_event_poor(fstype: &str) -> bool {
    fstype.starts_with("fuse.")
        || matches!(
            fstype,
            // Virtualisés : Docker Desktop (9p/drvfs), WSL (drvfs), Colima…
            "virtiofs" | "9p" | "drvfs" |
                // Réseau.
                "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs"
        )
}

/// Type du système de fichiers portant `path`, d'après un contenu de
/// `/proc/mounts` (voir [`filesystem_type`]).
///
/// On retient le point de montage **le plus précis** qui contient le chemin, en
/// comparant des **composants** de chemin : `/modelsX` n'est pas dans `/models`.
fn filesystem_type_in(mounts: &str, path: &Path) -> Option<String> {
    let mut best: Option<(usize, String)> = None;

    for line in mounts.lines() {
        // « périphérique point-de-montage type options 0 0 »
        let mut fields = line.split_whitespace();
        let (Some(_device), Some(mount_point), Some(fstype)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };

        let mount_point = PathBuf::from(unescape_mount(mount_point));
        if !path.starts_with(&mount_point) {
            continue;
        }

        let depth = mount_point.components().count();
        if best.as_ref().is_none_or(|(known, _)| depth > *known) {
            best = Some((depth, fstype.to_string()));
        }
    }

    best.map(|(_, fstype)| fstype)
}

/// Dé-échappe un champ de `/proc/mounts` (les espaces y sont écrits `\040`).
fn unescape_mount(field: &str) -> String {
    field
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
}

/// Type du système de fichiers portant `path`.
///
/// `None` hors Linux (Windows, macOS) : le backend y tourne directement sur le
/// disque, sans montage virtuel — la question ne se pose pas.
fn filesystem_type(path: &Path) -> Option<String> {
    filesystem_type_in(&std::fs::read_to_string("/proc/mounts").ok()?, path)
}

/// Intervalle **recommandé** pour cette installation, et le type de système de
/// fichiers qui le justifie (pour l'expliquer dans l'interface).
///
/// `0` quand les événements suffisent (disque local),
/// [`RECOMMENDED_POLL_SECONDS`] quand le dossier passe par un montage virtualisé
/// ou réseau. Détecter le système de fichiers, et non l'OS, est indispensable :
/// dans un conteneur Docker Desktop le backend tourne sous **Linux** alors que le
/// dossier vient de Windows.
pub fn watch_poll_recommendation(root: &Path) -> (u64, Option<String>) {
    let fstype = filesystem_type(root);
    let event_poor = fstype.as_deref().is_some_and(is_event_poor);

    (
        if event_poor {
            RECOMMENDED_POLL_SECONDS
        } else {
            0
        },
        fstype,
    )
}

/// Intervalle à appliquer : la valeur choisie, sinon la recommandation.
pub fn effective_interval(configured: Option<u64>, recommended: u64) -> Option<Duration> {
    let seconds = configured.unwrap_or(recommended);
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

/// Intervalle du re-scan périodique tel que le watcher doit l'appliquer
/// (`None` = aucun re-scan : les événements suffisent).
///
/// Vit ici, et non dans `main.rs`, pour être testable : `main.rs` est un binaire,
/// aucune de ses fonctions n'est atteignable par un test.
pub fn watch_poll_interval(config: &Config, root: &Path) -> Option<Duration> {
    let (recommended, _) = watch_poll_recommendation(root);
    effective_interval(config.watch.poll_seconds, recommended)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_le_fichier_de_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yml");
        std::fs::write(
            &file,
            "models_root: ../models\ndisplay:\n  mode: \"image\"\n",
        )
        .unwrap();

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
        assert_eq!(
            serde_json::to_string(&DisplayMode::ThreeD).unwrap(),
            "\"3d\""
        );
        assert_eq!(
            serde_json::to_string(&DisplayMode::Image).unwrap(),
            "\"image\""
        );
    }

    /// La liste des modèles fait l'aller-retour par `config.yml` : c'est ce qui
    /// évite de réinterroger le fournisseur à chaque visite de l'administration.
    #[test]
    fn la_liste_des_modeles_est_conservee() {
        let ai = Ai {
            provider: Some("openai".to_string()),
            models: vec!["gpt-4o".to_string(), "o3-mini".to_string()],
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&ai).unwrap();
        assert!(yaml.contains("models:"), "{yaml}");
        let relu: Ai = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(relu, ai);

        // Sans liste, le bloc reste aussi court qu'avant (et `is_empty` garde
        // tout le bloc hors du YAML quand rien n'est configuré).
        let vide = Ai {
            provider: Some("ollama".to_string()),
            ..Default::default()
        };
        assert!(!serde_yaml::to_string(&vide).unwrap().contains("models"));
        // Et une configuration sans IA n'écrit pas de bloc du tout.
        let yaml = serde_yaml::to_string(&Config::default()).unwrap();
        assert!(!yaml.contains("ai:"), "{yaml}");
    }

    #[test]
    fn la_liste_des_modeles_est_nettoyee() {
        let brut = [
            " gpt-4o ".to_string(),
            "gpt-4o".to_string(),
            "".to_string(),
            "   ".to_string(),
            "o3-mini".to_string(),
        ];
        assert_eq!(Ai::normalized_models(&brut), vec!["gpt-4o", "o3-mini"]);

        // Bornée : une liste absurde ne part pas dans le fichier.
        let enorme: Vec<String> = (0..600).map(|i| format!("modele-{i}")).collect();
        assert_eq!(Ai::normalized_models(&enorme).len(), 500);
    }

    #[test]
    fn re_scan_automatique_ou_choisi_a_la_main() {
        // Absent → recommandation de l'installation (« je ne me prononce pas »).
        assert_eq!(effective_interval(None, 5), Some(Duration::from_secs(5)));
        assert_eq!(effective_interval(None, 0), None);
        // Désactivé explicitement : la recommandation ne s'applique pas.
        assert_eq!(effective_interval(Some(0), 5), None);
        // Valeur choisie dans l'interface.
        assert_eq!(
            effective_interval(Some(15), 5),
            Some(Duration::from_secs(15))
        );
    }

    #[test]
    fn les_systemes_de_fichiers_virtualises_sont_reconnus() {
        for poor in [
            "virtiofs",
            "9p",
            "fuse.grpcfuse",
            "fuse.sshfs",
            "nfs4",
            "cifs",
        ] {
            assert!(is_event_poor(poor), "{poor} devrait recommander un re-scan");
        }
        for fine in ["ext4", "xfs", "btrfs", "ntfs", "apfs", "overlay", "tmpfs"] {
            assert!(!is_event_poor(fine), "{fine} remonte ses événements");
        }
    }

    #[test]
    fn le_point_de_montage_le_plus_precis_gagne() {
        let mounts = "\
/dev/sda1 / ext4 rw,relatime 0 0
\
models /models virtiofs rw,relatime 0 0
\
/dev/sdb1 /models/partage ext4 rw,relatime 0 0
";

        // Le montage virtuel du dossier : le cas des partages Docker Desktop.
        assert_eq!(
            filesystem_type_in(mounts, Path::new("/models/Maison")).as_deref(),
            Some("virtiofs")
        );
        // Un montage plus profond à l'intérieur gagne.
        assert_eq!(
            filesystem_type_in(mounts, Path::new("/models/partage/x.stl")).as_deref(),
            Some("ext4")
        );
        // Ailleurs : la racine.
        assert_eq!(
            filesystem_type_in(mounts, Path::new("/home/moi")).as_deref(),
            Some("ext4")
        );
        // `/modelsX` n'est pas dans `/models` (comparaison par composants).
        assert_eq!(
            filesystem_type_in(mounts, Path::new("/modelsX/y.stl")).as_deref(),
            Some("ext4")
        );
        // Point de montage contenant un espace (`\040` dans /proc/mounts).
        let espaces = "/dev/sdc1 /media/disque\\040externe vfat rw 0 0\n";
        assert_eq!(
            filesystem_type_in(espaces, Path::new("/media/disque externe/x")).as_deref(),
            Some("vfat")
        );

        assert_eq!(filesystem_type_in("", Path::new("/models")), None);
    }

    #[test]
    fn le_re_scan_se_lit_dans_le_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.yml");
        std::fs::write(&file, "watch:\n  poll_seconds: 15\n").unwrap();
        assert_eq!(Config::load_from(&file).watch.poll_seconds, Some(15));

        // Absent : automatique, et le YAML réécrit reste sans ce bloc.
        let cfg = Config::default();
        assert_eq!(cfg.watch.poll_seconds, None);
        assert!(!serde_yaml::to_string(&cfg).unwrap().contains("watch"));
    }
}
