//! Notes explicatives (Markdown) associées aux dossiers et aux fichiers.
//!
//! Une note est stockée dans un dossier caché `.easy3d-notes/`, en miroir de
//! l'arborescence des modèles — comme les aperçus dans `.easy3d-thumbs/` :
//!
//! ```text
//! models/DemaAuto/boitier.stl → models/.easy3d-notes/DemaAuto/boitier.stl.md
//! models/DemaAuto             → models/.easy3d-notes/DemaAuto.md
//! ```
//!
//! Le dossier étant caché, le scanner (et donc le frontend) ne le voit pas.

use std::fs;
use std::path::{Path, PathBuf};

/// Nom du dossier caché (dans `models/`) qui regroupe les notes.
pub const NOTES_DIR: &str = ".easy3d-notes";

/// Chemin absolu de la note d'un élément, ou `None` si `rel` est invalide.
///
/// `rel` est le chemin relatif d'un fichier (`DemaAuto/boitier.stl`) ou le nom
/// d'un dossier (`DemaAuto`). Tout ce qui permettrait de sortir du dossier des
/// notes est refusé : chemin absolu, segment vide, `.`/`..`, séparateur
/// Windows, `:` de flux alternatif NTFS.
pub fn note_path(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() || rel.contains('\\') || rel.contains(':') {
        return None;
    }
    if rel.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
        return None;
    }

    Some(root.join(NOTES_DIR).join(format!("{rel}.md")))
}

/// Lit la note d'un élément. `None` si elle n'existe pas (encore).
pub fn read(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(note_path(root, rel)?).ok()
}

/// Écrit la note d'un élément.
///
/// Un contenu vide (ou blanc) **supprime** le fichier, pour ne pas laisser
/// traîner des `.md` vides dans le dossier caché.
pub fn write(root: &Path, rel: &str, content: &str) -> std::io::Result<()> {
    let path = note_path(root, rel).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "chemin de note invalide")
    })?;

    if content.trim().is_empty() {
        if path.is_file() {
            fs::remove_file(&path)?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemin_miroir_de_l_arborescence() {
        let root = Path::new("/models");
        assert_eq!(
            note_path(root, "DemaAuto/boitier.stl").unwrap(),
            root.join(".easy3d-notes/DemaAuto/boitier.stl.md")
        );
        assert_eq!(
            note_path(root, "DemaAuto").unwrap(),
            root.join(".easy3d-notes/DemaAuto.md")
        );
    }

    #[test]
    fn refuse_les_chemins_dangereux() {
        let root = Path::new("/models");
        for rel in [
            "",
            "   ",
            "..",
            "../secret",
            "DemaAuto/../../secret",
            "/etc/passwd",
            "C:/Windows/system32",
            "DemaAuto\\evil",
            "file.txt:stream",
        ] {
            assert!(note_path(root, rel).is_none(), "accepté à tort : {rel:?}");
        }
    }

    #[test]
    fn ecrit_lit_et_supprime_une_note() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        assert!(read(root, "DemaAuto/x.stl").is_none());

        write(root, "DemaAuto/x.stl", "# Titre\n\ndu **texte**").unwrap();
        assert_eq!(read(root, "DemaAuto/x.stl").unwrap(), "# Titre\n\ndu **texte**");

        // Une note vidée disparaît du disque.
        write(root, "DemaAuto/x.stl", "   \n").unwrap();
        assert!(read(root, "DemaAuto/x.stl").is_none());
        assert!(!root.join(".easy3d-notes/DemaAuto/x.stl.md").exists());
    }
}
