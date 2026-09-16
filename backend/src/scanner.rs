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
    /// Version de cette image (voir [`version_token`]).
    ///
    /// Le frontend la recopie **telle quelle** dans l'URL de l'aperçu : le
    /// serveur peut alors répondre `immutable`, et le navigateur ne redemande
    /// plus la vignette à chaque chargement de page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_version: Option<String>,
    /// Note explicative (Markdown) associée, si présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Contenu d'un sous-dossier du répertoire surveillé.
///
/// `files` est **récursif** : il contient aussi les fichiers rangés dans les
/// sous-dossiers (voir [`SubFolderInfo`], qui les décrit à part).
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
    /// Sous-dossiers, à plat (vide s'il n'y en a aucun).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subfolders: Vec<SubFolderInfo>,
    /// Note explicative (Markdown) du dossier, si présente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Sous-dossier d'un dossier de premier niveau.
///
/// Les sous-dossiers arrivent **à plat** (tous les descendants, pas seulement
/// les enfants directs) : le frontend reconstitue la hiérarchie à partir des
/// `rel`, exactement comme il le fait pour les fichiers. Le JSON ne dépend donc
/// pas de la profondeur de l'arborescence.
#[derive(Serialize)]
pub struct SubFolderInfo {
    /// Chemin relatif à la racine des modèles (`Maison/sous`).
    pub rel: String,
    /// Nom du dossier seul (dernier segment du chemin).
    pub name: String,
    /// Nombre de fichiers contenus, **récursivement**.
    pub count: usize,
    /// Dernière modification (même calcul que [`FolderInfo::modified`]).
    pub modified: Option<u64>,
    /// Note explicative (Markdown) du sous-dossier, si présente.
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
    attach_sibling_images(&mut out, root);
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
            attach_sibling_images(&mut files, root);
            attach_generated_thumbs(&mut files);
            attach_notes(&mut files, root);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let modified = folder_modified(&path, &files);
            let mut subfolders = Vec::new();
            scan_subfolders(&path, root, &files, &mut subfolders);
            scan.folders.insert(
                name.clone(),
                FolderInfo {
                    note: notes::read(root, &name),
                    name,
                    count: files.len(),
                    modified,
                    files,
                    subfolders,
                },
            );
        } else if path.is_file()
            && let Some(info) = file_info(&path, root)
        {
            scan.files.push(info);
        }
    }

    attach_sibling_images(&mut scan.files, root);
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

