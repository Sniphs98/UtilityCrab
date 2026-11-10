//! Umrechnung zwischen Bildpixeln und Bildschirmpunkten (Zoom und Verschiebung).

use eframe::egui::{Pos2, Rect};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Bildschirmposition der linken oberen Bildecke.
    pub offset: Pos2,
    /// Bildschirmpunkte pro Bildpixel.
    pub scale: f32,
}

impl View {
    pub fn to_screen(self, p: Pos2) -> Pos2 {
        self.offset + p.to_vec2() * self.scale
    }

    pub fn to_image(self, p: Pos2) -> Pos2 {
        ((p - self.offset) / self.scale).to_pos2()
    }

    pub fn rect_to_screen(self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }

    /// Wie viele Bildpixel `points` Bildschirmpunkte sind (z. B. Trefferradius).
    pub fn image_len(self, points: f32) -> f32 {
        points / self.scale
    }
}
