use crate::{formats, notes, thumbnail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Métadonnées d'un fichier découvert par le scan.
#[derive(Serialize)]
pub struct FileInfo {
    pub path: String,
    /// Chemin relatif à la racine des modèles (séparateurs `/`).
    pub rel: String,
    pub created: Option<u64>,
    pub modified: Option<u64>,
    /// Image d'aperçu associée (même nom, même dossier) si présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Note explicative (Markdown) associée, si présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Contenu d'un sous-dossier du répertoire surveillé.
#[derive(Serialize)]
pub struct FolderInfo {
    pub name: String,
    pub count: usize,
    /// Dernière modification du dossier, en secondes unix.
    ///
    /// Le plus récent entre le `mtime` du **répertoire** (ajout, suppression ou
    /// renommage d'une entrée) et celui de ses **fichiers** (édition de
    /// contenu, qui ne touche pas le répertoire). Autrement dit : la dernière
    /// fois que quelque chose a changé dans ce dossier.
    pub modified: Option<u64>,
    pub files: Vec<FileInfo>,
    /// Note explicative (Markdown) du dossier, si présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Scan structuré : distingue les fichiers racine et les fichiers des dossiers.
#[derive(Serialize)]
pub struct ModelsScan {
    pub folders: BTreeMap<String, FolderInfo>,
    pub files: Vec<FileInfo>,
}

impl ModelsScan {
    /// Nombre total de fichiers (racine + tous les sous-dossiers).
    pub fn total(&self) -> usize {
        self.files.len() + self.folders.values().map(|f| f.count).sum::<usize>()
    }
}

/// Liste récursive de **tous** les fichiers (racine + dossiers, à plat).
pub fn scan_files(root: &Path) -> Vec<FileInfo> {
    let mut out = Vec::new();
    scan_dir_recursive(root, root, &mut out);
    attach_sibling_images(&mut out);
    attach_generated_thumbs(&mut out);
    attach_notes(&mut out, root);
    out
}

/// Scan structuré : fichiers à la racine et fichiers groupés par sous-dossier.
pub fn scan_models(root: &Path) -> ModelsScan {
    let mut scan = ModelsScan {
        folders: BTreeMap::new(),
        files: Vec::new(),
    };

    let Ok(entries) = std::fs::read_dir(root) else {
        return scan;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if is_hidden(entry.file_name().to_str().unwrap_or("")) {
            continue;
        }

        if path.is_dir() {
            let mut files = Vec::new();
            scan_dir_recursive(&path, root, &mut files);
            attach_sibling_images(&mut files);
            attach_generated_thumbs(&mut files);
            attach_notes(&mut files, root);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let modified = folder_modified(&path, &files);
            scan.folders.insert(
                name.clone(),
                FolderInfo {
                    note: notes::read(root, &name),
                    name,
                    count: files.len(),
                    modified,
                    files,
                },
            );
        } else if path.is_file()
            && let Some(info) = file_info(&path, root)
        {
            scan.files.push(info);
        }
    }

    attach_sibling_images(&mut scan.files);
    attach_generated_thumbs(&mut scan.files);
    attach_notes(&mut scan.files, root);
    scan
}

fn scan_dir_recursive(dir: &Path, root: &Path, out: &mut Vec<FileInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if is_hidden(entry.file_name().to_str().unwrap_or("")) {
            continue;
        }

        if path.is_dir() {
            scan_dir_recursive(&path, root, out);
        } else if path.is_file()
            && let Some(info) = file_info(&path, root)
        {
            out.push(info);
        }
    }
}

/// `true` si le nom de fichier/dossier commence par `.` (fichier caché, ex
/// `.easy3d-thumbs/`, `.gitkeep`).
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Chemin relatif à `root`, normalisé en séparateurs `/`.
fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Dernière modification d'un dossier, en secondes unix.
///
/// Le plus récent entre son `mtime` (ajout, suppression ou renommage d'une
/// entrée) et celui des fichiers qu'il contient (édition de contenu, qui ne
/// touche pas le répertoire). `None` seulement si rien n'est lisible.
fn folder_modified(dir: &Path, files: &[FileInfo]) -> Option<u64> {
    let dir_mtime = std::fs::metadata(dir)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(to_unix_secs);
    let files_mtime = files.iter().filter_map(|f| f.modified).max();

    match (dir_mtime, files_mtime) {
        (Some(dir), Some(file)) => Some(dir.max(file)),
        (dir, file) => dir.or(file),
    }
}

/// Lit les métadonnées d'un fichier en `FileInfo`.
fn file_info(path: &Path, root: &Path) -> Option<FileInfo> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileInfo {
        path: path.display().to_string(),
        rel: rel_path(root, path),
        created: meta.created().ok().and_then(to_unix_secs),
        modified: meta.modified().ok().and_then(to_unix_secs),
        image: None,
        note: None,
    })
}

