//! Maillage minimal et rasteriseur logiciel, **partagés** par les formats
//! maillés (STL, OBJ, 3MF).
//!
//! Rendu CPU (aucun GPU, aucun écran requis) : éclairage lambertien, z-buffer,
//! couleur de fond assortie au thème sombre du frontend. Plus léger à afficher
//! qu'un rendu 3D navigateur pour de grandes grilles.

use super::{THUMB_BASE, THUMB_BG, THUMB_H, THUMB_W};
use std::fs;
use std::path::Path;

/// Couleur de fond (assortie au thème sombre du frontend).
const BG: [u8; 3] = THUMB_BG;

/// Couleur de base des faces (bleu cyan, comme le rendu 3D du frontend).
const BASE: [f32; 3] = THUMB_BASE;

/// Direction de la lumière (normalisée à la volée).
const LIGHT: [f32; 3] = [0.45, 0.85, 0.70];

/// Maillage minimal : liste de sommets + triangles (triplets d'indices).
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// Maillage vide.
    pub fn empty() -> Self {
        Mesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        }
    }

    /// `true` si le maillage ne contient aucun triangle exploitable.
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty() || self.vertices.is_empty()
    }

    /// Applique une rotation 90° autour de l'axe X pour rendre le modèle Y-up.
    /// (Les STL édités par les slicers sont souvent Z-up.)
    pub fn rotate_x_minus90(&mut self) {
        for v in self.vertices.iter_mut() {
            let (y, z) = (v[1], v[2]);
            v[1] = z;
            v[2] = -y;
        }
    }
}

/// Rend `mesh` vers `<out>.png` (aperçu CPU, écran non requis).
///
/// `rotate_x` : si vrai, recale le modèle en Y-up avant rendu. Vrai pour les
/// formats produits par les slicers (STL, 3MF), faux pour l'OBJ (déjà Y-up).
pub fn write(mesh: &Mesh, out: &Path, rotate_x: bool) -> Option<()> {
    fs::create_dir_all(out.parent()?).ok()?;
    let mut mesh = mesh.clone();
    if rotate_x {
        mesh.rotate_x_minus90();
    }
    render_mesh(&mesh).save(out).ok()?;
    Some(())
}

