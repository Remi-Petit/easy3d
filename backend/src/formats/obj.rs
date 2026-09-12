//! Format Wavefront OBJ.
//!
//! Subset volontairement minimal : seules les lignes `v` (sommets) et `f`
//! (faces) sont lues — suffisant pour un aperçu.

use super::Format;
use super::Viewer;
use super::mesh::{self, Mesh};
use std::fs;
use std::path::Path;

/// Instance unique du format, référencée par le registre de [`super`].
pub static OBJ: Obj = Obj;

/// Le format OBJ.
pub struct Obj;

impl Format for Obj {
    fn name(&self) -> &'static str {
        "OBJ"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }

    fn content_type(&self) -> &'static str {
        "model/obj"
    }

    fn has_preview(&self) -> bool {
        true
    }

    fn viewer(&self) -> Viewer {
        Viewer::Mesh
    }

    fn thumbnail(&self, path: &Path, out: &Path) -> Option<()> {
        // L'OBJ est déjà Y-up : aucune rotation de rattrapage (contrairement
        // au STL / 3MF, Z-up comme les slicers).
        mesh::write(&load(path)?, out, false)
    }
}

/// Charge un OBJ (lignes `v` et `f`).
fn load(path: &Path) -> Option<Mesh> {
    let text = fs::read_to_string(path).ok()?;
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("v ") {
            let mut it = rest.split_whitespace();
            let x: f32 = it.next()?.parse().ok()?;
            let y: f32 = it.next()?.parse().ok()?;
            let z: f32 = it.next()?.parse().ok()?;
            vertices.push([x, y, z]);
        } else if let Some(rest) = line.strip_prefix("f ") {
            let idxs: Vec<u32> = rest
                .split_whitespace()
                .filter_map(|tok| {
                    let raw = tok.split('/').next()?;
                    let mut i: i64 = raw.parse().ok()?;
                    // Indice négatif = relatif à la fin de la liste des sommets.
                    if i < 0 {
                        i += vertices.len() as i64 + 1;
                    }
                    if i > 0 { Some((i - 1) as u32) } else { None }
                })
                .collect();
            // Éventail de triangles (les faces OBJ peuvent être des polygones).
            for k in 1..idxs.len().saturating_sub(1) {
                triangles.push([idxs[0], idxs[k], idxs[k + 1]]);
            }
        }
    }
    if triangles.is_empty() {
        None
    } else {
        Some(Mesh {
            vertices,
            triangles,
        })
    }
}