/// Nom de base sans extension d'un chemin relatif.
fn file_stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Associe à chaque fichier reconnu par le registre ([`formats`]) une image de
/// même nom dans le même dossier, si elle existe
/// (ex : `boitier.stl` + `boitier.png`).
fn attach_sibling_images(files: &mut [FileInfo]) {
    use std::collections::HashMap;
    let mut best: HashMap<String, String> = HashMap::new();
    for f in files.iter() {
        if formats::is_image(&f.rel) {
            let stem = file_stem(&f.rel);
            let prio = formats::image_priority(&formats::ext_of(&f.rel));
            let better = match best.get(&stem) {
                None => true,
                Some(cur) => prio < formats::image_priority(&formats::ext_of(cur)),
            };
            if better {
                best.insert(stem, f.rel.clone());
            }
        }
    }
    for f in files.iter_mut() {
        if formats::can_have_preview(&f.rel)
            && let Some(img) = best.get(&file_stem(&f.rel))
        {
            f.image = Some(img.clone());
        }
    }
}

/// Associe à chaque modèle **sans image sœur** l'aperçu PNG généré par le
/// backend (dans `<root>/.easy3d-thumbs/`), si celui-ci existe sur le disque.
/// L'image explicite (même nom, même dossier) reste prioritaire.
fn attach_generated_thumbs(files: &mut [FileInfo]) {
    use std::path::PathBuf;
    for f in files.iter_mut() {
        if f.image.is_some() || !formats::can_have_preview(&f.rel) {
            continue;
        }
        // Racine des modèles = chemin absolu du modèle moins ses composantes rel.
        let mut root = PathBuf::from(&f.path);
        for _ in f.rel.split('/') {
            root.pop();
        }
        let thumb_rel = thumbnail::thumb_rel_path(&f.rel);
        if root.join(&thumb_rel).is_file() {
            f.image = Some(thumb_rel);
        }
    }
}

/// Associe à chaque élément sa note Markdown (`.easy3d-notes/<rel>.md`).
fn attach_notes(files: &mut [FileInfo], root: &Path) {
    for f in files.iter_mut() {
        f.note = notes::read(root, &f.rel);
    }
}

