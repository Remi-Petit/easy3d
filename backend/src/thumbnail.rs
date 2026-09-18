//! Aperçus PNG : cache et orchestration **côté backend**.
//!
//! Les formats eux-mêmes (STL, OBJ, 3MF, G-code…) vivent dans [`crate::formats`] :
//! ce module décide seulement *quand* générer un aperçu, *où* le ranger et
//! *quand* réutiliser celui déjà présent.
//!
//! Pour chaque fichier reconnu dans `models/`, on produit un PNG dans
//! `<models_root>/.easy3d-thumbs/`, en reproduisant l'arborescence
//! (`chainsaw-man/x.stl` → `.easy3d-thumbs/chainsaw-man/x.png`). Cette image sert
//! d'aperçu sur le site — plus léger qu'un rendu 3D navigateur pour les grandes
//! grilles.

use crate::formats;
use std::fs;
use std::path::{Path, PathBuf};

/// Nom du dossier caché (dans `models/`) qui regroupe les aperçus générés.
pub const THUMB_DIR: &str = ".easy3d-thumbs";

/// Chemin (relatif à la racine des modèles) de l'aperçu d'un fichier donné.
///
/// `models/chainsaw-man/Chainsaw_Man.stl` → `.easy3d-thumbs/chainsaw-man/Chainsaw_Man.png`
pub fn thumb_rel_path(model_rel: &str) -> String {
    let stem = Path::new(model_rel).with_extension("png");
    format!("{THUMB_DIR}/{}", stem.display())
}

/// Génère (ou réutilise) l'aperçu d'un fichier de modèle.
///
/// - `model_path` : chemin **absolu** du fichier de modèle.
/// - `model_rel`  : chemin du modèle relatif à la racine (`folder/file.stl`).
/// - `thumbs_root`: dossier de sortie des aperçus (`<root>/.easy3d-thumbs`).
///
/// Retourne le chemin **absolu** de l'image, ou `None` si le format n'a pas
/// d'aperçu (ou si la génération échoue). Ne régénère pas un aperçu déjà à jour
/// (plus récent que le modèle).
pub fn ensure_thumbnail(model_path: &Path, model_rel: &str, thumbs_root: &Path) -> Option<PathBuf> {
    // Format inconnu ou sans aperçu : on n'écrit rien (et on ne crée pas le
    // dossier des aperçus pour rien).
    if !formats::can_have_preview(model_rel) {
        return None;
    }

    let out_abs = thumbs_root.join(Path::new(model_rel).with_extension("png"));
    // Assure l'existence du sous-dossier (ex : `.easy3d-thumbs/chainsaw-man/`).
    if let Some(parent) = out_abs.parent() {
        fs::create_dir_all(parent).ok();
    }
    if is_fresh(&out_abs, model_path) {
        return Some(out_abs);
    }

    formats::make_thumbnail(model_path, &out_abs).and(Some(out_abs))
}

/// Boucle le dossier des modèles et garantit un aperçu pour chaque modèle.
///
/// `root` est la **racine des modèles** : elle sert de base pour calculer les
/// chemins relatifs, afin que les aperçus respectent la structure des
/// sous-dossiers (`.easy3d-thumbs/chainsaw-man/x.png`).
pub fn generate_all(root: &Path, thumbs_root: &Path) {
    generate_dir(root, root, thumbs_root);
}

/// Parcourt récursivement `dir` en gardant `base` comme référence des rel.
fn generate_dir(base: &Path, dir: &Path, thumbs_root: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    fs::create_dir_all(thumbs_root).ok();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Ne respecte pas les dossiers cachés (comme .easy3d-thumbs).
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            generate_dir(base, &path, thumbs_root);
        } else if let Some(rel) = rel_of(base, &path) {
            ensure_thumbnail(&path, &rel, thumbs_root);
        }
    }
}

/// Re-génère l'aperçu d'un fichier précis (appelé par le watcher sur un chemin).
pub fn ensure_for_changed(root: &Path, thumbs_root: &Path, abs_path: &Path) {
    let Some(rel) = rel_of(root, abs_path) else {
        return;
    };
    ensure_thumbnail(abs_path, &rel, thumbs_root);
}

