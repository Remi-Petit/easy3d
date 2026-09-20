//! Adresses connues des fournisseurs, dans un fichier YAML **à part**.
//!
//! Le protocole d'un fournisseur (URL des appels, en-têtes d'authentification)
//! vit en Rust — c'est du code. La **liste des services** qui parlent ce
//! protocole n'en est pas : c'est de la donnée, qui change plus souvent que le
//! binaire (un service de plus, une adresse intermédiaire, une passerelle
//! d'entreprise). Elle vit donc dans `ai-presets.yml`, relu à chaud.
//!
//! Le fichier du dépôt est **embarqué** dans le binaire ([`SHIPPED`]) : c'est
//! lui qui est écrit dans le dossier de configuration quand il n'y en a pas
//! encore (premier démarrage d'un conteneur, dossier jetable). Il n'y a donc
//! qu'**une** liste à tenir à jour, et un service ajouté au fichier part avec
//! les binaires publiés.
//!
//! Un fichier illisible (le temps d'une écriture, ou une faute de frappe) ne
//! doit **jamais** vider les puces de l'interface : la relecture à chaud
//! conserve les adresses précédentes et se contente de le signaler (voir
//! [`Reloader`]).

use super::Preset;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Table des adresses connues : identifiant de fournisseur → adresses.
///
/// C'est la forme du fichier, et celle servie à l'interface.
pub type Presets = BTreeMap<String, Vec<Preset>>;

/// Empreinte d'un fichier : date de modification et taille. De quoi savoir qu'il
/// a bougé sans le relire ni comparer son contenu.
type Stamp = (SystemTime, u64);

/// Nom du fichier, posé à côté de `config.yml`.
pub const FILE_NAME: &str = "ai-presets.yml";

/// Le fichier livré avec le binaire (celui du dépôt, embarqué à la compilation).
const SHIPPED: &str = include_str!("../../ai-presets.yml");

/// Chemin du fichier des adresses.
///
/// `EASY3D_PRESETS` s'il est défini, sinon **à côté de la configuration** : les
/// deux fichiers se règlent ensemble, et le dossier de configuration est déjà
/// celui qu'on monte et qu'on surveille.
pub fn path() -> PathBuf {
    std::env::var("EASY3D_PRESETS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::config::Config::config_path().with_file_name(FILE_NAME))
}

/// Adresses livrées avec le binaire.
///
/// Sert quand il n'y a pas de fichier à lire (premier démarrage, fichier
/// supprimé). Le texte embarqué est fixé à la compilation : un test unitaire
/// vérifie qu'il se lit et couvre tous les fournisseurs, donc l'échec de
/// [`parse`] ici serait un défaut de développement, pas une surprise de
/// l'utilisateur.
pub fn defaults() -> Presets {
    parse(SHIPPED).unwrap_or_else(|e| {
        eprintln!("⚠️  Adresses livrées illisibles ({e}) : aucune adresse proposée.");
        Presets::new()
    })
}

/// Lit un fichier d'adresses.
///
/// Le message d'erreur est destiné à l'utilisateur : il nomme le fichier et
/// cite l'erreur de YAML telle quelle (elle donne le numéro de ligne).
pub fn load(path: &Path) -> Result<Presets, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("{} illisible : {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{} invalide : {e}", path.display()))
}

/// Lit un texte YAML d'adresses.
pub fn parse(text: &str) -> Result<Presets, String> {
    serde_yaml::from_str::<Presets>(text).map_err(|e| e.to_string())
}

/// Écrit le fichier s'il **n'existe pas**, à partir des adresses livrées.
///
/// Retourne `true` s'il a été créé. Le fichier existant n'est jamais réécrit :
/// c'est un fichier que l'on édite à la main, et ses commentaires — le mode
/// d'emploi, en tête — disparaîtraient.
pub fn ensure_file(path: &Path) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, SHIPPED)?;
    Ok(true)
}

/// Identifiants présents dans le fichier mais **inconnus** du binaire.
///
/// Une faute de frappe dans une clé (`openAi:`) ne produit ni erreur ni puce :
/// sans ce contrôle, l'utilisateur chercherait longtemps pourquoi son adresse
/// n'apparaît pas. À signaler au démarrage et à chaque relecture.
pub fn unknown(presets: &Presets) -> Vec<String> {
    presets
        .keys()
        .filter(|id| super::provider_for(id).is_none())
        .cloned()
        .collect()
}

/// Identifiants de fournisseurs connus, pour un message d'erreur utile.
pub fn known_ids() -> Vec<&'static str> {
    super::PROVIDERS.iter().map(|p| p.id()).collect()
}

/// Empreinte du fichier, `None` s'il n'existe pas (encore).
fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Ce qu'a donné un tour de relecture.
#[derive(Debug)]
pub enum Reload {
    /// Le fichier n'a pas bougé.
    Unchanged,
    /// Les adresses ont changé (et sont là).
    Changed(Presets),
    /// Le fichier a bougé mais ne se lit pas : message à journaliser. Les
    /// adresses en cours restent en place.
    Invalid(String),
}

/// Suit le fichier des adresses et le relit quand il change.
///
/// Sert deux sources d'événements, parce que les deux existent : le watcher
/// (une modification signalée par le noyau) et le re-scan périodique (les
/// montages virtualisés — Docker Desktop — ne signalent **rien**, y compris à
/// la racine du montage ; voir `config::watch_poll_recommendation`).
///
/// L'empreinte évite de relire — et donc de se plaindre — à chaque tour quand
/// rien n'a bougé. Un fichier **supprimé** ramène aux adresses livrées, ce que
/// le mode d'emploi du fichier annonce.
pub struct Reloader {
    path: PathBuf,
    seen: Option<Stamp>,
}

