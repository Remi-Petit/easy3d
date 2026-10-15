//! Format 3MF : archive ZIP contenant un modèle XML (`3D/3dmodel.model`).
//!
//! Stratégie d'aperçu, dans cet ordre :
//! 1. extraire la **vignette embarquée** (`Metadata/thumbnail.png`,
//!    `Metadata/plate_N.png` ou `thumbnail.png`) ;
//! 2. à défaut, rasteriser la géométrie des `<object>` du document.

use super::Format;
use super::Viewer;
use super::mesh::{self, Mesh};
use super::print_info::{self, PrintInfo};
use std::collections::BTreeMap;
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

    fn print_info(&self, _model_rel: &str, path: &Path) -> PrintInfo {
        let mut info = PrintInfo::from_metadata(&metadata_of(path));
        info.bytes = fs::metadata(path).ok().map(|m| m.len());
        info
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

// ─────────────────────────────────────────────────────────────
// Profil du slicer
// ─────────────────────────────────────────────────────────────

/// Profil de slicer embarqué dans un 3MF de **projet**.
///
/// Deux familles, deux écritures :
///
/// - **PrusaSlicer / SuperSlicer** : `Metadata/Slic3r_PE.config`, les mêmes
///   lignes `; clé = valeur` qu'en pied de G-code ;
/// - **Bambu Studio / OrcaSlicer** : `Metadata/slice_info.config`, un XML qui
///   annonce la durée (`prediction`, en **secondes**) et le poids (`weight`, en
///   grammes), et nomme la matière dans `<filament type="PLA">`.
///
/// Un 3MF exporté sans tranchage (« modèle seul ») n'a ni l'un ni l'autre : le
/// résultat est alors vide, et c'est une réponse — pas une panne.
fn metadata_of(path: &Path) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::new();
    let Ok(file) = fs::File::open(path) else {
        return meta;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return meta;
    };

    for index in 0..archive.len() {
        let name = archive
            .name_for_index(index)
            .unwrap_or("")
            .to_ascii_lowercase();
        if !name.starts_with("metadata/") || !name.ends_with(".config") {
            continue;
        }
        let mut text = String::new();
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        if entry.read_to_string(&mut text).is_err() {
            continue;
        }
        drop(entry);

        // Les deux lectures coexistent : un fichier PrusaSlicer n'a pas de
        // balises `<metadata>`, un fichier Bambu n'a pas de commentaires `;`.
        // Le filtre des clés connues laisse de côté les réglages fins.
        print_info::collect_pairs(&text, true, 0, &mut meta);
        for (cle, valeur) in xml_metadata(&text) {
            meta.entry(cle).or_insert(valeur);
        }
    }
    meta
}

/// Couples `clé → valeur` d'un XML de type `slice_info.config`, ramenés au
/// vocabulaire de [`print_info`].
fn xml_metadata(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();

    for fragment in text.split("<metadata").skip(1) {
        let (Some(cle), Some(valeur)) = (attribute(fragment, "key"), attribute(fragment, "value"))
        else {
            continue;
        };
        let mapped = match cle.as_str() {
            "prediction" => "time",
            "weight" => "filament_used_g",
            _ => continue,
        };
        out.push((mapped.to_string(), valeur));
    }

    // `<filament id="1" type="PLA" …>` : la matière, écrite en attribut.
    for fragment in text.split("<filament").skip(1) {
        if let Some(matiere) = attribute(fragment, "type") {
            out.push(("filament_type".to_string(), matiere));
        }
    }
    out
}

/// Valeur d'un attribut XML (`clé="valeur"`).
fn attribute(fragment: &str, nom: &str) -> Option<String> {
    let debut = fragment.find(&format!("{nom}=\""))? + nom.len() + 2;
    let fin = fragment[debut..].find('"')? + debut;
    Some(fragment[debut..fin].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Écrit un 3MF minimal (une seule entrée) dans un fichier temporaire.
    fn trois_mf(nom: &str, contenu: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piece.3mf");
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(nom, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contenu.as_bytes()).unwrap();
        zip.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn lit_le_profil_prusaslicer() {
        let (_dir, path) = trois_mf(
            "Metadata/Slic3r_PE.config",
            "; avoid_crossing_perimeters = 1\n\
             ; filament_type = PLA\n\
             ; layer_height = 0.15\n\
             ; printer_model = MK3S\n",
        );

        let info = THREE_MF.print_info("piece.3mf", &path);
        assert_eq!(info.material.as_deref(), Some("PLA"));
        assert_eq!(info.layer_height.as_deref(), Some("0.15"));
        assert_eq!(info.printer.as_deref(), Some("MK3S"));
        // Le réglage fin est écarté, la taille du fichier vient du disque.
        assert!(info.bytes.is_some_and(|b| b > 0));
    }

    #[test]
    fn lit_le_slice_info_de_bambu() {
        let (_dir, path) = trois_mf(
            "Metadata/slice_info.config",
            // `r##` : le contenu porte un `"#` (la couleur `#FFFFFF`).
            r##"<?xml version="1.0"?>
<config>
  <plate>
    <metadata key="prediction" value="8100"/>
    <metadata key="weight" value="12.34"/>
    <filament id="1" type="PETG" color="#FFFFFF"/>
  </plate>
</config>"##,
        );

        let info = THREE_MF.print_info("piece.3mf", &path);
        assert_eq!(info.duration_seconds, Some(8100));
        assert_eq!(info.filament_grams, Some(12.34));
        assert_eq!(info.material.as_deref(), Some("PETG"));
    }

    #[test]
    fn un_3mf_non_tranche_n_annonce_rien() {
        let (_dir, path) = trois_mf("3D/3dmodel.model", "<model/>");
        let info = THREE_MF.print_info("piece.3mf", &path);
        // La taille du fichier seule ne fait pas une information d'impression.
        assert!(info.material.is_none() && info.duration_seconds.is_none());
    }
}
