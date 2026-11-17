//! Selbst gezeichnete Symbole – unabhängig davon, welche Zeichen die Schrift kennt.

use std::f32::consts::TAU;

use crate::geom::{vec2, Color32, Pos2, Rect};
use crate::ui_kit::Painter;

use super::model::Kind;
use super::theme::{kind_color, tint};

/// Status-Symbol: getönter Kreis mit Haken, Ausrufezeichen, Kreuz, „i“ oder Drehkreis.
/// `time` in Sekunden treibt den Drehkreis an.
pub fn status(p: &mut Painter, rect: Rect, kind: Kind, time: f64) {
    let color = kind_color(kind);
    let c = rect.center();
    let r = rect.width() / 2.0;
    let u = r / 15.0; // Maßeinheit relativ zur Größe
    let width = 2.4 * u;

    match kind {
        Kind::Running => {
            p.circle_stroke(c, r - 2.0 * u, 2.6 * u, tint(color, 0.18));
            let start = (time * 1.6 * TAU as f64) as f32 % TAU;
            p.arc(c, r - 2.0 * u, start, start + TAU * 0.28, 2.6 * u, color);
        }
        Kind::Success => {
            p.circle_filled(c, r, tint(color, 0.16));
            p.line(
                &[c + vec2(-5.5, 0.3) * u, c + vec2(-1.8, 4.0) * u, c + vec2(5.8, -4.2) * u],
                width,
                color,
            );
        }
        Kind::Warning => {
            p.circle_filled(c, r, tint(color, 0.16));
            p.line(&[c + vec2(0.0, -6.0) * u, c + vec2(0.0, 1.8) * u], width, color);
            p.circle_filled(c + vec2(0.0, 5.6) * u, 1.5 * u, color);
        }
        Kind::Error => {
            p.circle_filled(c, r, tint(color, 0.16));
            cross(p, c, 4.6 * u, width, color);
        }
        Kind::Info => {
            p.circle_filled(c, r, tint(color, 0.16));
            p.circle_filled(c + vec2(0.0, -5.6) * u, 1.5 * u, color);
            p.line(&[c + vec2(0.0, -1.8) * u, c + vec2(0.0, 6.0) * u], width, color);
        }
    }
}

pub fn cross(p: &mut Painter, c: Pos2, half: f32, width: f32, color: Color32) {
    p.line(&[c + vec2(-half, -half), c + vec2(half, half)], width, color);
    p.line(&[c + vec2(-half, half), c + vec2(half, -half)], width, color);
}

/// Kleiner Pfeil nach rechts (für „vorher ▸ nachher“).
pub fn arrow(p: &mut Painter, c: Pos2, width: f32, color: Color32) {
    let tip = c + vec2(width / 2.0, 0.0);
    p.line(&[c - vec2(width / 2.0, 0.0), tip], 1.6, color);
    p.line(&[tip + vec2(-4.0, -4.0), tip, tip + vec2(-4.0, 4.0)], 1.6, color);
}

/// Zwei versetzte Blätter – „in der Zwischenablage“. `scale` 1.0 ≈ 12 px.
pub fn clipboard(p: &mut Painter, c: Pos2, color: Color32, scale: f32) {
    let back = Rect::from_center_size(c + vec2(-1.8, -1.8) * scale, vec2(8.0, 9.0) * scale);
    let front = Rect::from_center_size(c + vec2(1.8, 1.8) * scale, vec2(8.0, 9.0) * scale);
    p.rect_stroke(back, 2.0, 1.2, color);
    p.rect_filled(front, 2.0, super::theme::CARD_BG);
    p.rect_stroke(front, 2.0, 1.2, color);
}

/// Kleines „i“ im Kreis.
pub fn info(p: &mut Painter, c: Pos2, color: Color32) {
    p.circle_stroke(c, 6.5, 1.3, color);
    p.circle_filled(c + vec2(0.0, -2.9), 1.0, color);
    p.line(&[c + vec2(0.0, -0.8), c + vec2(0.0, 3.4)], 1.4, color);
}

/// Pfeilspitze nach unten bzw. rechts (Details auf-/zugeklappt).
pub fn chevron(p: &mut Painter, c: Pos2, open: bool, color: Color32) {
    let points = if open {
        [c + vec2(-3.5, -1.8), c + vec2(0.0, 1.8), c + vec2(3.5, -1.8)]
    } else {
        [c + vec2(-1.8, -3.5), c + vec2(1.8, 0.0), c + vec2(-1.8, 3.5)]
    };
    p.line(&points, 1.5, color);
}
