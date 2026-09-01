//! Génération d'images d'aperçu (thumbnails) **côté backend**.
//!
//! Pour chaque modèle (STL / OBJ / 3MF) détecté dans `models/`, on produit un
//! PNG placé dans `<models_root>/.easy3d-thumbs/`. Cette image sert d'aperçu
//! sur le site — plus léger qu'un rendu 3D navigateur pour les grandes grilles.
//!
//! Stratégies :
//! - **3MF** : extraction de la vignette embarquée (`Metadata/thumbnail.png`,
//!   `Metadata/plate_N.png` ou `thumbnail.png`) ; sinon on rasterise la
//!   géométrie.
//! - **STL / OBJ** : rasteriseur logiciel (CPU, sans GPU) avec éclairage
//!   lambertien et z-buffer.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Nom du dossier caché (dans `models/`) qui regroupe les aperçus générés.
pub const THUMB_DIR: &str = ".easy3d-thumbs";

/// Dimensions par défaut des aperçus générés.
const THUMB_W: usize = 480;
const THUMB_H: usize = 366;

/// Couleur de fond (assortie au thème sombre du frontend).
const BG: [u8; 3] = [0x16, 0x16, 0x25];

/// Couleur de base des faces (bleu cyan, comme le rendu 3D du frontend).
const BASE: [f32; 3] = [0.70, 0.82, 0.96];

/// Direction de la lumière (normalisée à la volée).
const LIGHT: [f32; 3] = [0.45, 0.85, 0.70];

// ─────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────

/// Maillage minimal : liste de sommets + triangles (triplets d'indices).
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    fn empty() -> Self {
        Mesh { vertices: Vec::new(), triangles: Vec::new() }
    }

    fn is_empty(&self) -> bool {
        self.triangles.is_empty() || self.vertices.is_empty()
    }

    /// Applique une rotation 90° autour de l'axe X pour rendre le modèle Y-up.
    /// (Les STL édités par les slicers sont souvent Z-up.)
    fn rotate_x_minus90(&mut self) {
        for v in self.vertices.iter_mut() {
            let (y, z) = (v[1], v[2]);
            v[1] = z;
            v[2] = -y;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// API publique
// ─────────────────────────────────────────────────────────────────────────

/// Chemin (relatif à la racine des modèles) de l'aperçu d'un modèle donné.
///
/// `models/chainsaw-man/Chainsaw_Man.stl` → `.easy3d-thumbs/chainsaw-man/Chainsaw_Man.png`
pub fn thumb_rel_path(model_rel: &str) -> String {
    let stem = Path::new(model_rel).with_extension("png");
    format!("{THUMB_DIR}/{}", stem.display())
}

/// Génère (ou réutilise) l'aperçu d'un modèle.
///
/// - `model_path` : chemin **absolu** du fichier de modèle.
/// - `model_rel`  : chemin du modèle relatif à la racine (`folder/file.stl`).
/// - `thumbs_root`: dossier de sortie des aperçus (`<root>/.easy3d-thumbs`).
///
/// Retourne le chemin **absolu** de l'image générée, ou `None` en cas d'échec.
/// Ne régénère pas un aperçu déjà à jour (plus récent que le modèle).
pub fn ensure_thumbnail(
    model_path: &Path,
    model_rel: &str,
    thumbs_root: &Path,
) -> Option<PathBuf> {
    let out_abs = thumbs_root.join(Path::new(model_rel).with_extension("png"));
    // Assure l'existence du sous-dossier (ex : `.easy3d-thumbs/chainsaw-man/`).
    if let Some(parent) = out_abs.parent() {
        fs::create_dir_all(parent).ok();
    }
    if is_fresh(&out_abs, model_path) {
        return Some(out_abs);
    }

    let ext = model_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let generated = match ext.as_str() {
        "3mf" => try_extract_3mf_thumbnail(model_path, &out_abs).or_else(|| {
            load_3mf_mesh(model_path).and_then(|m| write_model(&m, &out_abs, true))
        }),
        "stl" => load_stl(model_path).and_then(|m| write_model(&m, &out_abs, true)),
        "obj" => load_obj(model_path).and_then(|m| write_model(&m, &out_abs, false)),
        _ => None,
    };

    generated.and(Some(out_abs))
}

/// Boucle le dossier des modèles et garantit un aperçu pour chaque modèle.
///
/// `root` est la **racine des modèles** : elle sert de base pour calculer les
/// chemins relatifs, afin que les aperçus respectent la structure des
/// sous-dossiers (`.easy3d-thumbs/chainsaw-man/x.png`).
pub fn generate_all(root: &Path, thumbs_root: &Path) {
    generate_dir(root, root, thumbs_root);
}

/// Parcourt récursivement `dir` en gardant `base` comme référence des rel.
fn generate_dir(base: &Path, dir: &Path, thumbs_root: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    fs::create_dir_all(thumbs_root).ok();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Ne respecte pas les dossiers cachés (comme .easy3d-thumbs).
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            generate_dir(base, &path, thumbs_root);
        } else if let Some(rel) = rel_of(base, &path) {
            ensure_thumbnail(&path, &rel, thumbs_root);
        }
    }
}

