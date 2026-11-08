//! Selbst gezeichnete Symbole – unabhängig davon, welche Zeichen die Schrift kennt.

use std::f32::consts::TAU;

use eframe::egui::{pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke};

use super::model::Kind;
use super::theme::{kind_color, tint};

/// Status-Symbol: getönter Kreis mit Haken, Ausrufezeichen, Kreuz, „i“ oder Drehkreis.
/// `time` in Sekunden treibt den Drehkreis an.
pub fn status(painter: &Painter, rect: Rect, kind: Kind, time: f64) {
    let color = kind_color(kind);
    let c = rect.center();
    let r = rect.width() / 2.0;
    let u = r / 15.0; // Maßeinheit relativ zur Größe
    let stroke = Stroke::new(2.4 * u, color);

    match kind {
        Kind::Running => {
            painter.circle_stroke(c, r - 2.0 * u, Stroke::new(2.6 * u, tint(color, 0.18)));
            let start = (time * 1.6 * TAU as f64) as f32 % TAU;
            arc(painter, c, r - 2.0 * u, start, start + TAU * 0.28, Stroke::new(2.6 * u, color));
        }
        Kind::Success => {
            painter.circle_filled(c, r, tint(color, 0.16));
            let points = vec![
                c + vec2(-5.5, 0.3) * u,
                c + vec2(-1.8, 4.0) * u,
                c + vec2(5.8, -4.2) * u,
            ];
            painter.add(Shape::line(points, stroke));
        }
        Kind::Warning => {
            painter.circle_filled(c, r, tint(color, 0.16));
            painter.line_segment([c + vec2(0.0, -6.0) * u, c + vec2(0.0, 1.8) * u], stroke);
            painter.circle_filled(c + vec2(0.0, 5.6) * u, 1.5 * u, color);
        }
        Kind::Error => {
            painter.circle_filled(c, r, tint(color, 0.16));
            cross(painter, c, 4.6 * u, stroke);
        }
        Kind::Info => {
            painter.circle_filled(c, r, tint(color, 0.16));
            painter.circle_filled(c + vec2(0.0, -5.6) * u, 1.5 * u, color);
            painter.line_segment([c + vec2(0.0, -1.8) * u, c + vec2(0.0, 6.0) * u], stroke);
        }
    }
}

pub fn cross(painter: &Painter, c: Pos2, half: f32, stroke: Stroke) {
    painter.line_segment([c + vec2(-half, -half), c + vec2(half, half)], stroke);
    painter.line_segment([c + vec2(-half, half), c + vec2(half, -half)], stroke);
}

/// Kleiner Pfeil nach rechts (für „vorher ▸ nachher“).
pub fn arrow(painter: &Painter, c: Pos2, width: f32, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let tip = c + vec2(width / 2.0, 0.0);
    painter.line_segment([c - vec2(width / 2.0, 0.0), tip], stroke);
    painter.line_segment([tip, tip + vec2(-4.0, -4.0)], stroke);
    painter.line_segment([tip, tip + vec2(-4.0, 4.0)], stroke);
}

/// Zwei versetzte Blätter – „in der Zwischenablage“. `scale` 1.0 ≈ 12 px.
pub fn clipboard(painter: &Painter, c: Pos2, color: Color32, scale: f32) {
    let stroke = Stroke::new(1.2, color);
    let back = Rect::from_center_size(c + vec2(-1.8, -1.8) * scale, vec2(8.0, 9.0) * scale);
    let front = Rect::from_center_size(c + vec2(1.8, 1.8) * scale, vec2(8.0, 9.0) * scale);
    painter.rect_stroke(back, CornerRadius::same(2), stroke, eframe::egui::StrokeKind::Middle);
    painter.rect_filled(front, CornerRadius::same(2), super::theme::CARD_BG);
    painter.rect_stroke(front, CornerRadius::same(2), stroke, eframe::egui::StrokeKind::Middle);
}

/// Kleines „i“ im Kreis.
pub fn info(painter: &Painter, c: Pos2, color: Color32) {
    painter.circle_stroke(c, 6.5, Stroke::new(1.3, color));
    painter.circle_filled(c + vec2(0.0, -2.9), 1.0, color);
    painter.line_segment([c + vec2(0.0, -0.8), c + vec2(0.0, 3.4)], Stroke::new(1.4, color));
}

/// Pfeilspitze nach unten bzw. rechts (Details auf-/zugeklappt).
pub fn chevron(painter: &Painter, c: Pos2, open: bool, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    let points = if open {
        vec![c + vec2(-3.5, -1.8), c + vec2(0.0, 1.8), c + vec2(3.5, -1.8)]
    } else {
        vec![c + vec2(-1.8, -3.5), c + vec2(1.8, 0.0), c + vec2(-1.8, 3.5)]
    };
    painter.add(Shape::line(points, stroke));
}

fn arc(painter: &Painter, c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let steps = 24;
    let points = (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}
