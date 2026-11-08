//! Feste Design-Werte für Hinweise: Abstände, Schriften, Farben.
//! Kompakt wie eine macOS-Mitteilung: wenig Zeilen, klare Gewichtung über Schriftschnitt
//! und Farbe statt über Größe.

use eframe::egui::{Color32, FontFamily, FontId};

use super::model::{Kind, Tone};

/// Breite der Karte (logische Pixel). Das Fenster ist genau die Karte; runde Ecken,
/// Rahmen und Schatten zeichnet Windows 11 selbst.
pub const CARD_WIDTH: f32 = 372.0;
/// Abstand zur Taskleiste.
pub const BOTTOM_GAP: f32 = 12.0;

pub const PAD_X: f32 = 14.0;
pub const PAD_Y: f32 = 12.0;
pub const ICON_SIZE: f32 = 26.0;
/// Text unter dem Titel beginnt bündig mit dem Titel, nicht mit dem Symbol.
pub const INDENT: f32 = ICON_SIZE + 10.0;
/// Abstand zwischen Bausteinen bzw. vor der Fußzeile.
pub const GAP: f32 = 6.0;
pub const FOOTER_GAP: f32 = 10.0;

pub const TITLE_SIZE: f32 = 14.5;
pub const BODY_SIZE: f32 = 13.0;
pub const SMALL_SIZE: f32 = 12.0;

pub const CARD_BG: Color32 = Color32::from_rgb(0x26, 0x28, 0x2C);
pub const CARD_BORDER: Color32 = Color32::from_rgb(0x3C, 0x3F, 0x46);
pub const TOOLTIP_BG: Color32 = Color32::from_rgb(0x33, 0x36, 0x3C);
pub const TRACK: Color32 = Color32::from_rgb(0x3A, 0x3D, 0x44);
pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF3);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0xA6, 0xAB, 0xB3);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x80, 0x85, 0x8E);

pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x1C);
pub const GREEN: Color32 = Color32::from_rgb(0x4C, 0xC0, 0x6A);
pub const AMBER: Color32 = Color32::from_rgb(0xF0, 0xB0, 0x40);
pub const RED: Color32 = Color32::from_rgb(0xEE, 0x5D, 0x4E);
pub const BLUE: Color32 = Color32::from_rgb(0x5F, 0xA4, 0xF0);

/// Schriftfamilie für Hervorgehobenes (Segoe UI Semibold, sonst normal).
pub const SEMIBOLD: &str = "semibold";

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Running => ACCENT,
        Kind::Success => GREEN,
        Kind::Warning => AMBER,
        Kind::Error => RED,
        Kind::Info => BLUE,
    }
}

pub fn tone_color(tone: Tone) -> Color32 {
    match tone {
        Tone::Good => GREEN,
        Tone::Warn => AMBER,
        Tone::Bad => RED,
        Tone::Neutral => TEXT_WEAK,
    }
}

/// Farbe mit geringer Deckkraft, für getönte Hintergründe.
pub fn tint(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8)
}
