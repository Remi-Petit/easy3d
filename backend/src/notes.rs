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

/// Chemin de l'**état CRDT** d'une note (`<rel>.ydoc`).
///
/// Le Markdown reste la projection lisible ; ce fichier binaire contient
/// l'histoire Yjs du document, et c'est lui qui permet au serveur de repartir
/// sur le **même** document après un redémarrage. Sans lui, il ré-amorcerait un
/// document neuf : ses insertions et celles que les clients détiennent encore
/// se cumuleraient, et la note apparaîtrait en double.
pub fn state_path(root: &Path, rel: &str) -> Option<PathBuf> {
    note_path(root, rel).map(|path| path.with_extension("ydoc"))
}

/// Lit l'état CRDT d'une note, s'il existe.
pub fn read_state(root: &Path, rel: &str) -> Option<Vec<u8>> {
    fs::read(state_path(root, rel)?).ok()
}

/// Écrit l'état CRDT d'une note.
pub fn write_state(root: &Path, rel: &str, bytes: &[u8]) -> std::io::Result<()> {
    let path = state_path(root, rel).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "chemin de note invalide")
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

/// Supprime l'état CRDT d'une note (note vidée).
///
/// Indispensable : sans suppression, un redémarrage ressusciterait le texte
/// depuis ce document alors que le Markdown a disparu.
pub fn remove_state(root: &Path, rel: &str) {
    if let Some(path) = state_path(root, rel) {
        let _ = fs::remove_file(path);
    }
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

/// Chemin relatif (séparateurs `/`) de `path` par rapport à `root`.
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Fait suivre la note d'un élément qui vient d'être **déplacé ou renommé**.
///
/// `from` et `to` sont des chemins absolus. La note de l'élément est déplacée,
/// ainsi que **tout le sous-arbre de notes** s'il s'agit d'un dossier.
///
/// Sans effet (et sans erreur) si l'un des deux chemins est hors de la racine
/// des modèles : un fichier qui sort du catalogue laisse sa note en place, pour
/// la retrouver s'il y revient.
///
/// Retourne `true` si au moins un fichier de note a été déplacé.
pub fn move_for_path(root: &Path, from: &Path, to: &Path) -> std::io::Result<bool> {
    let (Some(from_rel), Some(to_rel)) = (rel_of(root, from), rel_of(root, to)) else {
        return Ok(false);
    };
    if from_rel == to_rel {
        return Ok(false);
    }

    let notes_root = root.join(NOTES_DIR);
    let mut moved = false;

    // 1. La note de l'élément lui-même (`.easy3d-notes/<rel>.md`) et son état
    //    CRDT (`.easy3d-notes/<rel>.ydoc`).
    for (old, new) in [
        (note_path(root, &from_rel), note_path(root, &to_rel)),
        (state_path(root, &from_rel), state_path(root, &to_rel)),
    ] {
        let (Some(old), Some(new)) = (old, new) else {
            continue;
        };
        // On n'écrase jamais un fichier déjà en place à la destination.
        if old.is_file() && !new.exists() {
            if let Some(parent) = new.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&old, &new)?;
            moved = true;
        }
    }

    // 2. Le sous-arbre de notes, si c'est un dossier qui a été déplacé.
    let old_dir = notes_root.join(&from_rel);
    if old_dir.is_dir() {
        let new_dir = notes_root.join(&to_rel);
        if let Some(parent) = new_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        if !new_dir.exists() {
            fs::rename(&old_dir, &new_dir)?;
            moved = true;
        }
    }

    Ok(moved)
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

    /// Prépare une arborescence `models/` minimale (dossiers A et B).
    fn models_tree() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join("A")).unwrap();
        fs::create_dir_all(root.join("B")).unwrap();
        (dir, root)
    }

    #[test]
    fn la_note_suit_un_fichier_deplace_vers_un_autre_dossier() {
        let (_dir, root) = models_tree();
        fs::write(root.join("A/x.stl"), "x").unwrap();
        write(&root, "A/x.stl", "# note de x").unwrap();

        let moved = move_for_path(&root, &root.join("A/x.stl"), &root.join("B/x.stl")).unwrap();

        assert!(moved);
        assert!(read(&root, "A/x.stl").is_none());
        assert_eq!(read(&root, "B/x.stl").as_deref(), Some("# note de x"));
    }

    #[test]
    fn la_note_suit_un_fichier_renomme() {
        let (_dir, root) = models_tree();
        fs::write(root.join("A/x.stl"), "x").unwrap();
        write(&root, "A/x.stl", "# note").unwrap();

        move_for_path(&root, &root.join("A/x.stl"), &root.join("A/y.stl")).unwrap();

        assert!(read(&root, "A/x.stl").is_none());
        assert_eq!(read(&root, "A/y.stl").as_deref(), Some("# note"));
    }

    #[test]
    fn un_dossier_deplace_emmene_toutes_ses_notes() {
        let (_dir, root) = models_tree();
        fs::create_dir_all(root.join("A/sous")).unwrap();
        fs::write(root.join("A/x.stl"), "x").unwrap();
        fs::write(root.join("A/sous/y.stl"), "y").unwrap();
        write(&root, "A", "# note du dossier").unwrap();
        write(&root, "A/x.stl", "# note de x").unwrap();
        write(&root, "A/sous/y.stl", "# note de y").unwrap();

        // A → B (le dossier entier change de nom).
        move_for_path(&root, &root.join("A"), &root.join("B")).unwrap();

        assert_eq!(read(&root, "B").as_deref(), Some("# note du dossier"));
        assert_eq!(read(&root, "B/x.stl").as_deref(), Some("# note de x"));
        assert_eq!(read(&root, "B/sous/y.stl").as_deref(), Some("# note de y"));
        // Plus rien à l'ancien emplacement.
        assert!(read(&root, "A").is_none());
        assert!(!root.join(".easy3d-notes/A").exists());
    }

    #[test]
    fn deplacement_sans_note_ne_cree_rien() {
        let (_dir, root) = models_tree();
        fs::write(root.join("A/x.stl"), "x").unwrap();

        assert!(!move_for_path(&root, &root.join("A/x.stl"), &root.join("B/x.stl")).unwrap());
        assert!(!root.join(".easy3d-notes").exists());
    }

    #[test]
    fn un_deplacement_hors_du_catalogue_laisse_la_note_en_place() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("models");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("x.stl"), "x").unwrap();
        write(&root, "x.stl", "# note").unwrap();

        // Sortie du catalogue : rien n'est déplacé, la note reste disponible.
        let outside = dir.path().join("ailleur.stl");
        assert!(!move_for_path(&root, &root.join("x.stl"), &outside).unwrap());
        assert_eq!(read(&root, "x.stl").as_deref(), Some("# note"));

        // Entrée dans le catalogue : la note existante n'est pas écrasée.
        assert!(!move_for_path(&root, &outside, &root.join("y.stl")).unwrap());
        assert!(read(&root, "y.stl").is_none());
    }

    #[test]
    fn une_note_deja_presente_a_la_destination_n_est_pas_ecrasee() {
        let (_dir, root) = models_tree();
        fs::write(root.join("A/x.stl"), "x").unwrap();
        write(&root, "A/x.stl", "# note source").unwrap();
        write(&root, "B/x.stl", "# note destination").unwrap();

        move_for_path(&root, &root.join("A/x.stl"), &root.join("B/x.stl")).unwrap();

        // La cible est conservée, la source reste où elle est : aucune perte.
        assert_eq!(read(&root, "B/x.stl").as_deref(), Some("# note destination"));
        assert_eq!(read(&root, "A/x.stl").as_deref(), Some("# note source"));
    }

    #[test]
    fn un_chemin_identique_ne_fait_rien() {
        let (_dir, root) = models_tree();
        fs::write(root.join("A/x.stl"), "x").unwrap();
        write(&root, "A/x.stl", "# note").unwrap();

        assert!(!move_for_path(&root, &root.join("A/x.stl"), &root.join("A/x.stl")).unwrap());
        assert_eq!(read(&root, "A/x.stl").as_deref(), Some("# note"));
    }
}
