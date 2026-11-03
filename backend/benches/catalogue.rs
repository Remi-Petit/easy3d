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

/// Agrégation par sous-dossier : filtre par préfixe (ancienne façon, `O(dossiers
/// × fichiers)`) contre un passage par les ancêtres (`O(fichiers × profondeur)`).
///
/// Le calcul se fait en mémoire, sur la liste déjà collectée par le scan : il
/// n'y a aucun appel système à mesurer ici, seulement la complexité. C'est
/// précisément ce qui n'apparaissait pas sur un petit catalogue — et qui
/// dominait sur un gros.
fn sous_dossiers(c: &mut Criterion) {
    use easy3d::scanner::FileInfo;
    use std::collections::HashMap;

    let dossiers = 40;
    let par_dossier = 25;
    let files: Vec<FileInfo> = (0..dossiers)
        .flat_map(|d| {
            (0..par_dossier).map(move |f| FileInfo {
                path: format!("/models/Dossier {d}/piece {f}.stl"),
                rel: format!("Dossier {d}/piece {f}.stl"),
                created: None,
                modified: Some(1_700_000_000),
                image: None,
                image_version: None,
                note: None,
            })
        })
        .collect();
    let rels: Vec<String> = (0..dossiers).map(|d| format!("Dossier {d}")).collect();

    let mut group = c.benchmark_group("catalogue/sous_dossiers");
    group.throughput(Throughput::Elements((dossiers * par_dossier) as u64));

    group.bench_function("filtre_par_prefixe", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            for rel in &rels {
                let prefix = format!("{rel}/");
                out.push(
                    black_box(&files)
                        .iter()
                        .filter(|f| f.rel.starts_with(&prefix))
                        .count(),
                );
            }
            black_box(out)
        })
    });

    group.bench_function("passage_par_ancetres", |b| {
        b.iter(|| {
            let mut totals: HashMap<String, (usize, Option<u64>)> = HashMap::new();
            for file in black_box(&files) {
                let mut parent = file.rel.as_str();
                while let Some((up, _)) = parent.rsplit_once('/') {
                    let entry = totals.entry(up.to_string()).or_insert((0, None));
                    entry.0 += 1;
                    if let Some(modified) = file.modified {
                        entry.1 = Some(entry.1.map_or(modified, |cur| cur.max(modified)));
                    }
                    parent = up;
                }
            }
            black_box(totals)
        })
    });

    group.finish();
}

criterion_group!(bancs, scan, json, sous_dossiers);
criterion_main!(bancs);
