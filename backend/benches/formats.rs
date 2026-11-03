//! Bancs des aperçus : rasterisation, encodage PNG, génération complète.
//!
//!     cargo bench --bench formats
//!
//! Un aperçu, c'est quatre étapes : **parser** le fichier, **rasteriser** le
//! maillage, **encoder** le PNG, **écrire** sur disque. Les mesurer ensemble ne
//! dit pas laquelle a régressé — et l'encodage pèse souvent plus que le rendu.
//! D'où trois groupes : `rendu` (2ᵉ étape seule), `png` (3ᵉ seule), `apercu`
//! (les quatre, comme en production).
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use easy3d::formats::{self, Mesh};
use std::fs;
use std::hint::black_box;
use std::path::Path;

/// Maillage de test : grille `n` × `n` (donc `2n²` triangles) bombée au centre.
///
/// Un maillage **plein** plutôt qu'un cube : c'est le nombre de triangles qui
/// pilote le coût du rasteriseur, et un modèle réel en compte des dizaines de
/// milliers.
fn grille(n: u32) -> Mesh {
    let mut vertices = Vec::with_capacity(((n + 1) * (n + 1)) as usize);
    for y in 0..=n {
        for x in 0..=n {
            let fx = x as f32 / n as f32 * 2.0 - 1.0;
            let fy = y as f32 / n as f32 * 2.0 - 1.0;
            // Bosse douce : assez de relief pour que l'ombrage varie.
            let rayon = fx * fx + fy * fy;
            let fz = 0.4 * (1.0 - rayon.min(1.0)).sqrt();
            vertices.push([fx, fy, fz]);
        }
    }

    let mut triangles = Vec::with_capacity((2 * n * n) as usize);
    let ligne = n + 1;
    for y in 0..n {
        for x in 0..n {
            let i = y * ligne + x;
            triangles.push([i, i + ligne, i + 1]);
            triangles.push([i + 1, i + ligne, i + ligne + 1]);
        }
    }

    Mesh {
        vertices,
        triangles,
    }
}

/// STL **binaire** : en-tête de 80 octets, compte, puis 50 octets par triangle
/// (normale, 3 sommets, attribut).
fn ecrire_stl_binaire(path: &Path, mesh: &Mesh) {
    let mut out = Vec::with_capacity(84 + mesh.triangles.len() * 50);
    out.extend_from_slice(&[0u8; 80]);
    out.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
    for triangle in &mesh.triangles {
        // Normale à zéro : les lecteurs la recalculent depuis les sommets.
        out.extend_from_slice(&[0u8; 12]);
        for i in triangle {
            for c in mesh.vertices[*i as usize] {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    fs::write(path, out).unwrap();
}

/// STL **ASCII** : le même maillage en texte (deux à trois fois plus gros, et
/// une analyse syntaxique au lieu d'une lecture binaire).
fn ecrire_stl_ascii(path: &Path, mesh: &Mesh) {
    let mut text = String::from("solid banc\n");
    for triangle in &mesh.triangles {
        text.push_str("  facet normal 0 0 1\n    outer loop\n");
        for i in triangle {
            let v = mesh.vertices[*i as usize];
            text.push_str(&format!("      vertex {} {} {}\n", v[0], v[1], v[2]));
        }
        text.push_str("    endloop\n  endfacet\n");
    }
    text.push_str("endsolid banc\n");
    fs::write(path, text).unwrap();
}

/// OBJ : sommets puis faces (indices **1-based**).
fn ecrire_obj(path: &Path, mesh: &Mesh) {
    let mut text = String::new();
    for v in &mesh.vertices {
        text.push_str(&format!("v {} {} {}\n", v[0], v[1], v[2]));
    }
    for triangle in &mesh.triangles {
        text.push_str(&format!(
            "f {} {} {}\n",
            triangle[0] + 1,
            triangle[1] + 1,
            triangle[2] + 1
        ));
    }
    fs::write(path, text).unwrap();
}

/// Coût du seul rasteriseur, en fonction du nombre de triangles.
fn rendu(c: &mut Criterion) {
    let mut group = c.benchmark_group("formats/rendu");
    for n in [32u32, 100, 224] {
        let mesh = grille(n);
        let triangles = mesh.triangles.len();
        group.throughput(Throughput::Elements(triangles as u64));
        group.bench_with_input(BenchmarkId::from_parameter(triangles), &mesh, |b, mesh| {
            b.iter(|| black_box(formats::render(black_box(mesh))))
        });
    }
    group.finish();
}

/// Coût de l'encodage PNG seul (l'image est produite une fois, hors mesure).
fn encodage_png(c: &mut Criterion) {
    let image = formats::render(&grille(100));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("apercu.png");

    c.bench_function("formats/encodage_png", |b| {
        b.iter(|| image.save(black_box(&out)).unwrap());
    });
}

/// Aperçu complet, par format : parse + rendu + PNG + écriture.
fn apercu_complet(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let mesh = grille(100);

    let stl_binaire = dir.path().join("grille.stl");
    ecrire_stl_binaire(&stl_binaire, &mesh);
    let stl_ascii = dir.path().join("grille_ascii.stl");
    ecrire_stl_ascii(&stl_ascii, &mesh);
    let obj = dir.path().join("grille.obj");
    ecrire_obj(&obj, &mesh);

    let out = dir.path().join("apercu.png");
    for (nom, path) in [
        ("stl_binaire", &stl_binaire),
        ("stl_ascii", &stl_ascii),
        ("obj", &obj),
    ] {
        c.bench_function(&format!("formats/apercu_{nom}"), |b| {
            b.iter(|| formats::make_thumbnail(black_box(path), black_box(&out)).unwrap());
        });
    }
}

criterion_group!(bancs, rendu, encodage_png, apercu_complet);
criterion_main!(bancs);