impl Reloader {
    /// Suit `path`, à partir de l'état qu'on vient de lire.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let seen = stamp(&path);
        Self { path, seen }
    }

    /// Relit le fichier s'il a bougé.
    pub fn poll(&mut self) -> Reload {
        let now = stamp(&self.path);
        if now == self.seen {
            return Reload::Unchanged;
        }
        self.seen = now;

        // Fichier absent : retour aux adresses livrées, comme annoncé en tête
        // du fichier.
        if now.is_none() {
            return Reload::Changed(defaults());
        }

        match load(&self.path) {
            Ok(presets) => Reload::Changed(presets),
            Err(e) => Reload::Invalid(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Le fichier embarqué est le mode d'emploi du binaire : il doit se lire,
    /// couvrir **chaque** fournisseur connu, et proposer des adresses utilisables.
    #[test]
    fn les_adresses_livrees_couvrent_tous_les_fournisseurs() {
        let shipped = parse(SHIPPED).expect("le fichier embarqué se lit");
        assert!(unknown(&shipped).is_empty());

        for provider in super::super::PROVIDERS {
            let list = shipped
                .get(provider.id())
                .unwrap_or_else(|| panic!("{} n'a aucune adresse livrée", provider.id()));
            assert!(!list.is_empty(), "{} : liste vide", provider.id());

            for preset in list {
                assert!(!preset.label.is_empty());
                assert!(
                    preset.base_url.starts_with("http"),
                    "adresse incomplète : {}",
                    preset.base_url
                );
            }

            let urls: Vec<&str> = list.iter().map(|p| p.base_url.as_str()).collect();
            let mut unique = urls.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(urls.len(), unique.len(), "adresses en double : {urls:?}");

            // Le champ laissé vide vaut l'adresse par défaut du fournisseur :
            // la première entrée doit donc être la sienne, sinon la puce qui
            // s'allume au chargement serait celle d'un autre service.
            assert_eq!(list[0].base_url, provider.default_base_url());
            assert_eq!(list[0].model, provider.default_model());
        }
    }

    /// Les services qui imitent l'API d'OpenAI sont livrés avec lui : c'est tout
    /// l'intérêt de la liste, et ce que l'utilisateur vient chercher.
    #[test]
    fn openai_livre_les_services_qui_l_imitent() {
        let labels: Vec<String> = defaults()["openai"]
            .iter()
            .map(|p| p.label.clone())
            .collect();

        for expected in ["OpenAI", "DeepSeek", "OpenRouter"] {
            assert!(
                labels.contains(&expected.to_string()),
                "absent de {labels:?}"
            );
        }
    }

    #[test]
    fn le_fichier_est_cree_puis_relu_tel_quel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);

        assert!(ensure_file(&path).unwrap());
        assert_eq!(load(&path).unwrap(), defaults());

        // Jamais réécrit : c'est un fichier que l'on édite à la main.
        std::fs::write(
            &path,
            "openai:\n  - label: Passerelle\n    base_url: https://gw/v1\n",
        )
        .unwrap();
        assert!(!ensure_file(&path).unwrap());
        let custom = load(&path).unwrap();
        assert_eq!(custom["openai"][0].label, "Passerelle");
        // Un fournisseur absent du fichier n'a simplement aucune adresse.
        assert!(!custom.contains_key("anthropic"));
    }

    #[test]
    fn un_fichier_invalide_est_signale_et_une_cle_inconnue_aussi() {
        let dir = tempfile::tempdir().unwrap();

        // Clé inconnue : le fichier se lit, mais l'entrée ne servira personne.
        let path = write(
            dir.path(),
            "openAi:\n  - label: OpenAI\n    base_url: https://api.openai.com/v1\n",
        );
        let presets = load(&path).unwrap();
        assert_eq!(unknown(&presets), vec!["openAi".to_string()]);

        // Adresse manquante : c'est une erreur, et le message cite le fichier.
        let path = write(dir.path(), "openai:\n  - label: OpenAI\n");
        let err = load(&path).unwrap_err();
        assert!(err.contains(FILE_NAME), "{err}");

        // YAML cassé : message également exploitable.
        let path = write(dir.path(), "openai: [");
        assert!(load(&path).is_err());
    }

    /// La relecture à chaud suit l'empreinte du fichier : rien à faire tant
    /// qu'il n'a pas bougé, et une erreur ne doit pas effacer les adresses.
    #[test]
    fn la_relecture_suit_le_fichier() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "ollama:\n  - label: Poste\n    base_url: http://localhost:11434/v1\n",
        );

        let mut reloader = Reloader::new(&path);
        assert!(matches!(reloader.poll(), Reload::Unchanged));

        // Même contenu, nouvelle date : c'est un changement (on ne compare pas
        // les textes), et le résultat est celui du fichier.
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap()).unwrap();
        match reloader.poll() {
            Reload::Changed(presets) => assert_eq!(presets["ollama"][0].label, "Poste"),
            other => panic!("attendu un rechargement, obtenu {other:?}"),
        }

        // Faute de frappe : signalée, mais les adresses en cours restent.
        std::fs::write(&path, "ollama: [").unwrap();
        match reloader.poll() {
            Reload::Invalid(message) => assert!(message.contains(FILE_NAME), "{message}"),
            other => panic!("attendu une erreur, obtenu {other:?}"),
        }
        assert!(matches!(reloader.poll(), Reload::Unchanged));

        // Fichier supprimé : retour aux adresses livrées (voir le mode d'emploi
        // écrit en tête du fichier).
        std::fs::remove_file(&path).unwrap();
        match reloader.poll() {
            Reload::Changed(presets) => assert_eq!(presets, defaults()),
            other => panic!("attendu les adresses livrées, obtenu {other:?}"),
        }
    }
}
