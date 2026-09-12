//! Formats de fichiers reconnus par easy3d.
//!
//! Un format = **un fichier** dans ce dossier, qui implémente [`Format`] :
//!
//! | Fichier       | Format                              |
//! |---------------|-------------------------------------|
//! | `stl.rs`      | STL (binaire et ASCII)              |
//! | `obj.rs`      | Wavefront OBJ                       |
//! | `three_mf.rs` | 3MF (archive ZIP + modèle XML)      |
//! | `gcode.rs`    | G-code (`.gcode`, `.gco`)           |
//! | `image.rs`    | Images d'aperçu « sœur » (PNG, JPEG…) |
//!
//! Les formats maillés (STL, OBJ, 3MF) partagent le rasteriseur de [`mesh`].
//!
//! # Ajouter un format
//!
//! 1. créer `formats/mon_format.rs` avec sa structure et son `impl Format` ;
//! 2. l'enregistrer dans [`REGISTRY`] (une ligne).
//!
//! Rien d'autre : le scan ([`crate::scanner`]), la génération d'aperçus
//! ([`crate::thumbnail`]) et le type MIME servi par `GET /file`
//! ([`crate::api`]) passent tous par ce registre.

mod gcode;
mod image;
mod mesh;
mod obj;
mod stl;
mod three_mf;

pub use image::{IMAGE_EXTENSIONS, is_image, priority as image_priority};

use std::path::Path;

/// Dimensions (pixels) des aperçus produits par [`mesh`].
///
/// Sert aussi de taille maximale aux formats qui réutilisent une vignette
/// embarquée (G-code, 3MF) : au-delà, elle est réduite à cette échelle.
pub(crate) const THUMB_W: usize = 480;
pub(crate) const THUMB_H: usize = 366;

/// Un format de fichier reconnu par easy3d.
pub trait Format: Sync {
    /// Nom lisible (journalisation, messages).
    fn name(&self) -> &'static str;

    /// Extensions gérées, **minuscules et sans point** (ex : `["gcode", "gco"]`).
    fn extensions(&self) -> &'static [&'static str];

    /// Type MIME servi par `GET /file` pour ce format.
    fn content_type(&self) -> &'static str;

    /// `true` si ce format peut produire un aperçu PNG.
    fn has_preview(&self) -> bool {
        false
    }

    /// Écrit dans `out` l'aperçu PNG du fichier `path` (chemin **absolu**).
    ///
    /// Retourne `None` si le format n'a pas d'aperçu, si le fichier est
    /// illisible ou s'il n'embarque pas de vignette exploitable.
    fn thumbnail(&self, _path: &Path, _out: &Path) -> Option<()> {
        None
    }
}

/// Registre des formats connus — **seul endroit à éditer pour en ajouter un**.
///
/// L'ordre n'a pas d'importance : les extensions sont supposées disjointes.
static REGISTRY: &[&dyn Format] = &[&stl::STL, &obj::OBJ, &three_mf::THREE_MF, &gcode::GCODE];

/// Tous les formats connus, dans l'ordre du registre.
pub fn all() -> &'static [&'static dyn Format] {
    REGISTRY
}

/// Extension (minuscule, sans point) d'un nom de fichier ou d'un chemin.
pub fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Format qui gère ce fichier (d'après son extension), s'il est connu.
pub fn find(name: &str) -> Option<&'static dyn Format> {
    let ext = ext_of(name);
    if ext.is_empty() {
        return None;
    }
    REGISTRY
        .iter()
        .copied()
        .find(|f| f.extensions().contains(&ext.as_str()))
}

/// `true` si l'extension du fichier correspond à un format connu.
pub fn is_supported(name: &str) -> bool {
    find(name).is_some()
}

/// `true` si un aperçu PNG peut être généré pour ce fichier.
pub fn can_have_preview(name: &str) -> bool {
    find(name).is_some_and(|f| f.has_preview())
}

/// Génère l'aperçu PNG de `path` dans `out`, selon son format.
///
/// `None` si le format est inconnu, sans aperçu, ou si la génération échoue.
pub fn make_thumbnail(path: &Path, out: &Path) -> Option<()> {
    let name = path.file_name()?.to_string_lossy();
    find(&name)?.thumbnail(path, out)
}

/// Type MIME servi pour un fichier (modèles, G-code, images).
///
/// `application/octet-stream` pour tout le reste.
pub fn content_type(name: &str) -> &'static str {
    match find(name) {
        Some(format) => format.content_type(),
        None => image::content_type(&ext_of(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trouve_le_format_par_extension() {
        assert_eq!(find("a.stl").unwrap().name(), "STL");
        assert_eq!(find("DemaAuto/a.obj").unwrap().name(), "OBJ");
        // L'extension est comparée en minuscules.
        assert_eq!(find("piece.3MF").unwrap().name(), "3MF");
        assert_eq!(find("piece.gco").unwrap().name(), "G-code");
        // Inconnu ou absent de toute extension.
        assert!(find("notes.md").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn apercu_et_type_mime() {
        for rel in ["a.stl", "a.obj", "a.3mf", "a.gcode", "a.gco"] {
            assert!(can_have_preview(rel), "devrait accepter {rel}");
        }
        // Une image n'est pas un aperçu « générable » : elle est déjà une image.
        for rel in ["notes.md", "photo.png", "readme.txt", ""] {
            assert!(!can_have_preview(rel), "devrait refuser {rel}");
        }

        assert_eq!(content_type("a.stl"), "model/stl");
        assert_eq!(content_type("a.3mf"), "model/3mf");
        assert_eq!(content_type("a.gcode"), "text/plain");
        assert_eq!(content_type("photo.jpg"), "image/jpeg");
        assert_eq!(content_type("archive.zip"), "application/octet-stream");
    }
}
