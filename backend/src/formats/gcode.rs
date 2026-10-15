//! Format G-code (`.gcode`, `.gco`).
//!
//! L'aperçu est la **vignette que le slicer écrit en tête de fichier**, encodée
//! en base64 dans un commentaire :
//!
//! ```text
//! ; thumbnail begin 220x124 4780
//! ; iVBORw0KGgoAAAANSUhEUgAA…
//! ; thumbnail end
//! ```
//!
//! Formats d'en-tête reconnus : `thumbnail`, `thumbnail_PNG`, `thumbnail_JPG`
//! (PrusaSlicer / OrcaSlicer / SuperSlicer / Bambu Studio / Cura / IdeaMaker).
//! Si plusieurs tailles sont présentes, la plus grande est retenue.
//! Aucun parsing du toolpath n'est nécessaire.

use super::Format;
use super::Viewer;
use super::print_info::{self, PrintInfo};
use super::{THUMB_H, THUMB_W};
use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

/// Instance unique du format, référencée par le registre de [`super`].
pub static GCODE: Gcode = Gcode;

/// Le format G-code.
pub struct Gcode;

impl Format for Gcode {
    fn name(&self) -> &'static str {
        "G-code"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gcode", "gco"]
    }

    fn content_type(&self) -> &'static str {
        "text/plain"
    }

    fn has_preview(&self) -> bool {
        true
    }

    fn viewer(&self) -> Viewer {
        Viewer::Gcode
    }

    fn thumbnail(&self, path: &Path, out: &Path) -> Option<()> {
        try_extract_thumbnail(path, out)
    }

    fn print_info(&self, model_rel: &str, path: &Path) -> PrintInfo {
        let mut info = PrintInfo::from_metadata(&metadata_for(model_rel, path));

        // La durée n'est **pas** un commentaire chez QIDISlicer (ni chez
        // PrusaSlicer, qui l'écrit pourtant aussi en clair dans son profil) :
        // elle est dans les `M73` du programme.
        if info.duration_seconds.is_none() {
            info.duration_seconds = first_m73_minutes(path).map(|minutes| minutes * 60);
        }
        // L'encombrement, lui, ne s'écrit nulle part : il se mesure sur le
        // toolpath. La taille du fichier vient du disque.
        info.size_mm = toolpath_size(path);
        info.bytes = fs::metadata(path).ok().map(|m| m.len());
        info
    }
}

/// Taille d'en-tête lue dans un G-code (la vignette est toujours tout au début,
/// avant les instructions d'impression ; inutile de lire 200 Mo).
const HEADER_LIMIT: usize = 512 * 1024;

/// Extrait la vignette embarquée d'un G-code et l'écrit en PNG dans `out`.
fn try_extract_thumbnail(model_path: &Path, out: &Path) -> Option<()> {
    fs::create_dir_all(out.parent()?).ok()?;
    let head = read_head(model_path, HEADER_LIMIT)?;
    let bytes = extract_largest_thumbnail(&String::from_utf8_lossy(&head))?;
    let img = image::load_from_memory(&bytes).ok()?;
    let scaled = if img.width() > THUMB_W as u32 || img.height() > THUMB_H as u32 {
        img.thumbnail(THUMB_W as u32, THUMB_H as u32)
    } else {
        img
    };

    // La vignette du slicer est claire, à la couleur du filament : on la ramène
    // au style de la maison (matière claire sur fond sombre), sans quoi elle
    // jure à côté des rendus de maillage. Si elle n'a pas de sujet identifiable,
    // `restyle` la laisse telle quelle.
    let mut styled = scaled.to_rgba8();
    super::thumb_style::restyle(&mut styled);

    styled.save(out).ok()?;
    Some(())
}

/// Lit au plus `limit` octets au début d'un fichier.
fn read_head(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let mut filled = 0;
    while filled < limit {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    buf.truncate(filled);
    Some(buf)
}

/// Lit au plus `limit` octets à la **fin** d'un fichier.
///
/// La première ligne est **écartée** : la coupe la tronque en son milieu, et une
/// paire `clé = valeur` amputée n'est pas une métadonnée.
fn read_tail(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit as u64)))
        .ok()?;
    let mut buf = Vec::new();
    file.take(limit as u64).read_to_end(&mut buf).ok()?;
    if let Some(fin) = buf.iter().position(|b| *b == b'\n') {
        buf.drain(..=fin);
    }
    Some(buf)
}

