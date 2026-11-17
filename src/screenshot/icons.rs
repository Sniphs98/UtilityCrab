//! Kleine Strichsymbole für Werkzeugleiste und Aufnahme-Leiste, mit dem Painter gezeichnet
//! (keine Bilddateien, gestochen scharf bei jeder Skalierung). Entworfen auf 20×20 Punkten.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

use crate::geom::{pos2, vec2, Color32, Pos2, Rect};
use crate::ui_kit::{painted, tint, Painter, ACCENT, TEXT, TEXT_FAINT};

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

pub fn paint(glyph: Glyph, painter: &mut Painter, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height()) / 20.0;
    let c = rect.center();
    let p = |x: f32, y: f32| c + vec2(x, y) * s;
    let width = 1.5 * s;
    let rect_at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(p(x, y), vec2(w, h) * s);
    macro_rules! stroke {
        ($points:expr) => {
            painter.line($points, width, color)
        };
    }

    match glyph {
        Glyph::Select => {
            let points = [p(-4.0, -7.0), p(-4.0, 6.0), p(-0.8, 3.0), p(1.6, 8.0), p(3.6, 7.0), p(1.2, 2.2), p(5.5, 2.0)];
            painter.closed_line(&points, width, color);
        }
        Glyph::Pen => {
            stroke!(&[p(-6.0, 6.0), p(-5.0, 2.5), p(4.0, -6.5), p(6.5, -4.0), p(-2.5, 5.0), p(-6.0, 6.0)]);
            stroke!(&[p(2.0, -4.5), p(4.5, -2.0)]);
        }
        Glyph::Highlighter => {
            stroke!(&[p(-3.0, 4.0), p(-5.0, 2.0), p(3.0, -6.0), p(5.0, -4.0), p(-3.0, 4.0)]);
            stroke!(&[p(-5.0, 2.0), p(-6.5, 5.5), p(-3.0, 4.0)]);
            painter.line(&[p(-7.0, 7.5), p(7.0, 7.5)], 2.5 * s, Color32::from_rgb(0xFF, 0xD8, 0x3D));
        }
        Glyph::Eraser => {
            stroke!(&[p(-7.0, 2.0), p(1.0, -6.0), p(7.0, 0.0), p(1.0, 6.0), p(-3.0, 6.0), p(-7.0, 2.0)]);
            stroke!(&[p(-3.0, -2.0), p(3.0, 4.0)]);
            stroke!(&[p(1.0, 6.0), p(7.0, 6.0)]);
        }
        Glyph::Arrow => {
            stroke!(&[p(-6.0, 6.0), p(5.0, -5.0)]);
            painter.polygon(&[p(6.5, -6.5), p(0.0, -5.0), p(5.0, 0.0)], color);
        }
        Glyph::Line => {
            stroke!(&[p(-6.5, 6.5), p(6.5, -6.5)]);
        }
        Glyph::Rect => {
            painter.rect_stroke(rect_at(-7.0, -5.5, 14.0, 11.0), 2.0 * s, width, color);
        }
        Glyph::Ellipse => {
            ellipse(painter, c, vec2(7.0, 5.5) * s, width, color);
        }
        Glyph::Text => {
            stroke!(&[p(-6.0, -6.0), p(6.0, -6.0)]);
            stroke!(&[p(0.0, -6.0), p(0.0, 7.0)]);
            stroke!(&[p(-2.5, 7.0), p(2.5, 7.0)]);
        }
        Glyph::Step => {
            painter.circle_stroke(c, 7.0 * s, width, color);
            // Eine „1“ aus Linien – unabhängig von der Schrift.
            painter.line(&[p(-1.8, -2.2), p(0.6, -4.0), p(0.6, 4.0)], width, color);
        }
        Glyph::Redact => {
            let r = rect_at(-7.0, -6.0, 14.0, 12.0);
            painter.rect_stroke(r, 1.5 * s, width, color);
            for (i, (x, y)) in [(0, 0), (2, 0), (1, 1), (3, 1), (0, 2), (2, 2)].iter().enumerate() {
                let cell = Rect::from_min_size(r.min + vec2(*x as f32, *y as f32) * 3.5 * s + vec2(0.0, 0.0), vec2(3.5, 4.0) * s);
                let alpha = if i % 2 == 0 { 1.0 } else { 0.6 };
                painter.rect_filled(cell.shrink(0.3 * s), 0.0, color.gamma_multiply(alpha));
            }
        }
        Glyph::Spotlight => {
            painter.rect_filled(rect_at(-8.0, -7.0, 16.0, 14.0), 2.0 * s, color.gamma_multiply(0.35));
            painter.circle_filled(c, 4.5 * s, color);
        }
        Glyph::Crop => {
            stroke!(&[p(-4.0, -8.0), p(-4.0, 4.0), p(8.0, 4.0)]);
            stroke!(&[p(-8.0, -4.0), p(4.0, -4.0), p(4.0, 8.0)]);
        }
        Glyph::New => {
            stroke!(&[p(0.0, -6.5), p(0.0, 6.5)]);
            stroke!(&[p(-6.5, 0.0), p(6.5, 0.0)]);
        }
        Glyph::Open => {
            stroke!(&[p(-7.0, 6.0), p(-7.0, -6.0), p(-2.5, -6.0), p(-1.0, -4.0), p(6.0, -4.0), p(6.0, -1.0)]);
            stroke!(&[p(-7.0, 6.0), p(-4.5, -1.0), p(8.0, -1.0), p(5.5, 6.0), p(-7.0, 6.0)]);
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
            stroke!(&points);
            stroke!(&[q(-2.0, -5.5), q(-5.5, -2.0), q(-2.0, 1.5)]);
        }
        Glyph::Copy => {
            painter.rect_stroke(rect_at(-3.5, -3.5, 10.0, 11.0), 1.5 * s, width, color);
            stroke!(&[p(-6.5, 4.0), p(-6.5, -7.5), p(3.0, -7.5)]);
        }
        Glyph::Save => {
            stroke!(&[p(0.0, -7.0), p(0.0, 3.0)]);
            stroke!(&[p(-4.0, -1.0), p(0.0, 3.0), p(4.0, -1.0)]);
            stroke!(&[p(-7.0, 3.0), p(-7.0, 7.0), p(7.0, 7.0), p(7.0, 3.0)]);
        }
        Glyph::OcrText => {
            for (a, b) in [((-7.0, -3.0), (-7.0, -7.0)), ((-7.0, -7.0), (-3.0, -7.0))] {
                stroke!(&[p(a.0, a.1), p(b.0, b.1)]);
            }
            stroke!(&[p(3.0, -7.0), p(7.0, -7.0), p(7.0, -3.0)]);
            stroke!(&[p(7.0, 3.0), p(7.0, 7.0), p(3.0, 7.0)]);
            stroke!(&[p(-3.0, 7.0), p(-7.0, 7.0), p(-7.0, 3.0)]);
            stroke!(&[p(-3.5, -2.0), p(3.5, -2.0)]);
            stroke!(&[p(-3.5, 1.0), p(3.5, 1.0)]);
            stroke!(&[p(-3.5, 4.0), p(1.0, 4.0)]);
        }
        Glyph::AutoRedact => {
            painter.rect_filled(rect_at(-7.5, -5.0, 9.0, 3.5), 1.0, color);
            painter.rect_filled(rect_at(-7.5, 1.5, 6.0, 3.5), 1.0, color);
            // Funkeln: „automatisch“.
            let star = p(4.5, -1.5);
            for (dx, dy) in [(0.0, -4.5), (0.0, 4.5), (-4.5, 0.0), (4.5, 0.0)] {
                painter.line(&[star, star + vec2(dx, dy) * s], width, color);
            }
        }
        Glyph::Pin => {
            stroke!(&[p(-2.0, -7.0), p(6.0, 1.0)]);
            stroke!(&[p(-3.5, -3.0), p(2.0, 2.5)]);
            stroke!(&[p(0.0, -5.0), p(-5.0, 0.0), p(4.0, 3.0), p(0.0, -5.0)]);
            stroke!(&[p(-1.0, 3.0), p(-7.0, 9.0)]);
        }
        Glyph::ModeRect => {
            painter.dashed_line(
                &[p(-7.0, -5.5), p(7.0, -5.5), p(7.0, 5.5), p(-7.0, 5.5), p(-7.0, -5.5)],
                width,
                color,
                2.5 * s,
                2.0 * s,
            );
        }
        Glyph::ModeWindow => {
            painter.rect_stroke(rect_at(-7.5, -6.0, 15.0, 12.0), 1.5 * s, width, color);
            stroke!(&[p(-7.5, -2.5), p(7.5, -2.5)]);
        }
        Glyph::ModeScreen => {
            painter.rect_stroke(rect_at(-8.0, -6.5, 16.0, 10.5), 1.5 * s, width, color);
            stroke!(&[p(-3.0, 7.0), p(3.0, 7.0)]);
            stroke!(&[p(0.0, 4.0), p(0.0, 7.0)]);
        }
        Glyph::ModeFreeform => {
            let points: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    let r = 6.0 + 1.6 * (a * 3.0).sin();
                    p(r * a.cos(), r * 0.8 * a.sin())
                })
                .collect();
            painter.dashed_line(&points, width, color, 2.5 * s, 2.0 * s);
        }
        Glyph::Close => {
            stroke!(&[p(-5.0, -5.0), p(5.0, 5.0)]);
            stroke!(&[p(5.0, -5.0), p(-5.0, 5.0)]);
        }
    }
}

