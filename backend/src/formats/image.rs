//! Images d'aperçu « sœur » (`piece.stl` + `piece.png`).
//!
//! Ce ne sont **pas** des formats de modèles : elles ne sont ni listées comme
//! telles ni rendues par le backend. Le scan s'en sert uniquement pour associer
//! une image explicite à un modèle, prioritairement à l'aperçu généré.

use super::ext_of;

/// Extensions d'image reconnues, **par ordre de préférence**.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// `true` si `name` est une image d'aperçu reconnue.
pub fn is_image(name: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext_of(name).as_str())
}

/// Rang de préférence d'une extension d'image (0 = préférée).
///
/// `99` si l'extension n'est pas reconnue.
pub fn priority(ext: &str) -> u32 {
    IMAGE_EXTENSIONS
        .iter()
        .position(|e| *e == ext)
        .unwrap_or(99) as u32
}

/// Type MIME d'une image, `application/octet-stream` sinon.
pub fn content_type(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}