/// Durée annoncée par le **premier** `M73` du programme, en minutes.
///
/// Les slicers émettent `M73 P<avancement> R<minutes restantes>` tout du long :
/// la première ligne porte donc l'estimation **totale**, les suivantes ne font
/// que décompter. `M73 Q… S<secondes>` en est la variante « minuteur », traitée
/// de même (arrondie à la minute).
fn first_m73_minutes(path: &Path) -> Option<u64> {
    let head = read_head(path, HEADER_LIMIT)?;
    let text = String::from_utf8_lossy(&head);
    for line in text.lines() {
        let line = line.trim_start();
        if !line.starts_with("M73") {
            continue;
        }
        for mot in line.split_whitespace().skip(1) {
            if let Some(minutes) = mot.strip_prefix('R').and_then(|v| v.parse::<f64>().ok()) {
                return Some(minutes.round().max(0.0) as u64);
            }
            if let Some(seconds) = mot.strip_prefix('S').and_then(|v| v.parse::<f64>().ok()) {
                return Some((seconds / 60.0).round().max(0.0) as u64);
            }
        }
    }
    None
}

/// Encombrement de l'objet imprimé, en mm : `[X, Y, Z]`.
///
/// Aucun slicer n'écrit cette information : elle se lit sur le toolpath. Seuls
/// les **mouvements d'extrusion** comptent (`G0`..`G3` portant un `E` en
/// paramètre) — un déplacement à vide part au coin du plateau ou en Z-hop et
/// gonflerait la boîte (un `G0 Z50` de mise en chauffe donnerait 50 mm de haut
/// pour un objet qui en fait 30).
///
/// La hauteur se suit **ligne à ligne** : la plupart des slicers séparent la
/// montée en Z (`G1 Z0.4 F600`) de l'extrusion (`G1 X… Y… E…`), si bien qu'une
/// extrusion ne porte pas toujours son propre Z. On retient donc la hauteur en
/// cours à ce moment-là — ce qui écarte au passage la remontée de fin de
/// programme (la tête qui monte de 30 mm après la dernière extrusion).
fn toolpath_size(path: &Path) -> Option<[f64; 3]> {
    let file = fs::File::open(path).ok()?;
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    let mut vus = [false; 3];
    let mut hauteur: Option<f64> = None;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        // Commentaire écarté : il peut raconter n'importe quoi (`; X10`).
        let code = line.trim_start().split(';').next().unwrap_or_default();
        if !matches!(
            code.get(..2),
            Some("G0") | Some("G1") | Some("G2") | Some("G3")
        ) {
            continue;
        }

        let mut extrusion = false;
        let mut coordonnees = [None; 3];
        for mot in code.split_whitespace() {
            let Some(reste) = mot.get(1..) else {
                continue;
            };
            match mot.as_bytes().first() {
                Some(b'X') => coordonnees[0] = reste.parse::<f64>().ok(),
                Some(b'Y') => coordonnees[1] = reste.parse::<f64>().ok(),
                Some(b'Z') => coordonnees[2] = reste.parse::<f64>().ok(),
                Some(b'E') => extrusion = reste.parse::<f64>().is_ok(),
                _ => {}
            }
        }
        if let Some(z) = coordonnees[2] {
            hauteur = Some(z);
        }
        if !extrusion {
            continue;
        }

        for axe in 0..3 {
            let valeur = if axe == 2 { hauteur } else { coordonnees[axe] };
            let Some(valeur) = valeur else {
                continue;
            };
            min[axe] = min[axe].min(valeur);
            max[axe] = max[axe].max(valeur);
            vus[axe] = true;
        }
    }

    // Un axe jamais vu (fichier qui n'extrude pas) : pas de dimensions du tout,
    // plutôt qu'un `null` dans le JSON.
    vus.iter()
        .all(|vu| *vu)
        .then(|| [max[0] - min[0], max[1] - min[1], max[2] - min[2]])
}

/// Nombre maximal de métadonnées retenues dans l'**en-tête**.
const META_MAX: usize = 24;

