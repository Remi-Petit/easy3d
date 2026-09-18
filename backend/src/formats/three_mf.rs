//! Format 3MF : archive ZIP contenant un modèle XML (`3D/3dmodel.model`).
//!
//! Stratégie d'aperçu, dans cet ordre :
//! 1. extraire la **vignette embarquée** (`Metadata/thumbnail.png`,
//!    `Metadata/plate_N.png` ou `thumbnail.png`) ;
//! 2. à défaut, rasteriser la géométrie des `<object>` du document.

use super::Format;
use super::Viewer;
use super::mesh::{self, Mesh};
use std::fs;
use std::io::Read;
use std::path::Path;

/// Instance unique du format, référencée par le registre de [`super`].
pub static THREE_MF: ThreeMf = ThreeMf;

/// Le format 3MF.
pub struct ThreeMf;

impl Format for ThreeMf {
    fn name(&self) -> &'static str {
        "3MF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["3mf"]
    }

    fn content_type(&self) -> &'static str {
        "model/3mf"
    }

    fn has_preview(&self) -> bool {
        true
    }

    fn viewer(&self) -> Viewer {
        Viewer::Mesh
    }

    fn thumbnail(&self, path: &Path, out: &Path) -> Option<()> {
        try_extract_thumbnail(path, out)
            // Le 3MF est Z-up (comme le STL) : rotation de rattrapage en Y-up.
            .or_else(|| load_mesh(path).and_then(|m| mesh::write(&m, out, true)))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Vignette embarquée
// ─────────────────────────────────────────────────────────────────────────

/// Cherche une vignette PNG/JPEG dans l'archive 3MF et l'écrit dans `out`.
fn try_extract_thumbnail(model_path: &Path, out: &Path) -> Option<()> {
    fs::create_dir_all(out.parent()?).ok()?;

    let file = fs::File::open(model_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Nom de l'entrée contenant la vignette (recherche insensible à la casse).
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
    //
    // Puis mise au style de la maison : ces vignettes sont des **rendus de
    // plateau** (modèle coloré, plateau et grille neutres) qui jurent à côté des
    // rendus de maillage. Le décor — fond, plateau, grille — prend la couleur de
    // l'application, le modèle la matière claire. Sans modèle identifiable
    // (filament blanc ou gris), `restyle` laisse l'image telle quelle.
    let img = image::load_from_memory(&bytes).ok()?;
    let scaled = if img.width() > 512 || img.height() > 512 {
        img.thumbnail(img.width().clamp(1, 512), img.height().clamp(1, 512))
    } else {
        img
    };
    let mut styled = scaled.to_rgba8();
    super::thumb_style::restyle(&mut styled);
    styled.save(out).ok()?;

    Some(())
}

// ─────────────────────────────────────────────────────────────────────────
// Géométrie
// ─────────────────────────────────────────────────────────────────────────

/// Charge la géométrie d'un 3MF (concatène les objets du `<build>`).
fn load_mesh(path: &Path) -> Option<Mesh> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Trouve l'entrée du modèle 3D (ex : `3D/3dmodel.model`).
    let model_name = (0..archive.len())
        .map(|i| archive.name_for_index(i).unwrap_or("").to_string())
        .find(|n| n.to_ascii_lowercase().ends_with(".model"))?;

    let mut xml = String::new();
    archive
        .by_name(&model_name)
        .ok()?
        .read_to_string(&mut xml)
        .ok()?;

    let doc = roxmltree::Document::parse(&xml).ok()?;
    let mut mesh = Mesh::empty();

    // Parcourt tous les `<object>` du document (ordre d'apparition) et concatène
    // leur `<mesh>` (les indices de chaque objet sont relatifs à ses propres
    // sommets, d'où un décalage par objet). On ignore les entrées malformées.
    for object in doc.descendants().filter(|n| n.has_tag_name("object")) {
        let base = mesh.vertices.len() as u32;
        if let Some(vertices_node) = object.descendants().find(|n| n.has_tag_name("vertices")) {
            for v in vertices_node
                .children()
                .filter(|n| n.has_tag_name("vertex"))
            {
                let Some(x) = v.attribute("x").and_then(|a| a.parse::<f32>().ok()) else {
                    continue;
                };
                let Some(y) = v.attribute("y").and_then(|a| a.parse::<f32>().ok()) else {
                    continue;
                };
                let Some(z) = v.attribute("z").and_then(|a| a.parse::<f32>().ok()) else {
                    continue;
                };
                mesh.vertices.push([x, y, z]);
            }
        }
        if let Some(triangles_node) = object.descendants().find(|n| n.has_tag_name("triangles")) {
            for t in triangles_node
                .children()
                .filter(|n| n.has_tag_name("triangle"))
            {
                let Some(a) = t.attribute("v1").and_then(|a| a.parse::<u32>().ok()) else {
                    continue;
                };
                let Some(b) = t.attribute("v2").and_then(|a| a.parse::<u32>().ok()) else {
                    continue;
                };
                let Some(c) = t.attribute("v3").and_then(|a| a.parse::<u32>().ok()) else {
                    continue;
                };
                mesh.triangles.push([a + base, b + base, c + base]);
            }
        }
    }

    if mesh.is_empty() { None } else { Some(mesh) }
}
