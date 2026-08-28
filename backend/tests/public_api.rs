//! Test d'intégration : consomme la bibliothèque `easy3d` comme un
//! utilisateur externe, via son API publique uniquement.

use easy3d::scanner::scan_files;
use std::fs;

#[test]
fn scan_files_publiqueparcourt_recursivement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("nested/data.txt"), "42").unwrap();

    let files = scan_files(root);

    assert_eq!(files.len(), 1);
    assert!(files[0].path.ends_with("data.txt"));
    assert!(files[0].modified.is_some());
}
