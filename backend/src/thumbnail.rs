//! Aperçus PNG : cache et orchestration **côté backend**.
//!
//! Les formats eux-mêmes (STL, OBJ, 3MF, G-code…) vivent dans [`crate::formats`] :
//! ce module décide seulement *quand* générer un aperçu, *où* le ranger et
//! *quand* réutiliser celui déjà présent.
//!
//! Pour chaque fichier reconnu dans `models/`, on produit un PNG dans
//! `<models_root>/.easy3d-thumbs/`, en reproduisant l'arborescence
//! (`chainsaw-man/x.stl` → `.easy3d-thumbs/chainsaw-man/x.stl.png`). Cette image
//! sert d'aperçu sur le site — plus léger qu'un rendu 3D navigateur pour les
//! grandes grilles.

use crate::formats;
use std::fs;
use std::path::{Path, PathBuf};

/// Nom du dossier caché (dans `models/`) qui regroupe les aperçus générés.
pub const THUMB_DIR: &str = ".easy3d-thumbs";

/// Version du **rendu** des aperçus.
///
/// À incrémenter dès que leur apparence change (palette, cadrage, mise au style
/// des vignettes de slicer) : le cache est alors vidé au démarrage, et tout est
/// régénéré avec le nouveau style. Sans ce marqueur, les aperçus déjà en cache
/// garderaient l'ancienne allure — [`is_fresh`] ne compare que les horodatages,
/// il n'a aucun moyen de savoir que le *rendu* a changé.
const STYLE_VERSION: u32 = 2;

/// Fichier qui retient la version du style, dans le dossier des aperçus.
const STYLE_FILE: &str = ".style";

/// Vide le cache des aperçus quand le **style** du rendu a changé.
///
/// Appelé au démarrage, avant [`generate_all`]. Retourne `true` si un cache d'un
/// style précédent a été vidé (les aperçus sont des artefacts : ils se
/// régénèrent juste après).
pub fn apply_style(thumbs_root: &Path) -> bool {
    let marker = thumbs_root.join(STYLE_FILE);
    let known = fs::read_to_string(&marker)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok());

    if known == Some(STYLE_VERSION) {
        return false;
    }

    // Marqueur absent alors que le cache est déjà rempli : aperçus d'avant le
    // marqueur, donc d'un autre style. Même traitement qu'un changement de
    // version. Un dossier vide, lui, n'a rien à vider.
    let outdated = fs::read_dir(thumbs_root)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);

    if outdated && let Ok(entries) = fs::read_dir(thumbs_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }

    fs::create_dir_all(thumbs_root).ok();
    let _ = fs::write(&marker, STYLE_VERSION.to_string());
    outdated
}

