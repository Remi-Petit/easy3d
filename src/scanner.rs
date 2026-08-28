use serde::Serialize;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Métadonnées d'un fichier découvert par le scan.
#[derive(Serialize)]
pub struct FileInfo {
    pub path: String,
    pub created: Option<u64>,
    pub modified: Option<u64>,
}

/// Liste récursive des fichiers (hors dossiers) avec leurs dates de
/// création/modification, en secondes Unix.
pub fn scan_files(root: &Path) -> Vec<FileInfo> {
    let mut out = Vec::new();
    scan_dir_recursive(root, &mut out);
    out
}

fn scan_dir_recursive(dir: &Path, out: &mut Vec<FileInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            scan_dir_recursive(&path, out);
        } else if path.is_file()
            && let Ok(meta) = entry.metadata()
        {
            out.push(FileInfo {
                path: path.display().to_string(),
                created: meta.created().ok().and_then(to_unix_secs),
                modified: meta.modified().ok().and_then(to_unix_secs),
            });
        }
    }
}

/// Convertit un `SystemTime` en secondes écoulées depuis `UNIX_EPOCH`.
fn to_unix_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}