/// Supprime l'aperçu mis en cache d'un fichier (appelé par le watcher lorsqu'un
/// fichier est supprimé). Aucun effet si aucun aperçu n'existe.
pub fn remove_for_path(root: &Path, thumbs_root: &Path, abs_path: &Path) {
    let Some(rel) = rel_of(root, abs_path) else {
        return;
    };
    let thumb = thumbs_root.join(thumb_rel_path(&rel));
    let _ = fs::remove_file(&thumb);
}

/// Fait suivre les aperçus d'un élément **renommé ou déplacé**.
///
/// `from` et `to` sont des chemins absolus. Un fichier a un `.png` unique ; un
/// dossier a tout un sous-arbre d'aperçus (les siens et ceux de ses
/// descendants), qu'on déplace d'un bloc : les régénérer coûterait le prix d'un
/// premier import, alors que l'arborescence des aperçus est le miroir de celle
/// des modèles.
///
/// C'est la forme d'aperçu **présente sur le disque** qui décide, pas le type de
/// l'élément : ainsi l'ordre des opérations (renommer le modèle avant ou après
/// ses aperçus) n'a pas d'importance.
pub fn move_for_path(root: &Path, thumbs_root: &Path, from: &Path, to: &Path) {
    let (Some(from_rel), Some(to_rel)) = (rel_of(root, from), rel_of(root, to)) else {
        return;
    };

    // `ensure_thumbnail` range l'aperçu d'un fichier à côté de ses frères
    // (`.easy3d-thumbs/Maison/x.png`), celui d'un dossier dans son propre
    // sous-dossier (`.easy3d-thumbs/Maison/Toit/`).
    let file_thumb = thumbs_root.join(Path::new(&from_rel).with_extension("png"));
    let (old, new) = if file_thumb.exists() {
        (
            file_thumb,
            thumbs_root.join(Path::new(&to_rel).with_extension("png")),
        )
    } else {
        (thumbs_root.join(&from_rel), thumbs_root.join(&to_rel))
    };

    if !old.exists() {
        return;
    }
    if let Some(parent) = new.parent() {
        fs::create_dir_all(parent).ok();
    }
    // La destination peut exister (aperçu d'un ancien fichier du même nom) : il
    // sera de toute façon régénéré s'il manque, donc on ne le garde pas deux fois.
    if new.exists() {
        let _ = if new.is_dir() {
            fs::remove_dir_all(&new)
        } else {
            fs::remove_file(&new)
        };
    }
    let _ = fs::rename(&old, &new);
}

// ─────────────────────────────────────────────────────────────────────────
// Utilitaires de chemin
// ─────────────────────────────────────────────────────────────────────────

/// Chemin relatif de `path` par rapport à `root` (séparateurs `/`).
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// `true` si l'aperçu existe et date d'après le modèle (donc à jour).
fn is_fresh(out: &Path, model: &Path) -> bool {
    match (fs::metadata(out), fs::metadata(model)) {
        (Ok(om), Ok(mm)) => om.modified().ok() >= mm.modified().ok(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemin_relatif_de_l_apercu() {
        // Le cache des aperçus reproduit l'arborescence des modèles.
        assert_eq!(
            thumb_rel_path("DemaAuto/boitier.stl"),
            ".easy3d-thumbs/DemaAuto/boitier.png"
        );
        // Un G-code produit bien un `.png` (et non `.gcode.png`).
        assert_eq!(thumb_rel_path("piece.gcode"), ".easy3d-thumbs/piece.png");
    }

    #[test]
    fn ensure_for_changed_ignore_les_fichiers_non_apercues() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);

        // Une note Markdown ne doit générer aucun aperçu.
        let note = root.join("lisez-moi.md");
        fs::write(&note, "# rien à voir").unwrap();
        ensure_for_changed(root, &thumbs, &note);
        assert!(!thumbs.exists());

        // Un chemin hors de la racine est ignoré sans erreur.
        ensure_for_changed(root, &thumbs, &dir.path().join("ailleurs.md"));
        assert!(!thumbs.exists());
    }

    #[test]
    fn un_gcode_sans_vignette_ne_produit_pas_de_fichier() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let gcode = root.join("piece.gcode");
        fs::write(&gcode, "; generated by Cura\nG1 X10 Y10 E1\n").unwrap();

        let out = root.join(THUMB_DIR).join("piece.png");
        assert!(ensure_thumbnail(&gcode, "piece.gcode", &root.join(THUMB_DIR)).is_none());
        assert!(!out.exists());
    }
}
