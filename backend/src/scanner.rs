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
}

/// Contenu d'un sous-dossier du répertoire surveillé.
#[derive(Serialize)]
pub struct FolderInfo {
    pub name: String,
    pub count: usize,
    pub files: Vec<FileInfo>,
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

        if path.is_dir() {
            let mut files = Vec::new();
            scan_dir_recursive(&path, root, &mut files);
            attach_sibling_images(&mut files);
            scan.folders.insert(
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                FolderInfo {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    count: files.len(),
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
    scan
}

fn scan_dir_recursive(dir: &Path, root: &Path, out: &mut Vec<FileInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            scan_dir_recursive(&path, root, out);
        } else if path.is_file()
            && let Some(info) = file_info(&path, root)
        {
            out.push(info);
        }
    }
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

/// Lit les métadonnées d'un fichier en `FileInfo`.
fn file_info(path: &Path, root: &Path) -> Option<FileInfo> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileInfo {
        path: path.display().to_string(),
        rel: rel_path(root, path),
        created: meta.created().ok().and_then(to_unix_secs),
        modified: meta.modified().ok().and_then(to_unix_secs),
        image: None,
    })
}

/// Extension (minuscule) d'un chemin relatif.
fn ext_of(rel: &str) -> String {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Nom de base sans extension d'un chemin relatif.
fn file_stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// `true` si le fichier est un modèle 3D affichable (STL / OBJ).
fn is_model(rel: &str) -> bool {
    matches!(ext_of(rel).as_str(), "stl" | "obj")
}

/// `true` si le fichier est une image d'aperçu.
fn is_image(rel: &str) -> bool {
    matches!(ext_of(rel).as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif")
}

/// Priorité d'extension image (0 = préférée), pour choisir l'aperçu.
fn img_priority(ext: &str) -> u32 {
    match ext {
        "png" => 0,
        "jpg" => 1,
        "jpeg" => 2,
        "webp" => 3,
        "gif" => 4,
        _ => 9,
    }
}

/// Associe à chaque modèle (STL / OBJ) une image de même nom dans le même
/// dossier, si elle existe (ex : `boitier.stl` + `boitier.png`).
fn attach_sibling_images(files: &mut [FileInfo]) {
    use std::collections::HashMap;
    let mut best: HashMap<String, String> = HashMap::new();
    for f in files.iter() {
        if is_image(&f.rel) {
            let stem = file_stem(&f.rel);
            let prio = img_priority(&ext_of(&f.rel));
            let better = match best.get(&stem) {
                None => true,
                Some(cur) => prio < img_priority(&ext_of(cur)),
            };
            if better {
                best.insert(stem, f.rel.clone());
            }
        }
    }
    for f in files.iter_mut() {
        if is_model(&f.rel) {
            if let Some(img) = best.get(&file_stem(&f.rel)) {
                f.image = Some(img.clone());
            }
        }
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
}