/// Re-génère l'aperçu d'un modèle précis (appelé par le watcher sur un chemin).
pub fn ensure_for_changed(root: &Path, thumbs_root: &Path, abs_path: &Path) {
    let rel = match rel_of(root, abs_path) {
        Some(r) => r,
        None => return,
    };
    let ext = abs_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "stl" | "obj" | "3mf") {
        ensure_thumbnail(abs_path, &rel, thumbs_root);
    }
}

/// Supprime l'aperçu mis en cache d'un modèle (appelé par le watcher lorsqu'un
/// fichier est supprimé). Aucun effet si aucun aperçu n'existe.
pub fn remove_for_path(root: &Path, thumbs_root: &Path, abs_path: &Path) {
    let rel = match rel_of(root, abs_path) {
        Some(r) => r,
        None => return,
    };
    let thumb = thumbs_root.join(thumb_rel_path(&rel));
    let _ = fs::remove_file(&thumb);
}

// ─────────────────────────────────────────────────────────────────────────
// Utilitaires de chemin
// ─────────────────────────────────────────────────────────────────────────

/// Chemin relatif de `path` par rapport à `root` (séparateurs `/`).
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// `true` si l'aperçu existe et date d'après le modèle (donc à jour).
fn is_fresh(out: &Path, model: &Path) -> bool {
    match (fs::metadata(out), fs::metadata(model)) {
        (Ok(om), Ok(mm)) => om.modified().ok() >= mm.modified().ok(),
        _ => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Extraction de la vignette embarquée (3MF = archive ZIP)
// ─────────────────────────────────────────────────────────────────────────

/// Cherche une vignette PNG/JPEG dans l'archive 3MF et l'écrit dans `out`.
fn try_extract_3mf_thumbnail(model_path: &Path, out: &Path) -> Option<()> {
    fs::create_dir_all(out.parent()?).ok()?;

    let file = fs::File::open(model_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Nom de l'entrée contenant la vignette, en minuscules.
    let mut target: Option<String> = None;
    for i in 0..archive.len() {
        let name = archive.name_for_index(i).unwrap_or("").to_ascii_lowercase();
        if name == "metadata/thumbnail.png"
            || name == "thumbnail.png"
            || name.starts_with("metadata/plate_")
        {
            target = Some(archive.name_for_index(i).unwrap().to_string());
            break;
        }
    }

    let target = target?;
    let mut entry = archive.by_name(&target).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;

    // Décode (PNG/JPEG), redimensionne et ré-encode en PNG pour homogénéité.
    let img = image::load_from_memory(&bytes).ok()?;
    if img.width() > 512 || img.height() > 512 {
        img.thumbnail(img.width().min(512).max(1), img.height().min(512).max(1))
            .to_rgba8()
            .save(out)
            .ok()?;
    } else {
        img.to_rgba8().save(out).ok()?;
    }
    Some(())
}

// ─────────────────────────────────────────────────────────────────────────
// Chargeurs de maillage
// ─────────────────────────────────────────────────────────────────────────

/// Charge un STL (binaire ou ASCII) en `Mesh`.
fn load_stl(path: &Path) -> Option<Mesh> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() >= 84 && (bytes.len() - 84) % 50 == 0 {
        load_stl_binary(&bytes)
    } else {
        load_stl_ascii(&bytes)
    }
}

fn load_stl_binary(data: &[u8]) -> Option<Mesh> {
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
    Some(Mesh { vertices, triangles })
}

fn load_stl_ascii(data: &[u8]) -> Option<Mesh> {
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
        Some(Mesh { vertices, triangles })
    }
}

/// Charge un OBJ (subset suffisant : lignes `v` et `f`).
fn load_obj(path: &Path) -> Option<Mesh> {
    let text = fs::read_to_string(path).ok()?;
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("v ") {
            let mut it = line[2..].split_whitespace();
            let x: f32 = it.next()?.parse().ok()?;
            let y: f32 = it.next()?.parse().ok()?;
            let z: f32 = it.next()?.parse().ok()?;
            vertices.push([x, y, z]);
        } else if line.starts_with("f ") {
            let idxs: Vec<u32> = line[2..]
                .split_whitespace()
                .filter_map(|tok| {
                    let raw = tok.split('/').next()?;
                    let mut i: i64 = raw.parse().ok()?;
                    if i < 0 {
                        i += vertices.len() as i64 + 1;
                    }
                    if i > 0 {
                        Some((i - 1) as u32)
                    } else {
                        None
                    }
                })
                .collect();
            for k in 1..idxs.len().saturating_sub(1) {
                triangles.push([idxs[0], idxs[k], idxs[k + 1]]);
            }
        }
    }
    if triangles.is_empty() {
        None
    } else {
        Some(Mesh { vertices, triangles })
    }
}