fn ellipse(painter: &mut Painter, c: Pos2, radius: crate::geom::Vec2, width: f32, color: Color32) {
    let points: Vec<Pos2> = (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            pos2(c.x + radius.x * a.cos(), c.y + radius.y * a.sin())
        })
        .collect();
    painter.closed_line(&points, width, color);
}

/// Quadratischer Knopf mit Symbol; `selected` hinterlegt ihn in Akzentfarbe.
/// Den Klick hängt der Aufrufer mit `.on_click` an (nur wenn `enabled`).
pub fn button(
    id: impl Into<ElementId>,
    glyph: Glyph,
    selected: bool,
    enabled: bool,
    tooltip: impl Into<SharedString>,
) -> Stateful<Div> {
    let tooltip: SharedString = tooltip.into();
    let color = if !enabled {
        TEXT_FAINT.gamma_multiply(0.6)
    } else if selected {
        Color32::from_rgb(0xFF, 0xB0, 0x7A)
    } else {
        TEXT
    };
    let mut el = div()
        .id(id)
        .flex_none()
        .w(px(32.0))
        .h(px(30.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(Color32::TRANSPARENT)
        .flex()
        .items_center()
        .justify_center()
        .child(painted(move |p, rect| paint(glyph, p, Rect::from_center_size(rect.center(), vec2(20.0, 20.0)), color)).size(px(20.0)))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
    if selected {
        el = el.bg(tint(ACCENT, 0.28)).border_color(tint(ACCENT, 0.7));
    } else if enabled {
        el = el.hover(|s| s.bg(Color32::from_rgb(0x3A, 0x3C, 0x42)));
    }
    if enabled {
        el = el.cursor_pointer();
    }
    el
}
