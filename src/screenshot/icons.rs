//! Kleine Strichsymbole für Werkzeugleiste und Aufnahme-Leiste, mit dem Painter gezeichnet
//! (keine Bilddateien, gestochen scharf bei jeder Skalierung). Entworfen auf 20×20 Punkten.

use eframe::egui::{pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    // Werkzeuge
    Select,
    Pen,
    Highlighter,
    Eraser,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Text,
    Step,
    Redact,
    Spotlight,
    Crop,
    // Aktionen
    New,
    Open,
    Undo,
    Redo,
    Copy,
    Save,
    OcrText,
    AutoRedact,
    Pin,
    // Aufnahmearten
    ModeRect,
    ModeWindow,
    ModeScreen,
    ModeFreeform,
    Close,
}

pub fn paint(glyph: Glyph, painter: &Painter, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height()) / 20.0;
    let c = rect.center();
    let p = |x: f32, y: f32| c + vec2(x, y) * s;
    let stroke = Stroke::new(1.5 * s, color);
    let line = |points: &[Pos2]| painter.add(Shape::line(points.to_vec(), stroke));
    let rect_at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(p(x, y), vec2(w, h) * s);

    match glyph {
        Glyph::Select => {
            let points = vec![p(-4.0, -7.0), p(-4.0, 6.0), p(-0.8, 3.0), p(1.6, 8.0), p(3.6, 7.0), p(1.2, 2.2), p(5.5, 2.0)];
            painter.add(Shape::closed_line(points, stroke));
        }
        Glyph::Pen => {
            line(&[p(-6.0, 6.0), p(-5.0, 2.5), p(4.0, -6.5), p(6.5, -4.0), p(-2.5, 5.0), p(-6.0, 6.0)]);
            line(&[p(2.0, -4.5), p(4.5, -2.0)]);
        }
        Glyph::Highlighter => {
            line(&[p(-3.0, 4.0), p(-5.0, 2.0), p(3.0, -6.0), p(5.0, -4.0), p(-3.0, 4.0)]);
            line(&[p(-5.0, 2.0), p(-6.5, 5.5), p(-3.0, 4.0)]);
            painter.line_segment([p(-7.0, 7.5), p(7.0, 7.5)], Stroke::new(2.5 * s, Color32::from_rgb(0xFF, 0xD8, 0x3D)));
        }
        Glyph::Eraser => {
            line(&[p(-7.0, 2.0), p(1.0, -6.0), p(7.0, 0.0), p(1.0, 6.0), p(-3.0, 6.0), p(-7.0, 2.0)]);
            line(&[p(-3.0, -2.0), p(3.0, 4.0)]);
            line(&[p(1.0, 6.0), p(7.0, 6.0)]);
        }
        Glyph::Arrow => {
            line(&[p(-6.0, 6.0), p(5.0, -5.0)]);
            painter.add(Shape::convex_polygon(vec![p(6.5, -6.5), p(0.0, -5.0), p(5.0, 0.0)], color, Stroke::NONE));
        }
        Glyph::Line => {
            line(&[p(-6.5, 6.5), p(6.5, -6.5)]);
        }
        Glyph::Rect => {
            painter.rect_stroke(rect_at(-7.0, -5.5, 14.0, 11.0), CornerRadius::same((2.0 * s) as u8), stroke, StrokeKind::Middle);
        }
        Glyph::Ellipse => {
            ellipse(painter, c, vec2(7.0, 5.5) * s, stroke);
        }
        Glyph::Text => {
            line(&[p(-6.0, -6.0), p(6.0, -6.0)]);
            line(&[p(0.0, -6.0), p(0.0, 7.0)]);
            line(&[p(-2.5, 7.0), p(2.5, 7.0)]);
        }
        Glyph::Step => {
            painter.circle_stroke(c, 7.0 * s, stroke);
            painter.text(c + vec2(0.0, 0.3 * s), eframe::egui::Align2::CENTER_CENTER, "1", crate::ui_kit::semibold(10.0 * s), color);
        }
        Glyph::Redact => {
            let r = rect_at(-7.0, -6.0, 14.0, 12.0);
            painter.rect_stroke(r, CornerRadius::same((1.5 * s) as u8), stroke, StrokeKind::Middle);
            for (i, (x, y)) in [(0, 0), (2, 0), (1, 1), (3, 1), (0, 2), (2, 2)].iter().enumerate() {
                let cell = Rect::from_min_size(r.min + vec2(*x as f32, *y as f32) * 3.5 * s + vec2(0.0, 0.0), vec2(3.5, 4.0) * s);
                let alpha = if i % 2 == 0 { 1.0 } else { 0.6 };
                painter.rect_filled(cell.shrink(0.3 * s), 0.0, color.gamma_multiply(alpha));
            }
        }
        Glyph::Spotlight => {
            painter.rect_filled(rect_at(-8.0, -7.0, 16.0, 14.0), CornerRadius::same((2.0 * s) as u8), color.gamma_multiply(0.35));
            painter.circle_filled(c, 4.5 * s, color);
        }
        Glyph::Crop => {
            line(&[p(-4.0, -8.0), p(-4.0, 4.0), p(8.0, 4.0)]);
            line(&[p(-8.0, -4.0), p(4.0, -4.0), p(4.0, 8.0)]);
        }
        Glyph::New => {
            line(&[p(0.0, -6.5), p(0.0, 6.5)]);
            line(&[p(-6.5, 0.0), p(6.5, 0.0)]);
        }
        Glyph::Open => {
            line(&[p(-7.0, 6.0), p(-7.0, -6.0), p(-2.5, -6.0), p(-1.0, -4.0), p(6.0, -4.0), p(6.0, -1.0)]);
            line(&[p(-7.0, 6.0), p(-4.5, -1.0), p(8.0, -1.0), p(5.5, 6.0), p(-7.0, 6.0)]);
        }
        Glyph::Undo | Glyph::Redo => {
            let flip = if glyph == Glyph::Redo { -1.0 } else { 1.0 };
            let q = |x: f32, y: f32| p(x * flip, y);
            let mut points = vec![q(-5.0, -2.0)];
            for i in 0..=10 {
                let a = std::f32::consts::PI * (-0.5 + i as f32 / 10.0);
                points.push(q(1.0 + 5.0 * a.cos(), 2.0 + 5.0 * a.sin()));
            }
            points.push(q(-3.0, 7.0));
            line(&points);
            line(&[q(-2.0, -5.5), q(-5.5, -2.0), q(-2.0, 1.5)]);
        }
        Glyph::Copy => {
            painter.rect_stroke(rect_at(-3.5, -3.5, 10.0, 11.0), CornerRadius::same((1.5 * s) as u8), stroke, StrokeKind::Middle);
            line(&[p(-6.5, 4.0), p(-6.5, -7.5), p(3.0, -7.5)]);
        }
        Glyph::Save => {
            line(&[p(0.0, -7.0), p(0.0, 3.0)]);
            line(&[p(-4.0, -1.0), p(0.0, 3.0), p(4.0, -1.0)]);
            line(&[p(-7.0, 3.0), p(-7.0, 7.0), p(7.0, 7.0), p(7.0, 3.0)]);
        }
        Glyph::OcrText => {
            for (a, b) in [((-7.0, -3.0), (-7.0, -7.0)), ((-7.0, -7.0), (-3.0, -7.0))] {
                line(&[p(a.0, a.1), p(b.0, b.1)]);
            }
            line(&[p(3.0, -7.0), p(7.0, -7.0), p(7.0, -3.0)]);
            line(&[p(7.0, 3.0), p(7.0, 7.0), p(3.0, 7.0)]);
            line(&[p(-3.0, 7.0), p(-7.0, 7.0), p(-7.0, 3.0)]);
            line(&[p(-3.5, -2.0), p(3.5, -2.0)]);
            line(&[p(-3.5, 1.0), p(3.5, 1.0)]);
            line(&[p(-3.5, 4.0), p(1.0, 4.0)]);
        }
        Glyph::AutoRedact => {
            painter.rect_filled(rect_at(-7.5, -5.0, 9.0, 3.5), 1.0, color);
            painter.rect_filled(rect_at(-7.5, 1.5, 6.0, 3.5), 1.0, color);
            // Funkeln: „automatisch“.
            let star = p(4.5, -1.5);
            for (dx, dy) in [(0.0, -4.5), (0.0, 4.5), (-4.5, 0.0), (4.5, 0.0)] {
                painter.line_segment([star, star + vec2(dx, dy) * s], stroke);
            }
        }
        Glyph::Pin => {
            line(&[p(-2.0, -7.0), p(6.0, 1.0)]);
            line(&[p(-3.5, -3.0), p(2.0, 2.5)]);
            line(&[p(0.0, -5.0), p(-5.0, 0.0), p(4.0, 3.0), p(0.0, -5.0)]);
            line(&[p(-1.0, 3.0), p(-7.0, 9.0)]);
        }
        Glyph::ModeRect => {
            painter.add(Shape::dashed_line(
                &[p(-7.0, -5.5), p(7.0, -5.5), p(7.0, 5.5), p(-7.0, 5.5), p(-7.0, -5.5)],
                stroke,
                2.5 * s,
                2.0 * s,
            ));
        }
        Glyph::ModeWindow => {
            painter.rect_stroke(rect_at(-7.5, -6.0, 15.0, 12.0), CornerRadius::same((1.5 * s) as u8), stroke, StrokeKind::Middle);
            line(&[p(-7.5, -2.5), p(7.5, -2.5)]);
        }
        Glyph::ModeScreen => {
            painter.rect_stroke(rect_at(-8.0, -6.5, 16.0, 10.5), CornerRadius::same((1.5 * s) as u8), stroke, StrokeKind::Middle);
            line(&[p(-3.0, 7.0), p(3.0, 7.0)]);
            line(&[p(0.0, 4.0), p(0.0, 7.0)]);
        }
        Glyph::ModeFreeform => {
            let points: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    let r = 6.0 + 1.6 * (a * 3.0).sin();
                    p(r * a.cos(), r * 0.8 * a.sin())
                })
                .collect();
            painter.add(Shape::dashed_line(&points, stroke, 2.5 * s, 2.0 * s));
        }
        Glyph::Close => {
            line(&[p(-5.0, -5.0), p(5.0, 5.0)]);
            line(&[p(5.0, -5.0), p(-5.0, 5.0)]);
        }
    }
}