/// Charge la géométrie d'un 3MF (concatène les objets du `<build>`).
fn load_3mf_mesh(path: &Path) -> Option<Mesh> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Trouve l'entrée du modèle 3D (ex : `3D/3dmodel.model`).
    let model_name = (0..archive.len())
        .map(|i| archive.name_for_index(i).unwrap_or("").to_string())
        .find(|n| n.to_ascii_lowercase().ends_with(".model"))?;

    let mut xml = String::new();
    archive.by_name(&model_name).ok()?.read_to_string(&mut xml).ok()?;

    let doc = roxmltree::Document::parse(&xml).ok()?;
    let mut mesh = Mesh::empty();

    // Parcourt tous les `<object>` du document (ordre d'apparition) et concatène
    // leur `<mesh>` (les indices de chaque objet sont relatifs à ses propres
    // sommets, d'où un décalage par objet). On ignore les entrées malformées.
    for object in doc.descendants().filter(|n| n.has_tag_name("object")) {
        let base = mesh.vertices.len() as u32;
        if let Some(vertices_node) = object.descendants().find(|n| n.has_tag_name("vertices")) {
            for v in vertices_node.children().filter(|n| n.has_tag_name("vertex")) {
                let Some(x) = v.attribute("x").and_then(|a| a.parse::<f32>().ok()) else { continue };
                let Some(y) = v.attribute("y").and_then(|a| a.parse::<f32>().ok()) else { continue };
                let Some(z) = v.attribute("z").and_then(|a| a.parse::<f32>().ok()) else { continue };
                mesh.vertices.push([x, y, z]);
            }
        }
        if let Some(triangles_node) = object.descendants().find(|n| n.has_tag_name("triangles")) {
            for t in triangles_node.children().filter(|n| n.has_tag_name("triangle")) {
                let Some(a) = t.attribute("v1").and_then(|a| a.parse::<u32>().ok()) else { continue };
                let Some(b) = t.attribute("v2").and_then(|a| a.parse::<u32>().ok()) else { continue };
                let Some(c) = t.attribute("v3").and_then(|a| a.parse::<u32>().ok()) else { continue };
                mesh.triangles.push([a + base, b + base, c + base]);
            }
        }
    }

    if mesh.is_empty() {
        None
    } else {
        Some(mesh)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Rasteriseur logiciel
// ─────────────────────────────────────────────────────────────────────────

/// Rend `mesh` vers `<out>.png` (aperçu CPU, écran non requis).
/// `rotate_x` : si vrai, recale le modèle en Y-up avant rendu (STL / 3MF).
fn write_model(mesh: &Mesh, out: &Path, rotate_x: bool) -> Option<()> {
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
    let center = [(minx + maxx) / 2.0, (miny + maxy) / 2.0, (minz + maxz) / 2.0];

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
        raster_triangle(
            &mut buf,
            &mut zbuf,
            w as usize,
            h as usize,
            s0,
            s1,
            s2,
            col,
        );
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
