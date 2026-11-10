//! Anmerkungen auf dem Screenshot. Jede ist ein eigener Typ hinter [`Annotation`]:
//! Sie zeichnet sich selbst auf eine [`Canvas`], kennt ihre Ausdehnung und lässt sich
//! verschieben. Neue Arten (z. B. Emoji, Lupe, Rahmen) brauchen nur ein neues `impl`
//! und ein Werkzeug, das sie erzeugt – siehe `tools.rs`.

use eframe::egui::{pos2, vec2, Color32, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};
use tiny_skia::BlendMode;

use super::canvas::{self, measure_text, Canvas, IRect};

pub trait Annotation: Send + Sync {
    /// Zeichnet die Anmerkung. Die Zeichenfläche kann ein Ausschnitt sein (Vorschau).
    fn render(&self, canvas: &mut Canvas);

    /// Bereich in Bildpixeln, den `render` verändern kann.
    fn bounds(&self) -> Rect;

    /// Liegt `p` auf der Anmerkung (für Radierer und Auswahl)?
    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        self.bounds().expand(tolerance).contains(p)
    }

    fn translate(&mut self, delta: Vec2);

    fn box_clone(&self) -> Box<dyn Annotation>;

    /// Nummer eines Schrittmarkers, damit der nächste weiterzählt.
    fn step_number(&self) -> Option<u32> {
        None
    }
}

impl Clone for Box<dyn Annotation> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// Wie ein Werkzeug zeichnet. Jedes Werkzeug merkt sich seinen eigenen Stil.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub color: [u8; 4],
    /// Strichstärke in Bildpixeln (Text: Schriftgröße).
    pub width: f32,
    pub filled: bool,
    pub redact: RedactMode,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: [0xE5, 0x39, 0x35, 0xFF],
            width: 4.0,
            filled: false,
            redact: RedactMode::Pixelate,
        }
    }
}

impl Style {
    pub fn color(&self) -> Color32 {
        let [r, g, b, a] = self.color;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

/// Punkt-Abstand zur Strecke a–b.
fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(p)
}

fn bounds_of(points: &[Pos2]) -> Rect {
    Rect::from_points(points)
}

// ---------------------------------------------------------------------------------------------
// Freihand (Stift und Textmarker)

#[derive(Clone)]
pub struct Freehand {
    pub points: Vec<Pos2>,
    pub color: Color32,
    pub width: f32,
    /// Textmarker: halbtransparent multipliziert, Text darunter bleibt lesbar.
    pub highlighter: bool,
}

impl Annotation for Freehand {
    fn render(&self, canvas: &mut Canvas) {
        let Some(path) = canvas::polyline(&self.points) else {
            return;
        };
        if self.highlighter {
            canvas.stroke_path(&path, self.color.gamma_multiply(0.55), self.width, BlendMode::Multiply);
        } else {
            canvas.stroke_path(&path, self.color, self.width, BlendMode::SourceOver);
        }
    }

