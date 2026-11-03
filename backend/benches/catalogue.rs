//! Bancs du catalogue : ce que coûte `GET /models`.
//!
//!     cargo bench --bench catalogue
//!
//! Le handler enchaîne deux travaux : un **parcours du dossier** (un `stat` par
//! fichier, un `read` par note) puis la **sérialisation JSON** de tout le
//! catalogue. Les mesurer séparément dit où corriger — un cache du scan et une
//! forme de JSON plus légère ne se remplacent pas.
//!
//! Mesure de référence utile : le banc `scan` seul explose sur un montage
//! virtualisé (Docker Desktop), où chaque `stat` coûte bien plus qu'un
//! changement de structure en mémoire.
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use easy3d::api::ModelsResponse;
use easy3d::config::Config;
use easy3d::scanner::scan_models;
use std::fs;
use std::hint::black_box;
use std::path::Path;

/// Catalogue synthétique : `dossiers` dossiers de `fichiers` modèles, une note
/// Markdown par dossier, et un sous-dossier vide.
fn catalogue(root: &Path, dossiers: usize, fichiers: usize) {
    for d in 0..dossiers {
        let nom = format!("Dossier {d}");
        let dir = root.join(&nom);
        fs::create_dir_all(dir.join("Sous-dossier")).unwrap();
        for f in 0..fichiers {
            // Le contenu n'est jamais lu par le scan : seule l'extension compte.
            fs::write(dir.join(format!("piece {f}.stl")), b"solid x\nendsolid x\n").unwrap();
        }
        // Une note par dossier : c'est un fichier de plus à ouvrir à chaque scan.
        easy3d::notes::write(root, &nom, "# Note\n\nUn texte court.\n").unwrap();
    }
}

/// Coût du scan seul, pour deux tailles de catalogue.
fn scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("catalogue/scan");
    for (dossiers, fichiers) in [(10usize, 20usize), (50, 20)] {
        let dir = tempfile::tempdir().unwrap();
        catalogue(dir.path(), dossiers, fichiers);
        let total = dossiers * fichiers;
        group.throughput(Throughput::Elements(total as u64));
        group.bench_with_input(BenchmarkId::from_parameter(total), &dir, |b, dir| {
            b.iter(|| black_box(scan_models(black_box(dir.path()))))
        });
        // `dir` vit jusqu'à la fin de l'itération de boucle : le banc a fini.
    }
    group.finish();
}

/// Sérialisation seule, puis le couple scan + sérialisation (ce que fait le
/// handler).
fn json(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    catalogue(dir.path(), 10, 20);
    let config = Config::default();
    let presets = easy3d::ai::presets::defaults();

    // Construite une fois : on ne mesure que la conversion en texte.
    let reponse = ModelsResponse::from_scan(scan_models(dir.path()), &config, &presets);
    c.bench_function("catalogue/json_seul", |b| {
        b.iter(|| black_box(serde_json::to_string(black_box(&reponse)).unwrap()))
    });

    c.bench_function("catalogue/models_complet", |b| {
        b.iter(|| {
            let scan = scan_models(black_box(dir.path()));
            let reponse = ModelsResponse::from_scan(scan, &config, &presets);
            black_box(serde_json::to_string(&reponse).unwrap())
        })
    });
}

criterion_group!(bancs, scan, json);
criterion_main!(bancs);
