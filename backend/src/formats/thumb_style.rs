//! Mise au **style de la maison** des vignettes qui ne viennent pas de notre
//! rasteriseur.
//!
//! Les slicers embarquent leur propre rendu dans le G-code : le modèle à plat, à
//! la couleur du filament, sur fond clair (PrusaSlicer, OrcaSlicer, Cura…).
//! Posé à côté des rendus de maillage — matière claire sur fond sombre — l'écart
//! se voit tout de suite dans la grille.
//!
//! On ne peut pas relancer le rendu (un G-code n'est pas un maillage), donc on
//! **repeint** l'image reçue : le fond devient celui de l'application, la
//! silhouette est conservée, et l'ombrage interne — la seule information de
//! relief de ces vignettes — est ramené dans la plage claire de la matière.
//!
//! Ce que ça donne, sur une vignette « modèle bleu sur blanc » : un modèle clair
//! sur fond bleu nuit, comme les STL. Une vignette déjà sombre est simplement
//! confirmée dans la palette.

use super::{THUMB_BASE, THUMB_BG};
use image::RgbaImage;

/// Saturation en deçà de laquelle un pixel fait partie du **décor**.
///
/// Le fond d'une vignette de slicer est **neutre** : blanc, gris, ou le plateau
/// et sa grille. Le modèle, lui, est à la couleur du filament — c'est donc la
/// saturation qui les sépare, et non la distance à une couleur de fond.
/// Un modèle blanc ou gris, lui, ne se distingue pas : on préfère alors laisser
/// la vignette d'origine.
const CHROMA_MIN: f32 = 0.10;

/// Largeur du dégradé de bord, au-delà du seuil de décor.
///
/// Un pixel de bord anti-aliasé est partiellement coloré : il est donc mélangé
/// au prorata. En deçà du seuil, en revanche, on ne mélange rien — un fond très
/// légèrement teinté (le bleu nuit de l'application, par exemple) reste du fond.
const EDGE_RAMP: f32 = 0.06;

/// Part minimale de l'image que le sujet doit occuper.
///
/// En dessous, il n'y a pas de modèle à repeindre : mieux vaut garder la
/// vignette d'origine qu'afficher un fond uni.
const MIN_SUBJECT: f32 = 0.004;

/// Plage d'ombrage de la matière : le ton le plus sombre du sujet garde 75 % de
/// la matière, le plus clair la donne entière.
const SHADE_MIN: f32 = 0.75;

/// En deçà de cet écart entre le ton le plus sombre et le plus clair du sujet,
/// celui-ci est considéré comme **plat** : la vignette du slicer n'a pas dessiné
/// de relief, on ne va donc pas lui en inventer un (la matière est donnée
/// entière, sans modulation).
const FLAT_SPREAD: f32 = 0.02;

/// Repointe une vignette aux couleurs de l'application, sur place.
///
/// Retourne `false` — et laisse l'image **intacte** — quand il n'y a pas de
/// modèle identifiable : image uniforme, ou entièrement colorée (aucun fond).
/// Une vignette d'origine vaut mieux qu'une image sans sujet.
pub fn restyle(image: &mut RgbaImage) -> bool {
    let (width, height) = image.dimensions();
    if width < 2 || height < 2 {
        return false;
    }

    // 1. Le modèle : les pixels colorés. On relève au passage son ombrage, pour
    //    le ramener dans la plage claire de la matière sans l'aplatir.
    let total = (width * height) as usize;
    let mut subject = 0usize;
    let (mut dark, mut light) = (f32::MAX, f32::MIN);

    for pixel in image.pixels() {
        if chroma(pixel.0) <= CHROMA_MIN {
            continue;
        }
        subject += 1;
        let luma = luminance(pixel.0);
        dark = dark.min(luma);
        light = light.max(luma);
    }

    // Au moins un pixel de modèle, mais pas toute l'image : une vignette sans
    // fond uni ne se repeint pas (on ne saurait pas quoi garder).
    let floor = ((total as f32 * MIN_SUBJECT) as usize).max(1);
    if subject < floor || subject == total {
        return false;
    }

    // 2. Le repaint : le décor (fond, plateau, grille) prend la couleur de
    //    l'application, le modèle la matière — modulée par son ombrage
    //    d'origine, seule information de relief de ces vignettes.
    let spread = light - dark;
    for pixel in image.pixels_mut() {
        let source = pixel.0;
        // Progression de 0 (décor) à 1 (modèle franc) : les bords anti-aliasés,
        // partiellement colorés, sont mélangés.
        let coverage = ((chroma(source) - CHROMA_MIN) / EDGE_RAMP).clamp(0.0, 1.0);
        // Un modèle **plat** (le slicer n'a rien ombré) reçoit la matière pleine :
        // lui inventer un relief serait pire que de n'en pas avoir.
        let shade = if spread < FLAT_SPREAD {
            1.0
        } else {
            SHADE_MIN + (1.0 - SHADE_MIN) * ((luminance(source) - dark) / spread).clamp(0.0, 1.0)
        };

        let mut out = [0u8; 3];
        for channel in 0..3 {
            let material = THUMB_BASE[channel] * shade * 255.0;
            out[channel] = mix(THUMB_BG[channel] as f32, material, coverage).round() as u8;
        }
        *pixel = image::Rgba([out[0], out[1], out[2], 255]);
    }

    true
}