/// Taille de la **fin** de fichier examinée pour le profil du slicer.
///
/// PrusaSlicer, SuperSlicer, OrcaSlicer, Bambu Studio et QIDISlicer écrivent le
/// profil complet (`; clé = valeur`, plusieurs centaines de lignes) **après** le
/// toolpath — pas en tête. 256 Ko couvrent très largement ce profil (quelques
/// dizaines de kilo-octets dans les faits).
const FOOTER_LIMIT: usize = 256 * 1024;

/// Métadonnées lisibles annoncées par un slicer.
///
/// Renvoie ce qui a été trouvé (`clé → valeur`), vide si le fichier n'est pas un
/// G-code ou s'il n'annonce rien. Trois écritures sont reconnues :
///
/// ```text
/// ; generated by PrusaSlicer 2.7.4      → generated_by = PrusaSlicer 2.7.4
/// ; layer_height = 0.2                  → layer_height = 0.2
/// ;TIME:1234                            → time = 1234      (Cura)
/// ;Filament used: 1.24m, 9.5g           → filament_used = 1.24m, 9.5g
/// ```
///
/// L'**en-tête** est lu sans filtre (quelques dizaines de paires : provenance,
/// vignette, largeurs d'extrusion) ; le **pied** seulement sur les clés de
/// [`print_info::KNOWN_KEYS`], parce qu'un profil de slicer en compte des
/// centaines dont une poignée seulement parle.
///
/// Ces valeurs servent à la **recherche assistée** : « une pièce en PETG », « ce
/// qui s'imprime en moins d'une heure » sont des questions auxquelles le nom du
/// fichier ne répond pas.
pub fn metadata_for(model_rel: &str, path: &Path) -> BTreeMap<String, String> {
    if !handles(&super::ext_of(model_rel)) {
        return BTreeMap::new();
    }

    let mut found = BTreeMap::new();
    if let Some(head) = read_head(path, HEADER_LIMIT) {
        print_info::collect_pairs(&String::from_utf8_lossy(&head), false, META_MAX, &mut found);
    }
    if let Some(footer) = read_tail(path, FOOTER_LIMIT) {
        print_info::collect_pairs(&String::from_utf8_lossy(&footer), true, 0, &mut found);
    }
    found
}

/// `true` si cette extension est un G-code (utilisé aussi par le scan).
pub fn handles(ext: &str) -> bool {
    GCODE
        .extensions()
        .iter()
        .any(|e| e.eq_ignore_ascii_case(ext))
}

/// Cherche la plus grande vignette embarquée dans un en-tête de G-code.
///
/// Côté `f32`/`u32` uniquement : les blocs dont le format n'est pas décodable
/// par la crate `image` (QOI) sont ignorés.
fn extract_largest_thumbnail(text: &str) -> Option<Vec<u8>> {
    // Bloc `begin … end` en cours d'accumulation (surface + base64).
    let mut pending: Option<(u32, String)> = None;
    let mut best: Option<(u32, Vec<u8>)> = None;

    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(';').trim();
        let lower = line.to_ascii_lowercase();

        if lower.starts_with("thumbnail") {
            if lower.contains("begin") {
                // `thumbnail_QOI` n'est pas supporté : on n'accumule rien pour
                // ce bloc (les lignes suivantes seront ignorées).
                pending = dims_of(&lower)
                    .filter(|_| !lower.contains("_qoi"))
                    .map(|(w, h)| (w.saturating_mul(h), String::new()));
                continue;
            }
            if lower.contains("end") {
                if let Some((area, data)) = pending.take()
                    && let Some(bytes) = base64_decode(&data)
                    && image::guess_format(&bytes).is_ok()
                    && best.as_ref().is_none_or(|(a, _)| area > *a)
                {
                    best = Some((area, bytes));
                }
                continue;
            }
        }

        // Ligne de données base64 à l'intérieur d'un bloc `begin … end`.
        if let Some((_, data)) = pending.as_mut() {
            data.push_str(line);
        }
    }

    best.map(|(_, bytes)| bytes)
}

/// Dimensions (`220x124`) annoncées par un en-tête `thumbnail begin`.
fn dims_of(header: &str) -> Option<(u32, u32)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\d+)\s*x\s*(\d+)").expect("regex valide"));
    let caps = re.captures(header)?;
    Some((caps[1].parse().ok()?, caps[2].parse().ok()?))
}