fn render_mesh(mesh: &Mesh) -> image::RgbaImage {
    let (w, h) = (THUMB_W as u32, THUMB_H as u32);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let mut zbuf = vec![f32::INFINITY; (w * h) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            buf[i] = BG[0];
            buf[i + 1] = BG[1];
            buf[i + 2] = BG[2];
            buf[i + 3] = 255;
        }
    }

    if mesh.is_empty() {
        return image::RgbaImage::from_raw(w, h, buf).unwrap();
    }

    // Centre de la boîte englobante.
    let (mut minx, mut maxx) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut miny, mut maxy) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut minz, mut maxz) = (f32::INFINITY, f32::NEG_INFINITY);
    for v in &mesh.vertices {
        minx = minx.min(v[0]);
        maxx = maxx.max(v[0]);
        miny = miny.min(v[1]);
        maxy = maxy.max(v[1]);
        minz = minz.min(v[2]);
        maxz = maxz.max(v[2]);
    }
    let center = [
        (minx + maxx) / 2.0,
        (miny + maxy) / 2.0,
        (minz + maxz) / 2.0,
    ];

    // Base caméra orthonormée (vue 3/4 depuis le haut-droit).
    let cam_dir = norm([1.0, 0.75, 1.15]);
    let up = [0.0, 1.0, 0.0];
    let forward = [-cam_dir[0], -cam_dir[1], -cam_dir[2]]; // de l'œil vers le centre
    let right = norm(cross(forward, up));
    let true_up = cross(right, forward);

    // Coordonnées caméra (cx, cy, cz) pour chaque sommet.
    let cam: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .map(|v| {
            let p = [v[0] - center[0], v[1] - center[1], v[2] - center[2]];
            [dot(p, right), dot(p, true_up), dot(p, forward)]
        })
        .collect();

    // Boîte englobante en vue caméra (pour dimensionner l'échelle).
    let (mut cminx, mut cmaxx) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut cminy, mut cmaxy) = (f32::INFINITY, f32::NEG_INFINITY);
    for c in &cam {
        cminx = cminx.min(c[0]);
        cmaxx = cmaxx.max(c[0]);
        cminy = cminy.min(c[1]);
        cmaxy = cmaxy.max(c[1]);
    }
    let span_x = (cmaxx - cminx).max(1e-9);
    let span_y = (cmaxy - cminy).max(1e-9);
    let scale = 0.82 * (w as f32 / span_x).min(h as f32 / span_y);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);

    // Sommets écran.
    let screen: Vec<[f32; 3]> = cam
        .iter()
        .map(|c| [cx + c[0] * scale, cy - c[1] * scale, c[2]])
        .collect();

    let light = norm(LIGHT);

    // Rasterisation de chaque triangle.
    for tri in &mesh.triangles {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= cam.len() || i1 >= cam.len() || i2 >= cam.len() {
            continue;
        }
        let s0 = screen[i0];
        let s1 = screen[i1];
        let s2 = screen[i2];

        // Normale de la face (dans l'espace modèle, centré).
        let v0 = [
            mesh.vertices[i0][0] - center[0],
            mesh.vertices[i0][1] - center[1],
            mesh.vertices[i0][2] - center[2],
        ];
        let v1 = [
            mesh.vertices[i1][0] - center[0],
            mesh.vertices[i1][1] - center[1],
            mesh.vertices[i1][2] - center[2],
        ];
        let v2 = [
            mesh.vertices[i2][0] - center[0],
            mesh.vertices[i2][1] - center[1],
            mesh.vertices[i2][2] - center[2],
        ];
        let n = cross(sub(v1, v0), sub(v2, v0));
        let nn = norm(n);
        // Face arrière : on retire si elle pointe à l'opposé de la caméra.
        if dot(n, forward) >= 0.0 {
            continue;
        }
        let intensity = 0.35 + 0.85 * dot(nn, light).max(0.0);
        let col = [
            (BASE[0] * intensity).min(1.0),
            (BASE[1] * intensity).min(1.0),
            (BASE[2] * intensity).min(1.0),
        ];
        raster_triangle(&mut buf, &mut zbuf, w as usize, h as usize, s0, s1, s2, col);
    }

    image::RgbaImage::from_raw(w, h, buf).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn raster_triangle(
    buf: &mut [u8],
    zbuf: &mut [f32],
    w: usize,
    h: usize,
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    col: [f32; 3],
) {
    let area = edge(a, b, c);
    if area.abs() < 1e-9 {
        return;
    }
    let minx = (0.0f32.max(a[0].min(b[0]).min(c[0]).floor())) as i32;
    let maxx = ((w as f32 - 1.0).min(a[0].max(b[0]).max(c[0]).ceil())) as i32;
    let miny = (0.0f32.max(a[1].min(b[1]).min(c[1]).floor())) as i32;
    let maxy = ((h as f32 - 1.0).min(a[1].max(b[1]).max(c[1]).ceil())) as i32;

    let inv_area = 1.0 / area;
    let (r, g, bcol) = (
        (col[0] * 255.0) as u8,
        (col[1] * 255.0) as u8,
        (col[2] * 255.0) as u8,
    );

    let mut y = miny;
    while y <= maxy {
        let mut x = minx;
        while x <= maxx {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(b, c, [px, py, 0.0]) * inv_area;
            let w1 = edge(c, a, [px, py, 0.0]) * inv_area;
            let w2 = edge(a, b, [px, py, 0.0]) * inv_area;
            if w0 >= -1e-6 && w1 >= -1e-6 && w2 >= -1e-6 {
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let idx = y as usize * w + x as usize;
                if z < zbuf[idx] {
                    zbuf[idx] = z;
                    let o = idx * 4;
                    buf[o] = r;
                    buf[o + 1] = g;
                    buf[o + 2] = bcol;
                    buf[o + 3] = 255;
                }
            }
            x += 1;
        }
        y += 1;
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Petites aides vectorielles
// ─────────────────────────────────────────────────────────────────────────

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

fn edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
