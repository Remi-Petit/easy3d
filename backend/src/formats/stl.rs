//! Format STL (binaire et ASCII).
//!
//! L'aperçu rasterise la géométrie (`mesh`) : les STL édités par les slicers
//! sont Z-up, d'où la rotation de rattrapage en Y-up.

use super::Format;
use super::mesh::{self, Mesh};
use std::fs;
use std::path::Path;

/// Instance unique du format, référencée par le registre de [`super`].
pub static STL: Stl = Stl;

/// Le format STL.
pub struct Stl;

impl Format for Stl {
    fn name(&self) -> &'static str {
        "STL"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["stl"]
    }

    fn content_type(&self) -> &'static str {
        "model/stl"
    }

    fn has_preview(&self) -> bool {
        true
    }

    fn thumbnail(&self, path: &Path, out: &Path) -> Option<()> {
        mesh::write(&load(path)?, out, true)
    }
}

/// Charge un STL (binaire ou ASCII) en `Mesh`.
fn load(path: &Path) -> Option<Mesh> {
    let bytes = fs::read(path).ok()?;
    // STL binaire = en-tête de 84 octets + N triangles de 50 octets.
    if bytes.len() >= 84 && (bytes.len() - 84) % 50 == 0 {
        load_binary(&bytes)
    } else {
        load_ascii(&bytes)
    }
}

fn load_binary(data: &[u8]) -> Option<Mesh> {
    if data.len() < 84 {
        return None;
    }
    let n = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for i in 0..n {
        let base = 84 + i * 50;
        if base + 48 > data.len() {
            break;
        }
        let v = |off: usize| -> [f32; 3] {
            let o = base + off;
            let f = |k: usize| {
                let s = o + k * 4;
                f32::from_le_bytes([data[s], data[s + 1], data[s + 2], data[s + 3]])
            };
            [f(0), f(1), f(2)]
        };
        let a = vertices.len() as u32;
        vertices.push(v(12));
        vertices.push(v(24));
        vertices.push(v(36));
        triangles.push([a, a + 1, a + 2]);
    }
    Some(Mesh {
        vertices,
        triangles,
    })
}

fn load_ascii(data: &[u8]) -> Option<Mesh> {
    let text = std::str::from_utf8(data).ok()?;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut group: Vec<[f32; 3]> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("vertex") {
            let mut it = rest.split_whitespace();
            let x: f32 = it.next()?.parse().ok()?;
            let y: f32 = it.next()?.parse().ok()?;
            let z: f32 = it.next()?.parse().ok()?;
            group.push([x, y, z]);
            if group.len() == 3 {
                let a = vertices.len() as u32;
                vertices.extend_from_slice(&group);
                triangles.push([a, a + 1, a + 2]);
                group.clear();
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