/// Décode du base64 standard en ignorant tout caractère hors alphabet (retours
/// à la ligne, espaces, `;`). `None` si rien n'a pu être décodé.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    /// Valeur (0–63) d'un caractère base64, ou `None` s'il est à ignorer.
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = value(c) else { continue };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            // Ne conserve que les bits non encore consommés.
            acc &= (1 << bits) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signature PNG (8 octets) encodée en base64 — sert de charge utile aux
    /// tests, `image::guess_format` ne lit que les octets magiques.
    const PNG_SIG_B64: &str = "iVBORw0KGgo=";
    const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    #[test]
    fn base64_decode_ignore_les_caracteres_etrangers() {
        // Retours à la ligne et padding `=` ignorés.
        assert_eq!(base64_decode("M0\nQ=").unwrap(), b"3D");
        assert_eq!(base64_decode(PNG_SIG_B64).unwrap(), PNG_SIG);
        assert!(base64_decode("   \n  ").is_none());
    }

    #[test]
    fn dims_of_lit_les_dimensions_annoncees() {
        assert_eq!(dims_of("thumbnail begin 220x124 4780"), Some((220, 124)));
        assert_eq!(dims_of("thumbnail_jpg begin 32x32 96"), Some((32, 32)));
        assert_eq!(dims_of("thumbnail begin"), None);
    }

    #[test]
    fn extrait_la_plus_grande_vignette_gcode() {
        let text = format!(
            "; thumbnail begin 16x16 12\n; {PNG_SIG_B64}\n; thumbnail end\n\
             ;thumbnail begin 220x124 12\n; {PNG_SIG_B64}\n;thumbnail end\n"
        );
        // Les deux blocs contiennent la même charge utile : on vérifie le
        // décodage et la sélection de la surface la plus grande.
        assert_eq!(extract_largest_thumbnail(&text).unwrap(), PNG_SIG);
    }

    #[test]
    fn ignore_les_blocs_qoi() {
        let text = "; thumbnail_QOI begin 16x16 4\n; qoif\n; thumbnail_QOI end\n";
        assert!(extract_largest_thumbnail(text).is_none());
    }

    #[test]
    fn aucun_apercu_dans_un_gcode_sans_vignette() {
        let text = "; generated by Cura\nG1 X10 Y10 E1\nG1 X20 Y10 E2\n";
        assert!(extract_largest_thumbnail(text).is_none());
    }

    /// Encode en base64 standard (utilisé seulement par les tests).
    fn base64_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn extrait_les_metadonnees_d_un_slicer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piece.gcode");
        fs::write(
            &path,
            "; generated by OrcaSlicer 2.1.0 on 2026-09-19\n\
             ; thumbnail begin 16x16 12\n\
             ; iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==\n\
             ; thumbnail end\n\
             ; layer_height = 0.2\n\
             ; nozzle_diameter = 0.4\n\
             ; filament_type = PETG\n\
             ; estimated printing time (normal mode) = 1h 12m\n\
             G1 X0 Y0 E1\n",
        )
        .unwrap();

        let meta = metadata_for("piece.gcode", &path);
        assert_eq!(
            meta.get("generated_by").map(String::as_str),
            Some("OrcaSlicer 2.1.0 on 2026-09-19")
        );
        assert_eq!(meta.get("layer_height").map(String::as_str), Some("0.2"));
        assert_eq!(meta.get("filament_type").map(String::as_str), Some("PETG"));
        assert_eq!(meta.get("nozzle_diameter").map(String::as_str), Some("0.4"));
        assert_eq!(
            meta.get("estimated_printing_time_normal_mode")
                .map(String::as_str),
            Some("1h 12m")
        );
        // Le bloc de vignette (base64, padding `=`) ne doit pas passer pour des
        // métadonnées.
        assert!(
            !meta.values().any(|v| v.contains("iVBORw0KGgo")),
            "la vignette a été prise pour une métadonnée : {meta:?}"
        );
    }

    #[test]
    fn comprend_l_ecriture_cura() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piece.gco");
        fs::write(
            &path,
            ";FLAVOR:Marlin\n;TIME:4321\n;Filament used: 1.24m, 9.5g\n\
             ;Layer height: 0.2\n;LAYER_COUNT:120\n",
        )
        .unwrap();

        let meta = metadata_for("piece.gco", &path);
        assert_eq!(meta.get("time").map(String::as_str), Some("4321"));
        assert_eq!(
            meta.get("filament_used").map(String::as_str),
            Some("1.24m, 9.5g")
        );
        assert_eq!(meta.get("layer_height").map(String::as_str), Some("0.2"));
        assert_eq!(meta.get("layer_count").map(String::as_str), Some("120"));
    }

    /// L'encombrement, alors que QIDISlicer sépare la hauteur de l'extrusion.
    ///
    /// Ses extrusions ne portent pas de Z (`G1 X… Y… E…`) : la montée est écrite
    /// à part (`G1 Z0.2 F600`). Et la fin de programme remonte la tête de 50 mm,
    /// sans extruder — ce qui ne doit pas compter comme une hauteur d'objet.
    #[test]
    fn encombrement_malgre_la_hauteur_ecrite_a_part() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("piece.gcode");
        fs::write(
            &path,
            "G0 Z50 F600\n\
             G1 Z0.2 F600\n\
             G1 X10 Y10 E1\n\
             G1 X110 Y10 E5\n\
             G1 Z30.2 F600\n\
             G1 X110 Y60 E6\n\
             G1 X10 Y60 E7\n\
             G1 Z100 F600\n\
             G1 X0 Y0 F9000\n",
        )
        .unwrap();

        assert_eq!(toolpath_size(&path), Some([100.0, 50.0, 30.0]));
        // Un fichier sans aucune extrusion n'a pas de dimensions du tout.
        let vide = dir.path().join("vide.gcode");
        fs::write(&vide, "G0 Z50\nG1 X10 Y10 F9000\n").unwrap();
        assert_eq!(toolpath_size(&vide), None);
    }

    #[test]
    fn pas_de_metadonnees_pour_les_autres_formats() {
        let dir = tempfile::tempdir().unwrap();
        let stl = dir.path().join("piece.stl");
        fs::write(&stl, "; layer_height = 0.2\n").unwrap();

        // Un STL n'a pas de métadonnées de slicer, même si le contenu en parle.
        assert!(metadata_for("piece.stl", &stl).is_empty());
        // Fichier absent : aucune erreur, aucune métadonnée.
        assert!(metadata_for("piece.gcode", &dir.path().join("absent.gcode")).is_empty());
    }

    #[test]
    fn genere_un_apercu_depuis_la_vignette_embarquee() {
        use std::io::Cursor;

        // Vraie vignette de slicer : fond clair, modèle bleu au centre.
        let mut img = image::RgbaImage::from_pixel(24, 16, image::Rgba([250, 250, 248, 255]));
        for y in 5..11 {
            for x in 6..18 {
                img.put_pixel(x, y, image::Rgba([60, 130, 200, 255]));
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let gcode = dir.path().join("piece.gcode");
        fs::write(
            &gcode,
            format!(
                "; generated by OrcaSlicer\n\
                 ; thumbnail begin 24x16 {}\n; {}\n; thumbnail end\n\
                 G1 X0 Y0 E1\n",
                png.len(),
                base64_encode(&png),
            ),
        )
        .unwrap();

        let out = dir.path().join("thumbs/piece.png");
        assert!(try_extract_thumbnail(&gcode, &out).is_some());
        let written = image::open(&out).unwrap().to_rgba8();
        assert_eq!((written.width(), written.height()), (24, 16));

        // La vignette du slicer est **repeinte** au style de la maison : fond
        // sombre de l'application, modèle clair. Sans ça, un G-code bleu sur
        // blanc jurerait à côté des rendus de maillage.
        assert_eq!(
            written.get_pixel(0, 0).0,
            [
                super::super::THUMB_BG[0],
                super::super::THUMB_BG[1],
                super::super::THUMB_BG[2],
                255
            ]
        );
        let model = written.get_pixel(12, 7).0;
        let luma =
            (0.299 * model[0] as f32 + 0.587 * model[1] as f32 + 0.114 * model[2] as f32) / 255.0;
        assert!(luma > 0.65, "modèle trop sombre après repaint : {model:?}");
        assert!(model[2] > model[0], "teinte froide perdue : {model:?}");
    }
}