/// Convertit un `SystemTime` en secondes écoulées depuis `UNIX_EPOCH`.
fn to_unix_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_files_liste_fichiers_nestes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("a.txt"), "hello").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/b.txt"), "world").unwrap();

        let files = scan_files(root);

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.path.ends_with("a.txt")));
        assert!(files.iter().any(|f| f.path.ends_with("b.txt")));
        // La date de modification est toujours disponible.
        assert!(files.iter().all(|f| f.modified.is_some()));
    }

    #[test]
    fn scan_models_expose_la_date_de_modification_du_dossier() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/a.txt"), "hello").unwrap();

        let scan = scan_models(root);
        let folder = &scan.folders["sub"];

        assert_eq!(folder.count, 1);
        // Le dossier porte sa propre date : le plus récent entre ses entrées
        // (mtime du répertoire) et le contenu de ses fichiers.
        let folder_mtime = folder.modified.expect("date du dossier");
        let file_mtime = folder.files[0].modified.expect("date du fichier");
        assert!(folder_mtime >= file_mtime);
    }

    #[test]
    fn scan_files_ignore_les_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/file.txt"), "x").unwrap();

        let files = scan_files(root);

        // Seul le fichier est listé, pas le dossier `sub`.
        assert_eq!(files.len(), 1);
        assert!(files[0].path.ends_with("file.txt"));
    }

    #[test]
    fn scan_files_dossier_vide() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_files(dir.path()).is_empty());
    }

    #[test]
    fn total_compte_racine_et_sous_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "x").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/b.txt"), "y").unwrap();
        fs::write(root.join("sub/c.txt"), "z").unwrap();

        assert_eq!(scan_models(root).total(), 3);
    }

    #[test]
    fn le_dossier_des_notes_est_ignore_par_le_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/a.stl"), "x").unwrap();
        fs::create_dir_all(root.join(".easy3d-notes/sub")).unwrap();
        fs::write(root.join(".easy3d-notes/sub/a.stl.md"), "# Note").unwrap();

        let scan = scan_models(root);

        // Les notes ne sont ni des dossiers ni des fichiers du catalogue.
        assert!(!scan.folders.contains_key(".easy3d-notes"));
        assert_eq!(scan.total(), 1);
    }

    #[test]
    fn scan_attache_les_notes_aux_fichiers_et_dossiers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("DemaAuto")).unwrap();
        fs::write(root.join("DemaAuto/a.stl"), "x").unwrap();
        fs::create_dir_all(root.join(".easy3d-notes/DemaAuto")).unwrap();
        fs::write(
            root.join(".easy3d-notes/DemaAuto/a.stl.md"),
            "# Note fichier",
        )
        .unwrap();
        fs::write(root.join(".easy3d-notes/DemaAuto.md"), "# Note dossier").unwrap();

        let scan = scan_models(root);
        let folder = &scan.folders["DemaAuto"];

        assert_eq!(folder.note.as_deref(), Some("# Note dossier"));
        assert_eq!(folder.files[0].note.as_deref(), Some("# Note fichier"));
    }

    #[test]
    fn notes_absentes_restent_nulles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.stl"), "x").unwrap();

        let scan = scan_models(root);
        assert!(scan.files[0].note.is_none());
    }

    #[test]
    fn eligibility_a_un_apercu() {
        // Modèles 3D et G-code : oui. Autre chose : non.
        // (Le détail des extensions appartient au module `formats`.)
        for rel in ["a.stl", "a.obj", "a.3mf", "a.gcode", "a.gco", "sub/a.GCODE"] {
            assert!(formats::can_have_preview(rel), "devrait accepter {rel}");
        }
        for rel in ["notes.md", "photo.png", "readme.txt", ""] {
            assert!(!formats::can_have_preview(rel), "devrait refuser {rel}");
        }
    }

    #[test]
    fn image_soeur_associee_a_un_gcode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("piece.gcode"), "G1 X0").unwrap();
        fs::write(root.join("piece.png"), "faux png").unwrap();

        let files = scan_files(root);
        let gcode = files.iter().find(|f| f.rel == "piece.gcode").unwrap();
        assert_eq!(gcode.image.as_deref(), Some("piece.png"));
    }

    #[test]
    fn apercu_genere_associe_a_un_gcode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("piece.gcode"), "G1 X0").unwrap();
        fs::create_dir_all(root.join(".easy3d-thumbs")).unwrap();
        fs::write(root.join(".easy3d-thumbs/piece.png"), "faux png").unwrap();

        let files = scan_files(root);
        let gcode = files.iter().find(|f| f.rel == "piece.gcode").unwrap();
        assert_eq!(gcode.image.as_deref(), Some(".easy3d-thumbs/piece.png"));
    }
}
