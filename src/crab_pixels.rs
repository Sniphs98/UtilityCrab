//! Das App-Icon (Ferris mit Warnweste und Schraubenschlüssel) aus `assets/icon.png`,
//! in die .exe eingebettet und auf die gewünschte Größe verkleinert.
//! Ein neues Bild bereitet `cargo run --release --example icon_from_image` auf.

use std::sync::OnceLock;

use image::imageops::FilterType;
use image::RgbaImage;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn source() -> &'static RgbaImage {
    static SOURCE: OnceLock<RgbaImage> = OnceLock::new();
    SOURCE.get_or_init(|| {
        image::load_from_memory(ICON_PNG)
            .expect("assets/icon.png ist kein gültiges Bild")
            .to_rgba8()
    })
}

/// RGBA-Pixel (`size`×`size`).
pub fn crab_rgba(size: u32) -> Vec<u8> {
    image::imageops::resize(source(), size, size, FilterType::Lanczos3).into_raw()
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_transparent_corners() {
        let rgba = super::crab_rgba(32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert_eq!(rgba[3], 0, "Ecke oben links sollte durchsichtig sein");
    }
}