/// Chemin (relatif à la racine des modèles) de l'aperçu d'un fichier donné.
///
/// `models/chainsaw-man/Chainsaw_Man.stl` →
/// `.easy3d-thumbs/chainsaw-man/Chainsaw_Man.stl.png`
///
/// Le nom du modèle est repris **entier**, extension comprise. Deux modèles qui
/// ne diffèrent que par leur format (`pieces.stl`, `pieces.gcode` — le cas de
/// tous les exports slicer) sont deux fichiers distincts pour le catalogue :
/// sans l'extension, ils se partageraient un unique `.png`, et régénérer l'un
/// écraserait l'aperçu de l'autre.
pub fn thumb_rel_path(model_rel: &str) -> String {
    format!("{THUMB_DIR}/{model_rel}.png")
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

    let out_abs = thumbs_root.join(format!("{model_rel}.png"));
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
/// sous-dossiers (`.easy3d-thumbs/chainsaw-man/x.stl.png`).
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

/// Supprime l'aperçu mis en cache d'un fichier ou d'un dossier (appelé par le
/// watcher sur un chemin supprimé, et par `DELETE` côté API). Aucun effet si
/// aucun aperçu n'existe.
///
/// Un fichier a son `.png` ; un dossier a **tout un sous-arbre** d'aperçus
/// (`.easy3d-thumbs/Maison/Toit/…`), qui part d'un bloc. Les deux formes sont
/// tentées : le chemin n'existe plus sur disque quand c'est le watcher qui
/// appelle, on ne peut donc pas se fier à son type.
///
/// Le chemin est reconstruit depuis `thumbs_root` : [`thumb_rel_path`] inclut
/// déjà `THUMB_DIR`, l'y joindre à nouveau viserait
/// `<root>/.easy3d-thumbs/.easy3d-thumbs/…` — un aperçu qui n'était donc jamais
/// supprimé.
pub fn remove_for_path(root: &Path, thumbs_root: &Path, abs_path: &Path) {
    let Some(rel) = rel_of(root, abs_path) else {
        return;
    };

    let _ = fs::remove_file(thumbs_root.join(format!("{rel}.png")));

    let subtree = thumbs_root.join(&rel);
    if subtree.is_dir() {
        let _ = fs::remove_dir_all(&subtree);
    }
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

    // Un aperçu de **fichier** porte le nom complet du modèle (`piece.stl.png`),
    // celui d'un **dossier** est le dossier du même nom
    // (`.easy3d-thumbs/Maison/Toit/`) : c'est cette forme, présente sur le
    // disque, qui décide — pas le type de l'élément.
    let file_thumb = thumbs_root.join(format!("{from_rel}.png"));
    let (old, new) = if file_thumb.exists() {
        (file_thumb, thumbs_root.join(format!("{to_rel}.png")))
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

/// Supprime les aperçus qui n'ont plus de modèle.
///
/// Deux cas, tous deux sans modèle correspondant : les `.png` laissés par un
/// ancien nommage (l'aperçu s'appelait `piece.png` pour `piece.stl`, ce qui
/// faisait collision entre formats) et ceux d'un modèle supprimé hors de
/// l'interface (le watcher, lui, les enlève au fil de l'eau, mais il ne voit
/// rien d'un montage virtualisé).
///
/// Les dossiers devenus **vides** par ce nettoyage partent aussi : supprimer un
/// dossier de modèles laissait sinon son arborescence d'aperçus derrière lui.
/// `remove_dir` échoue sur un dossier non vide, il n'y a donc rien à craindre en
/// le tentant, du plus profond au moins profond.
///
/// Appelé au démarrage, après [`generate_all`] : à ce stade, tout aperçu
/// légitime vient d'être écrit. Retourne le nombre de **fichiers** supprimés.
pub fn prune(root: &Path, thumbs_root: &Path) -> usize {
    let mut removed = 0;
    let mut pending = vec![thumbs_root.to_path_buf()];
    let mut visited: Vec<PathBuf> = Vec::new();

    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        visited.push(dir);

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }

            // `<rel>.png` → `<rel>` : le modèle doit encore exister sur disque.
            let Some(model) = rel_of(thumbs_root, &path)
                .and_then(|rel| rel.strip_suffix(".png").map(str::to_string))
            else {
                continue;
            };
            if !root.join(&model).is_file() && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }

    visited.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in visited {
        if dir != thumbs_root {
            let _ = fs::remove_dir(&dir);
        }
    }

    removed
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
    use std::time::Duration;

    /// STL ASCII minimal mais valide : une facette, que le backend rastérise.
    const STL: &str = "solid piece\n\
         facet normal 0 0 1\n\
         outer loop\n\
         vertex 0 0 0\n\
         vertex 1 0 0\n\
         vertex 0 1 0\n\
         endloop\n\
         endfacet\n\
         endsolid piece\n";

    /// OBJ minimal, **d'une autre forme** (un carré au lieu d'un triangle) :
    /// deux aperçus visuellement différents, ce dont dépendent les tests.
    const OBJ_CARRE: &str = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3\nf 1 3 4\n";

    /// Même chose qu'un triangle : le rendu d'un OBJ qui devient identique au
    /// STL, aperçu dans un **autre** fichier.
    const OBJ_TRIANGLE: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

    #[test]
    fn chemin_relatif_de_l_apercu() {
        // Le cache des aperçus reproduit l'arborescence des modèles.
        assert_eq!(
            thumb_rel_path("DemaAuto/boitier.stl"),
            ".easy3d-thumbs/DemaAuto/boitier.stl.png"
        );
        // Le nom du modèle est repris **entier**, extension comprise : sans ça,
        // un `.stl` et un `.gcode` homonymes se partageraient un unique `.png`.
        assert_eq!(thumb_rel_path("piece.stl"), ".easy3d-thumbs/piece.stl.png");
        assert_eq!(
            thumb_rel_path("piece.gcode"),
            ".easy3d-thumbs/piece.gcode.png"
        );
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

        let out = root.join(THUMB_DIR).join("piece.gcode.png");
        assert!(ensure_thumbnail(&gcode, "piece.gcode", &root.join(THUMB_DIR)).is_none());
        assert!(!out.exists());
    }

    /// Le cas signalé : deux formats homonymes (l'export d'un slicer) doivent
    /// garder chacun leur aperçu. Avant, tous deux visaient
    /// `.easy3d-thumbs/piece.png` : régénérer l'un écrasait l'image de l'autre —
    /// et le sens de l'écrasement dépendait de l'ordre des modifications.
    #[test]
    fn deux_modeles_homonymes_gardent_leur_apercu() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);

        let stl = root.join("piece.stl");
        let obj = root.join("piece.obj");
        fs::write(&stl, STL).unwrap();
        fs::write(&obj, OBJ_CARRE).unwrap();

        let stl_out = ensure_thumbnail(&stl, "piece.stl", &thumbs).unwrap();
        let obj_out = ensure_thumbnail(&obj, "piece.obj", &thumbs).unwrap();
        assert_ne!(stl_out, obj_out, "un aperçu pour deux modèles");
        assert!(stl_out.is_file() && obj_out.is_file());

        let stl_bytes = fs::read(&stl_out).unwrap();
        let obj_bytes = fs::read(&obj_out).unwrap();
        // Prémisse : les deux formes donnent deux images différentes. Sans ça,
        // « l'aperçu du STL n'a pas bougé » ne prouverait rien.
        assert_ne!(
            stl_bytes, obj_bytes,
            "aperçus identiques : le test ne prouverait rien"
        );

        // Le modèle homonyme est modifié (donc plus récent que son aperçu) : son
        // aperçu est refait, celui du STL reste intact — même si le nouveau rendu
        // est pixel pour pixel celui du STL.
        std::thread::sleep(Duration::from_millis(30));
        fs::write(&obj, OBJ_TRIANGLE).unwrap();
        ensure_thumbnail(&obj, "piece.obj", &thumbs).unwrap();

        assert_eq!(
            fs::read(&stl_out).unwrap(),
            stl_bytes,
            "l'aperçu du STL a été écrasé par celui de l'OBJ"
        );
        assert_ne!(
            fs::read(&obj_out).unwrap(),
            obj_bytes,
            "l'aperçu de l'OBJ n'a pas été refait"
        );
    }

    /// Renommer ou supprimer un modèle ne touche pas l'aperçu de son homonyme.
    #[test]
    fn renommer_ou_supprimer_ne_touche_pas_l_homonyme() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);

        let stl = root.join("piece.stl");
        let obj = root.join("piece.obj");
        fs::write(&stl, STL).unwrap();
        fs::write(&obj, OBJ_CARRE).unwrap();
        ensure_thumbnail(&stl, "piece.stl", &thumbs).unwrap();
        ensure_thumbnail(&obj, "piece.obj", &thumbs).unwrap();

        // Renommage : l'aperçu suit **son** modèle, pas celui du même nom racine.
        let to = root.join("toit.stl");
        fs::rename(&stl, &to).unwrap();
        move_for_path(root, &thumbs, &stl, &to);
        assert!(thumbs.join("toit.stl.png").is_file());
        assert!(!thumbs.join("piece.stl.png").exists());
        assert!(
            thumbs.join("piece.obj.png").is_file(),
            "l'aperçu de l'OBJ a suivi le renommage du STL"
        );

        // Suppression : seul l'aperçu du fichier supprimé s'en va.
        remove_for_path(root, &thumbs, &to);
        assert!(!thumbs.join("toit.stl.png").exists());
        assert!(thumbs.join("piece.obj.png").is_file());
    }

    /// Supprimer un dossier emporte tout son sous-arbre d'aperçus, sans toucher
    /// aux voisins ni au reste du cache.
    #[test]
    fn remove_for_path_d_un_dossier_emporte_le_sous_arbre() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);
        fs::create_dir_all(root.join("Maison/Toit")).unwrap();

        for rel in [
            "Maison/Toit/piece.stl.png",
            "Maison/Toit/sous/vis.stl.png",
            "Maison/garde.stl.png",
        ] {
            let path = thumbs.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "png").unwrap();
        }

        remove_for_path(root, &thumbs, &root.join("Maison/Toit"));

        assert!(!thumbs.join("Maison/Toit").exists());
        assert!(thumbs.join("Maison/garde.stl.png").is_file());
    }

    #[test]
    fn apply_style_regenere_les_apercus_d_un_style_precedent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);
        fs::create_dir_all(thumbs.join("Maison")).unwrap();
        fs::write(thumbs.join("Maison/x.stl.png"), "aperçu").unwrap();

        // Cache sans marqueur : aperçus d'avant la version de style, tout part.
        assert!(apply_style(&thumbs));
        assert!(!thumbs.join("Maison").exists());
        assert!(thumbs.is_dir(), "le dossier des aperçus reste en place");

        // Marqueur à jour : un cache neuf (ou regénéré) n'est plus touché.
        assert!(!apply_style(&thumbs));
        fs::create_dir_all(thumbs.join("Maison")).unwrap();
        fs::write(thumbs.join("Maison/x.stl.png"), "aperçu").unwrap();
        assert!(!apply_style(&thumbs));
        assert!(thumbs.join("Maison/x.stl.png").is_file());

        // Version périmée : même traitement.
        fs::write(thumbs.join(STYLE_FILE), "0").unwrap();
        assert!(apply_style(&thumbs));
        assert!(!thumbs.join("Maison").exists());
        assert_eq!(
            fs::read_to_string(thumbs.join(STYLE_FILE)).unwrap().trim(),
            STYLE_VERSION.to_string()
        );
    }

    /// Les aperçus sans modèle sont retirés : ceux d'un ancien nommage
    /// (`piece.png` pour `piece.stl`) et ceux d'un modèle disparu.
    #[test]
    fn prune_supprime_les_apercus_orphelins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let thumbs = root.join(THUMB_DIR);
        fs::create_dir_all(root.join("Maison")).unwrap();
        fs::create_dir_all(thumbs.join("Maison")).unwrap();
        fs::write(root.join("piece.stl"), STL).unwrap();
        fs::write(root.join("Maison/piece.stl"), STL).unwrap();

        // Deux légitimes (leurs modèles existent), deux orphelins — et un
        // dossier vidé par le nettoyage.
        fs::write(thumbs.join("piece.stl.png"), "png").unwrap();
        fs::write(thumbs.join("Maison/piece.stl.png"), "png").unwrap();
        fs::write(thumbs.join("piece.png"), "ancien nommage").unwrap();
        fs::create_dir_all(thumbs.join("Maison/parti")).unwrap();
        fs::write(thumbs.join("Maison/parti.stl.png"), "modèle supprimé").unwrap();
        fs::write(thumbs.join("Maison/parti/piece.stl.png"), "idem").unwrap();

        assert_eq!(prune(root, &thumbs), 3);
        assert!(thumbs.join("piece.stl.png").is_file());
        assert!(thumbs.join("Maison/piece.stl.png").is_file());
        assert!(!thumbs.join("piece.png").exists());
        assert!(!thumbs.join("Maison/parti.stl.png").exists());
        // Le dossier vidé part avec ; un dossier qui garde un aperçu légitime
        // reste en place.
        assert!(!thumbs.join("Maison/parti").exists());
        assert!(thumbs.join("Maison").is_dir());
        // Sans orphelin, plus rien à faire.
        assert_eq!(prune(root, &thumbs), 0);
    }
}