/// Décrit les sous-dossiers d'un dossier, à plat : tous les descendants, chacun
/// avec le nombre de fichiers qu'il contient.
///
/// Le compte et la date se calculent à partir des fichiers **déjà** collectés par
/// [`scan_dir_recursive`] (filtre sur préfixe de chemin) : aucun second parcours
/// du disque, et donc le même compte que celui affiché pour le dossier parent.
fn scan_subfolders(dir: &Path, root: &Path, files: &[FileInfo], out: &mut Vec<SubFolderInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || is_hidden(entry.file_name().to_str().unwrap_or("")) {
            continue;
        }

        let rel = rel_path(root, &path);
        let prefix = format!("{rel}/");
        let inside = || files.iter().filter(|f| f.rel.starts_with(&prefix));

        out.push(SubFolderInfo {
            name: entry.file_name().to_string_lossy().into_owned(),
            count: inside().count(),
            modified: max_modified(&path, inside().map(|f| f.modified)),
            note: notes::read(root, &rel),
            rel,
        });

        // Descendants : listés à la suite, le frontend filtrant par parent.
        scan_subfolders(&path, root, files, out);
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

/// Dernière modification d'un dossier, en secondes unix.
///
/// Le plus récent entre son `mtime` (ajout, suppression ou renommage d'une
/// entrée) et celui des fichiers qu'il contient (édition de contenu, qui ne
/// touche pas le répertoire). `None` seulement si rien n'est lisible.
fn folder_modified(dir: &Path, files: &[FileInfo]) -> Option<u64> {
    max_modified(dir, files.iter().map(|f| f.modified))
}

/// Le plus récent entre le `mtime` d'un dossier et celui des fichiers donnés.
fn max_modified(dir: &Path, files: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let dir_mtime = std::fs::metadata(dir)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(to_unix_secs);
    let files_mtime = files.max().flatten();

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
        image_version: None,
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
///
/// `root` sert à calculer la version de l'aperçu (voir [`version_token`]).
fn attach_sibling_images(files: &mut [FileInfo], root: &Path) {
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
            f.image_version = file_version(&root.join(img));
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
        let thumb = root.join(&thumb_rel);
        if let Ok(meta) = std::fs::metadata(&thumb)
            && meta.is_file()
        {
            f.image = Some(thumb_rel);
            f.image_version = version_token(&meta);
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

/// Version d'un fichier sur disque, ou `None` s'il est introuvable.
fn file_version(path: &Path) -> Option<String> {
    version_token(&std::fs::metadata(path).ok()?)
}

/// Jeton de version d'un fichier : `mtime` en nanosecondes + taille.
///
/// Il circule **tel quel** entre le scan (qui l'annonce au frontend) et le
/// serveur de fichiers (qui le renvoie en `ETag` et compare au paramètre `v`) :
/// les deux calculent la même chaîne, donc une URL déclarée « immuable » le
/// reste vraiment. La nanoseconde évite qu'une réécriture dans la même seconde
/// passe inaperçue ; la taille couvre les systèmes de fichiers à `mtime` grossier.
pub fn version_token(meta: &std::fs::Metadata) -> Option<String> {
    let nanos = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{nanos}-{}", meta.len()))
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

    #[test]
    fn sous_dossiers_decrits_a_plat() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Maison/sous")).unwrap();
        fs::create_dir_all(root.join(".easy3d-notes/Maison")).unwrap();
        // Dossier interne au backend : jamais un sous-dossier du catalogue.
        fs::create_dir_all(root.join("Maison/.easy3d-thumbs")).unwrap();
        fs::write(root.join("Maison/a.stl"), "x").unwrap();
        fs::write(root.join("Maison/sous/b.stl"), "y").unwrap();
        fs::write(root.join(".easy3d-notes/Maison/sous.md"), "# Note du sous-dossier").unwrap();

        let scan = scan_models(root);
        let maison = &scan.folders["Maison"];

        // Les fichiers restent listés à plat, sous-dossier compris…
        assert_eq!(maison.count, 2);
        // …et les sous-dossiers sont décrits à part.
        let rels: Vec<&str> = maison.subfolders.iter().map(|s| s.rel.as_str()).collect();
        assert_eq!(rels, vec!["Maison/sous"]);

        let sous = &maison.subfolders[0];
        assert_eq!(sous.name, "sous");
        assert_eq!(sous.count, 1);
        assert!(sous.modified.is_some());
        assert_eq!(sous.note.as_deref(), Some("# Note du sous-dossier"));
    }

    #[test]
    fn sous_dossiers_imbriques_et_vide() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Maison/sous/encore")).unwrap();
        fs::create_dir_all(root.join("Maison/vide")).unwrap();
        fs::write(root.join("Maison/sous/encore/x.stl"), "x").unwrap();

        let scan = scan_models(root);
        // Tous les descendants sont listés à la suite : c'est le frontend qui
        // reconstitue la hiérarchie, en filtrant sur le dossier parent.
        let mut rels: Vec<&str> = scan.folders["Maison"]
            .subfolders
            .iter()
            .map(|s| s.rel.as_str())
            .collect();
        rels.sort();
        assert_eq!(rels, vec!["Maison/sous", "Maison/sous/encore", "Maison/vide"]);

        // Un sous-dossier vide est décrit lui aussi : l'interface doit pouvoir
        // distinguer « vide » de « absent ».
        let vide = scan.folders["Maison"]
            .subfolders
            .iter()
            .find(|s| s.rel == "Maison/vide")
            .unwrap();
        assert_eq!(vide.count, 0);
    }

    #[test]
    fn dossier_sans_sous_dossier() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("DemaAuto")).unwrap();
        fs::write(root.join("DemaAuto/a.stl"), "x").unwrap();

        let scan = scan_models(root);
        assert!(scan.folders["DemaAuto"].subfolders.is_empty());
    }
}