/// Écart entre le canal le plus fort et le plus faible, normalisé en 0–1.
///
/// C'est la mesure du « coloré » : 0 pour un gris (fond, plateau, grille),
/// élevé pour un filament coloré.
fn chroma(pixel: [u8; 4]) -> f32 {
    let max = pixel[0].max(pixel[1]).max(pixel[2]) as f32;
    let min = pixel[0].min(pixel[1]).min(pixel[2]) as f32;
    (max - min) / 255.0
}

/// Luminance perçue (Rec. 601), normalisée en 0–1.
fn luminance(pixel: [u8; 4]) -> f32 {
    (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) / 255.0
}

/// Mélange linéaire, `t` allant de 0 (tout `from`) à 1 (tout `to`).
fn mix(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Vignette « slicer » : fond clair, modèle bleu au centre.
    fn vignette_slicer() -> RgbaImage {
        let mut image = RgbaImage::from_pixel(30, 20, Rgba([252, 252, 250, 255]));
        for y in 6..14 {
            for x in 10..20 {
                image.put_pixel(x, y, Rgba([60, 130, 200, 255]));
            }
        }
        // Un relief plus sombre dans le modèle : il doit survivre au repaint.
        for y in 11..13 {
            for x in 10..13 {
                image.put_pixel(x, y, Rgba([40, 90, 150, 255]));
            }
        }
        image
    }

    #[test]
    fn repeint_une_vignette_claire_en_matiere_claire_sur_fond_sombre() {
        let mut image = vignette_slicer();
        assert!(restyle(&mut image));

        // Le fond est celui de l'application.
        assert_eq!(
            image.get_pixel(0, 0).0,
            [THUMB_BG[0], THUMB_BG[1], THUMB_BG[2], 255]
        );

        // Le modèle est clair (c'était le but) et garde sa teinte froide.
        let model = image.get_pixel(15, 7).0;
        assert!(luminance(model) > 0.65, "modèle trop sombre : {model:?}");
        assert!(model[2] > model[0], "teinte perdue : {model:?}");
    }

    #[test]
    fn conserve_l_ombrage_interne_du_modele() {
        let mut image = vignette_slicer();
        assert!(restyle(&mut image));

        let clair = luminance(image.get_pixel(15, 7).0);
        let sombre = luminance(image.get_pixel(11, 11).0);
        assert!(
            sombre < clair,
            "l'ombrage du slicer a été aplati ({sombre} vs {clair})"
        );
        assert!(sombre > 0.4, "l'ombrage est trop sombre : {sombre}");
    }

    #[test]
    fn une_image_uniforme_est_laissee_intacte() {
        let mut image = RgbaImage::from_pixel(12, 8, Rgba([255, 255, 255, 255]));
        let avant = image.clone();

        assert!(!restyle(&mut image));
        assert_eq!(image, avant, "l'image a été modifiée sans sujet");
    }

    #[test]
    fn un_modele_sans_couleur_est_laisse_tel_quel() {
        // Fond sombre, modèle **gris** : rien ne les distingue par la couleur.
        // Mieux vaut la vignette d'origine qu'une image sans modèle.
        let mut image = RgbaImage::from_pixel(20, 12, Rgba([30, 30, 34, 255]));
        for y in 3..9 {
            for x in 5..15 {
                image.put_pixel(x, y, Rgba([200, 202, 206, 255]));
            }
        }
        let avant = image.clone();

        assert!(!restyle(&mut image));
        assert_eq!(image, avant);
    }

    /// Rendus de **plateau** (3MF de Bambu Studio, vignettes de slicer) : fond
    /// gris, grille claire, modèle orange. La grille est neutre, donc elle fait
    /// partie du décor et disparaît ; seul le modèle est repeint.
    #[test]
    fn la_grille_du_plateau_fait_partie_du_decor() {
        let mut image = RgbaImage::from_pixel(40, 30, Rgba([63, 63, 63, 255]));
        for x in (0..40).step_by(5) {
            for y in 0..30 {
                image.put_pixel(x, y, Rgba([98, 98, 98, 255]));
            }
        }
        for y in 8..20 {
            for x in 12..28 {
                image.put_pixel(x, y, Rgba([161, 88, 46, 255]));
            }
        }

        assert!(restyle(&mut image));

        // La grille a rejoint le fond : plus de quadrillage.
        let fond = image.get_pixel(0, 0).0;
        assert_eq!(fond, [THUMB_BG[0], THUMB_BG[1], THUMB_BG[2], 255]);
        assert_eq!(
            image.get_pixel(5, 3).0,
            fond,
            "la grille est restée visible"
        );

        // Le modèle est clair, à la matière de la maison.
        assert!(luminance(image.get_pixel(20, 14).0) > 0.65);
    }

    #[test]
    fn une_image_trop_petite_est_ignoree() {
        let mut image = RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 255]));
        assert!(!restyle(&mut image));
    }
}