    fn bounds(&self) -> Rect {
        bounds_of(&self.points).expand(self.width / 2.0 + 1.0)
    }

    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        let reach = self.width / 2.0 + tolerance;
        if self.points.len() == 1 {
            return self.points[0].distance(p) <= reach;
        }
        self.points.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= reach)
    }

    fn translate(&mut self, delta: Vec2) {
        self.points.iter_mut().for_each(|p| *p += delta);
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// Linie und Pfeil

#[derive(Clone)]
pub struct Line {
    pub from: Pos2,
    pub to: Pos2,
    pub color: Color32,
    pub width: f32,
    pub arrow: bool,
}

impl Line {
    fn head(&self) -> Option<[Pos2; 3]> {
        let dir = self.to - self.from;
        if !self.arrow || dir.length() < 1.0 {
            return None;
        }
        let dir = dir.normalized();
        let len = (self.width * 4.0).max(14.0).min((self.to - self.from).length());
        let half = len * 0.5;
        let normal = vec2(-dir.y, dir.x);
        let base = self.to - dir * len;
        Some([self.to, base + normal * half, base - normal * half])
    }
}

impl Annotation for Line {
    fn render(&self, canvas: &mut Canvas) {
        let end = match self.head() {
            // Die Linie endet in der Pfeilspitze, damit die runde Kappe nicht herausschaut.
            Some([tip, a, b]) => tip + ((a + b.to_vec2()) / 2.0 - tip) * 0.6,
            None => self.to,
        };
        if let Some(path) = canvas::polyline(&[self.from, end]) {
            canvas.stroke_path(&path, self.color, self.width, BlendMode::SourceOver);
        }
        if let Some(head) = self.head() {
            if let Some(path) = canvas::polygon(&head) {
                canvas.fill_path(&path, self.color, BlendMode::SourceOver);
                canvas.stroke_path(&path, self.color, (self.width * 0.5).max(1.0), BlendMode::SourceOver);
            }
        }
    }

    fn bounds(&self) -> Rect {
        let mut points = vec![self.from, self.to];
        if let Some(head) = self.head() {
            points.extend(head);
        }
        bounds_of(&points).expand(self.width + 1.0)
    }

    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        distance_to_segment(p, self.from, self.to) <= self.width / 2.0 + tolerance
    }

    fn translate(&mut self, delta: Vec2) {
        self.from += delta;
        self.to += delta;
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// Rechteck und Ellipse

#[derive(Clone, Copy, PartialEq)]
pub enum ShapeKind {
    Rect,
    Ellipse,
}

#[derive(Clone)]
pub struct Shape {
    pub kind: ShapeKind,
    pub rect: Rect,
    pub color: Color32,
    pub width: f32,
    pub filled: bool,
}

impl Shape {
    fn path(&self) -> Option<tiny_skia::Path> {
        match self.kind {
            ShapeKind::Rect => canvas::rect_path(self.rect, self.width.min(6.0)),
            ShapeKind::Ellipse => canvas::ellipse_path(self.rect),
        }
    }
}

impl Annotation for Shape {
    fn render(&self, canvas: &mut Canvas) {
        let Some(path) = self.path() else {
            return;
        };
        if self.filled {
            canvas.fill_path(&path, self.color, BlendMode::SourceOver);
        } else {
            canvas.stroke_path(&path, self.color, self.width, BlendMode::SourceOver);
        }
    }

    fn bounds(&self) -> Rect {
        self.rect.expand(self.width / 2.0 + 1.0)
    }

    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        let reach = self.width / 2.0 + tolerance;
        if self.filled {
            return self.bounds().expand(tolerance).contains(p);
        }
        match self.kind {
            ShapeKind::Rect => {
                self.rect.expand(reach).contains(p) && !self.rect.shrink(reach).contains(p)
            }
            ShapeKind::Ellipse => {
                let c = self.rect.center();
                let (rx, ry) = (self.rect.width() / 2.0, self.rect.height() / 2.0);
                if rx < 1.0 || ry < 1.0 {
                    return self.rect.expand(reach).contains(p);
                }
                let d = ((p.x - c.x) / rx).powi(2) + ((p.y - c.y) / ry).powi(2);
                (d.sqrt() - 1.0).abs() * rx.min(ry) <= reach
            }
        }
    }

    fn translate(&mut self, delta: Vec2) {
        self.rect = self.rect.translate(delta);
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// Text

#[derive(Clone)]
pub struct Text {
    pub pos: Pos2,
    pub text: String,
    pub size: f32,
    pub color: Color32,
    /// Mit hinterlegtem Kasten (gut lesbar auf unruhigem Hintergrund).
    pub boxed: bool,
}

impl Text {
    pub fn padding(&self) -> f32 {
        if self.boxed {
            self.size * 0.35
        } else {
            0.0
        }
    }

    fn text_rect(&self) -> Rect {
        Rect::from_min_size(self.pos, measure_text(&self.text, self.size))
    }
}

impl Annotation for Text {
    fn render(&self, canvas: &mut Canvas) {
        let mut color = self.color;
        if self.boxed {
            if let Some(path) = canvas::rect_path(self.text_rect().expand(self.padding()), self.size * 0.25) {
                canvas.fill_path(&path, self.color, BlendMode::SourceOver);
            }
            color = contrast_color(self.color);
        }
        canvas.text(self.pos, &self.text, self.size, color);
    }

    fn bounds(&self) -> Rect {
        self.text_rect().expand(self.padding() + 2.0)
    }

    fn translate(&mut self, delta: Vec2) {
        self.pos += delta;
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

/// Weiß oder Schwarz, je nachdem, was auf `color` besser lesbar ist.
pub fn contrast_color(color: Color32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 160.0 {
        Color32::from_rgb(0x15, 0x15, 0x18)
    } else {
        Color32::WHITE
    }
}

// ---------------------------------------------------------------------------------------------
// Schrittmarker ①②③

#[derive(Clone)]
pub struct Step {
    pub center: Pos2,
    pub number: u32,
    pub radius: f32,
    pub color: Color32,
}

impl Annotation for Step {
    fn render(&self, canvas: &mut Canvas) {
        if let Some(path) = canvas::circle_path(self.center, self.radius) {
            canvas.fill_path(&path, self.color, BlendMode::SourceOver);
            canvas.stroke_path(&path, Color32::WHITE, (self.radius * 0.12).max(1.5), BlendMode::SourceOver);
        }
        let label = self.number.to_string();
        let size = self.radius * if label.len() > 1 { 1.05 } else { 1.25 };
        let text = measure_text(&label, size);
        let pos = self.center - text / 2.0 - vec2(0.0, size * 0.04);
        canvas.text(pos, &label, size, contrast_color(self.color));
    }

    fn bounds(&self) -> Rect {
        Rect::from_center_size(self.center, Vec2::splat(self.radius * 2.0 + 4.0))
    }

    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        self.center.distance(p) <= self.radius + tolerance
    }

    fn translate(&mut self, delta: Vec2) {
        self.center += delta;
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }

    fn step_number(&self) -> Option<u32> {
        Some(self.number)
    }
}

// ---------------------------------------------------------------------------------------------
// Unkenntlich machen

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedactMode {
    Pixelate,
    Blur,
    /// Deckend schwärzen – die sicherste Variante, nichts lässt sich zurückrechnen.
    Black,
}

impl RedactMode {
    pub const ALL: [RedactMode; 3] = [RedactMode::Pixelate, RedactMode::Blur, RedactMode::Black];

    pub fn label(self) -> &'static str {
        match self {
            RedactMode::Pixelate => "Verpixeln",
            RedactMode::Blur => "Weichzeichnen",
            RedactMode::Black => "Schwärzen",
        }
    }
}

#[derive(Clone)]
pub struct Redact {
    pub rect: Rect,
    pub mode: RedactMode,
    /// Blockgröße bzw. Radius in Pixeln.
    pub strength: f32,
}

impl Redact {
    fn area(&self) -> IRect {
        IRect::round(self.rect)
    }
}

impl Annotation for Redact {
    fn render(&self, canvas: &mut Canvas) {
        let area = self.area();
        if area.w <= 0 || area.h <= 0 {
            return;
        }
        // Mindeststärke relativ zur Fläche, damit kleine Schrift wirklich unlesbar wird.
        let strength = self.strength.max(area.h.min(area.w) as f32 / 6.0).max(4.0);
        match self.mode {
            RedactMode::Pixelate => canvas.pixelate(area, strength.round() as i32),
            RedactMode::Blur => {
                // Vorher grob verpixeln: reiner Weichzeichner lässt sich teils zurückrechnen.
                canvas.pixelate(area, (strength / 2.0).round().max(2.0) as i32);
                canvas.blur(area, strength.round() as i32);
            }
            RedactMode::Black => {
                if let Some(path) = canvas::rect_path(area.to_rect(), 0.0) {
                    canvas.fill_path(&path, Color32::from_rgb(0x10, 0x10, 0x12), BlendMode::Source);
                }
            }
        }
    }

    fn bounds(&self) -> Rect {
        self.area().to_rect()
    }

    fn translate(&mut self, delta: Vec2) {
        self.rect = self.rect.translate(delta);
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

// ---------------------------------------------------------------------------------------------
// Spotlight: alles außer dem Bereich abdunkeln

#[derive(Clone)]
pub struct Spotlight {
    pub rect: Rect,
    pub ellipse: bool,
    /// Deckkraft der Abdunklung, 0–1.
    pub dim: f32,
}

impl Annotation for Spotlight {
    fn render(&self, canvas: &mut Canvas) {
        let path = if self.ellipse {
            canvas::ellipse_path(self.rect)
        } else {
            canvas::rect_path(self.rect, 6.0)
        };
        if let Some(path) = path {
            canvas.fill_outside(&path, Color32::from_black_alpha((self.dim.clamp(0.0, 1.0) * 255.0) as u8));
        }
    }

    /// Wirkt auf das ganze Bild.
    fn bounds(&self) -> Rect {
        Rect::EVERYTHING
    }

    fn hit(&self, p: Pos2, tolerance: f32) -> bool {
        let r = self.rect;
        r.expand(tolerance).contains(p) && !r.shrink(tolerance).contains(p)
    }

    fn translate(&mut self, delta: Vec2) {
        self.rect = self.rect.translate(delta);
    }

    fn box_clone(&self) -> Box<dyn Annotation> {
        Box::new(self.clone())
    }
}

/// Rechteck aus zwei Punkten; mit `square` quadratisch (Umschalt gedrückt).
pub fn drag_rect(a: Pos2, b: Pos2, square: bool) -> Rect {
    if !square {
        return Rect::from_two_pos(a, b);
    }
    let d = b - a;
    let side = d.x.abs().max(d.y.abs());
    Rect::from_two_pos(a, pos2(a.x + side * d.x.signum(), a.y + side * d.y.signum()))
}

/// Endpunkt auf 45°-Schritte eingerastet (Umschalt gedrückt).
pub fn snap_angle(a: Pos2, b: Pos2) -> Pos2 {
    let d = b - a;
    let angle = d.y.atan2(d.x);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    a + vec2(snapped.cos(), snapped.sin()) * d.length()
}