fn ellipse(painter: &Painter, c: Pos2, radius: eframe::egui::Vec2, stroke: Stroke) {
    let points = (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            pos2(c.x + radius.x * a.cos(), c.y + radius.y * a.sin())
        })
        .collect();
    painter.add(Shape::closed_line(points, stroke));
}

/// Quadratischer Knopf mit Symbol; `selected` hinterlegt ihn in Akzentfarbe.
pub fn button(ui: &mut eframe::egui::Ui, glyph: Glyph, selected: bool, enabled: bool, tooltip: &str) -> eframe::egui::Response {
    use eframe::egui::Sense;
    let size = vec2(32.0, 30.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    let radius = CornerRadius::same(6);
    if selected {
        painter.rect_filled(rect, radius, crate::ui_kit::tint(crate::ui_kit::ACCENT, 0.28));
        painter.rect_stroke(rect, radius, Stroke::new(1.0, crate::ui_kit::tint(crate::ui_kit::ACCENT, 0.7)), StrokeKind::Inside);
    } else if enabled && response.hovered() {
        painter.rect_filled(rect, radius, Color32::from_rgb(0x3A, 0x3C, 0x42));
    }
    let color = if !enabled {
        crate::ui_kit::TEXT_FAINT.gamma_multiply(0.6)
    } else if selected {
        Color32::from_rgb(0xFF, 0xB0, 0x7A)
    } else {
        crate::ui_kit::TEXT
    };
    paint(glyph, painter, Rect::from_center_size(rect.center(), vec2(20.0, 20.0)), color);
    let response = response.on_hover_text(tooltip);
    if enabled {
        response.on_hover_cursor(eframe::egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
