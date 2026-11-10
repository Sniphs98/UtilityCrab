//! Zeichenfläche für Anmerkungen: ein Ausschnitt des Bildes (oder das ganze) als
//! tiny-skia-Pixmap. Alle Angaben sind in Bildkoordinaten; `origin` sagt, wo der
//! Ausschnitt im Bild liegt. So rendert dieselbe Anmerkung im fertigen Bild und in der
//! Live-Vorschau (nur ihr Bereich) Pixel für Pixel gleich.

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use eframe::egui::{pos2, vec2, Color32, Pos2, Rect, Vec2};
use image::RgbaImage;
use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8, Stroke,
    Transform,
};

pub struct Canvas {
    pub pixmap: Pixmap,
    /// Position der linken oberen Ecke im Bild.
    pub origin: (i32, i32),
}

impl Canvas {
    pub fn from_image(image: &RgbaImage) -> Self {
        let mut pixmap = Pixmap::new(image.width().max(1), image.height().max(1)).expect("Bildgröße");
        for (dst, src) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
            let [r, g, b, a] = src.0;
            *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
        }
        Self { pixmap, origin: (0, 0) }
    }

    /// Kopie des Bereichs `rect` (in Bildpixeln) als eigene Zeichenfläche.
    pub fn crop(&self, rect: IRect) -> Option<Canvas> {
        let full = IRect::new(self.origin.0, self.origin.1, self.pixmap.width() as i32, self.pixmap.height() as i32);
        let r = rect.intersect(&full)?;
        let mut pixmap = Pixmap::new(r.w as u32, r.h as u32)?;
        let src_w = self.pixmap.width() as usize;
        let src = self.pixmap.pixels();
        let dst = pixmap.pixels_mut();
        for y in 0..r.h as usize {
            let sy = (r.y - self.origin.1) as usize + y;
            let sx = (r.x - self.origin.0) as usize;
            dst[y * r.w as usize..(y + 1) * r.w as usize].copy_from_slice(&src[sy * src_w + sx..sy * src_w + sx + r.w as usize]);
        }
        Some(Canvas { pixmap, origin: (r.x, r.y) })
    }

    pub fn to_image(&self) -> RgbaImage {
        let mut out = RgbaImage::new(self.pixmap.width(), self.pixmap.height());
        for (dst, src) in out.pixels_mut().zip(self.pixmap.pixels()) {
            let c = src.demultiply();
            dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
        }
        out
    }

    /// Verschiebt Bildkoordinaten in den Ausschnitt.
    fn transform(&self) -> Transform {
        Transform::from_translate(-self.origin.0 as f32, -self.origin.1 as f32)
    }

    /// Bildbereich, den dieser Ausschnitt abdeckt.
    pub fn area(&self) -> IRect {
        IRect::new(self.origin.0, self.origin.1, self.pixmap.width() as i32, self.pixmap.height() as i32)
    }

    pub fn fill_path(&mut self, path: &Path, color: Color32, blend: BlendMode) {
        let paint = paint(color, blend);
        let t = self.transform();
        self.pixmap.fill_path(path, &paint, FillRule::Winding, t, None);
    }

    pub fn stroke_path(&mut self, path: &Path, color: Color32, width: f32, blend: BlendMode) {
        let paint = paint(color, blend);
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        let t = self.transform();
        self.pixmap.stroke_path(path, &paint, &stroke, t, None);
    }

    /// Füllt alles außer den `paths` (z. B. Spotlight) – in einem Zug, damit sich
    /// überlappende Flächen nicht doppelt abdunkeln.
    pub fn fill_outside(&mut self, paths: &[Path], color: Color32) {
        let Some(mut mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) else {
            return;
        };
        for path in paths {
            mask.fill_path(path, FillRule::Winding, true, self.transform());
        }
        mask.invert();
        let paint = paint(color, BlendMode::SourceOver);
        let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, self.pixmap.width() as f32, self.pixmap.height() as f32).unwrap();
        self.pixmap.fill_rect(rect, &paint, Transform::identity(), Some(&mask));
    }

    /// Pixelblöcke, ausgerichtet am Bild (nicht am Ausschnitt), damit Vorschau und Ergebnis gleich sind.
    pub fn pixelate(&mut self, rect: IRect, block: i32) {
        let block = block.max(2);
        let Some(r) = rect.intersect(&self.area()) else {
            return;
        };
        let (ox, oy) = self.origin;
        let width = self.pixmap.width() as usize;
        let data = self.pixmap.pixels_mut();
        let mut by = rect.y + ((r.y - rect.y) / block) * block;
        while by < r.y + r.h {
            let y0 = by.max(r.y);
            let y1 = (by + block).min(r.y + r.h).min(rect.y + rect.h);
            let mut bx = rect.x + ((r.x - rect.x) / block) * block;
            while bx < r.x + r.w {
                let x0 = bx.max(r.x);
                let x1 = (bx + block).min(r.x + r.w).min(rect.x + rect.w);
                let mut sum = [0u32; 4];
                let mut n = 0u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = data[(y - oy) as usize * width + (x - ox) as usize];
                        sum[0] += p.red() as u32;
                        sum[1] += p.green() as u32;
                        sum[2] += p.blue() as u32;
                        sum[3] += p.alpha() as u32;
                        n += 1;
                    }
                }
                if n > 0 {
                    let avg = PremultipliedColorU8::from_rgba(
                        (sum[0] / n) as u8,
                        (sum[1] / n) as u8,
                        (sum[2] / n) as u8,
                        (sum[3] / n) as u8,
                    )
                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                    for y in y0..y1 {
                        for x in x0..x1 {
                            data[(y - oy) as usize * width + (x - ox) as usize] = avg;
                        }
                    }
                }
                bx += block;
            }
            by += block;
        }
    }

    /// Starker Weichzeichner (dreifacher Box-Blur) nur mit Pixeln aus `rect`.
    pub fn blur(&mut self, rect: IRect, radius: i32) {
        let Some(r) = rect.intersect(&self.area()) else {
            return;
        };
        // Nur der sichtbare Teil wird geändert, gerechnet wird aber über das ganze Rechteck,
        // soweit es im Ausschnitt liegt. Liegt es nur teilweise darin, ist die Vorschau am Rand
        // minimal anders – das fertige Bild rendert immer das ganze Rechteck.
        let (ox, oy) = self.origin;
        let width = self.pixmap.width() as usize;
        let (w, h) = (r.w as usize, r.h as usize);
        let mut buf: Vec<[f32; 4]> = Vec::with_capacity(w * h);
        {
            let data = self.pixmap.pixels();
            for y in 0..h {
                for x in 0..w {
                    let p = data[(r.y - oy) as usize * width + y * width + (r.x - ox) as usize + x];
                    buf.push([p.red() as f32, p.green() as f32, p.blue() as f32, p.alpha() as f32]);
                }
            }
        }
        let radius = radius.max(1) as usize;
        let mut tmp = buf.clone();
        for _ in 0..3 {
            box_blur_line(&buf, &mut tmp, w, h, radius, true);
            box_blur_line(&tmp, &mut buf, w, h, radius, false);
        }
        let data = self.pixmap.pixels_mut();
        for y in 0..h {
            for x in 0..w {
                let [rr, g, b, a] = buf[y * w + x];
                let a = a.round().clamp(0.0, 255.0) as u8;
                let c = |v: f32| (v.round().clamp(0.0, 255.0) as u8).min(a);
                data[(r.y - oy) as usize * width + y * width + (r.x - ox) as usize + x] =
                    PremultipliedColorU8::from_rgba(c(rr), c(g), c(b), a).unwrap_or(PremultipliedColorU8::TRANSPARENT);
            }
        }
    }

    /// Überblendet zurück zum `original`: innerhalb von `inner` (mit runden Ecken `corner`)
    /// bleibt das jetzige Bild, außerhalb läuft es über `feather` Pixel weich ins Original aus.
    pub fn blend_soft_edge(&mut self, original: &Canvas, inner: Rect, corner: f32, feather: f32) {
        let Some(r) = original.area().intersect(&self.area()) else {
            return;
        };
        let center = inner.center();
        let half = inner.size() / 2.0;
        let feather = feather.max(1.0);
        let (ox, oy) = self.origin;
        let width = self.pixmap.width() as usize;
        let (sx, sy) = original.origin;
        let src_width = original.pixmap.width() as usize;
        let src = original.pixmap.pixels();
        let data = self.pixmap.pixels_mut();
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                // Abstand zum abgerundeten Rechteck (innen ≤ 0).
                let px = (x as f32 + 0.5 - center.x).abs() - half.x + corner;
                let py = (y as f32 + 0.5 - center.y).abs() - half.y + corner;
                let dist = vec2(px.max(0.0), py.max(0.0)).length() + px.max(py).min(0.0) - corner;
                let t = (dist / feather).clamp(0.0, 1.0);
                // Glatter Verlauf: 1 innen, 0 am äußeren Rand.
                let w = 1.0 - t * t * (3.0 - 2.0 * t);
                if w >= 1.0 {
                    continue;
                }
                let o = src[(y - sy) as usize * src_width + (x - sx) as usize];
                let d = &mut data[(y - oy) as usize * width + (x - ox) as usize];
                let mix = |a: u8, b: u8| (a as f32 * w + b as f32 * (1.0 - w)).round() as u8;
                let alpha = mix(d.alpha(), o.alpha());
                *d = PremultipliedColorU8::from_rgba(
                    mix(d.red(), o.red()).min(alpha),
                    mix(d.green(), o.green()).min(alpha),
                    mix(d.blue(), o.blue()).min(alpha),
                    alpha,
                )
                .unwrap_or(*d);
            }
        }
    }

    /// Schreibt Text (mehrzeilig) mit der linken oberen Ecke bei `pos`.
    pub fn text(&mut self, pos: Pos2, text: &str, size: f32, color: Color32) {
        let font = font();
        let scaled = font.as_scaled(PxScale::from(size));
        let line_height = scaled.height() + scaled.line_gap();
        let (ox, oy) = self.origin;
        let (pw, ph) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        let [cr, cg, cb, ca] = color.to_srgba_unmultiplied();
        let data = self.pixmap.pixels_mut();
        for (i, line) in text.split('\n').enumerate() {
            let mut x = pos.x;
            let baseline = pos.y + scaled.ascent() + i as f32 * line_height;
            let mut previous = None;
            for ch in line.chars() {
                let id = scaled.glyph_id(ch);
                if let Some(prev) = previous {
                    x += scaled.kern(prev, id);
                }
                let glyph = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(x, baseline));
                x += scaled.h_advance(id);
                previous = Some(id);
                let Some(outlined) = font.outline_glyph(glyph) else {
                    continue;
                };
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px = bounds.min.x as i32 + gx as i32 - ox;
                    let py = bounds.min.y as i32 + gy as i32 - oy;
                    if px < 0 || py < 0 || px >= pw || py >= ph {
                        return;
                    }
                    let a = coverage.clamp(0.0, 1.0) * ca as f32 / 255.0;
                    let dst = &mut data[(py * pw + px) as usize];
                    let inv = 1.0 - a;
                    let mix = |s: u8, d: u8| (s as f32 * a + d as f32 * inv).round() as u8;
                    let out_a = (255.0 * a + dst.alpha() as f32 * inv).round() as u8;
                    *dst = PremultipliedColorU8::from_rgba(
                        mix(cr, dst.red()).min(out_a),
                        mix(cg, dst.green()).min(out_a),
                        mix(cb, dst.blue()).min(out_a),
                        out_a,
                    )
                    .unwrap_or(*dst);
                });
            }
        }
    }
}

fn box_blur_line(src: &[[f32; 4]], dst: &mut [[f32; 4]], w: usize, h: usize, radius: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let index = |o: usize, i: usize| if horizontal { o * w + i } else { i * w + o };
    for o in 0..outer {
        let mut sum = [0f32; 4];
        let mut count = 0f32;
        // Fenster [i - radius, i + radius], an den Rändern abgeschnitten.
        for i in 0..=radius.min(inner - 1) {
            let p = src[index(o, i)];
            for c in 0..4 {
                sum[c] += p[c];
            }
            count += 1.0;
        }
        for i in 0..inner {
            for c in 0..4 {
                dst[index(o, i)][c] = sum[c] / count;
            }
            let add = i + radius + 1;
            if add < inner {
                let p = src[index(o, add)];
                for c in 0..4 {
                    sum[c] += p[c];
                }
                count += 1.0;
            }
            if i >= radius {
                let p = src[index(o, i - radius)];
                for c in 0..4 {
                    sum[c] -= p[c];
                }
                count -= 1.0;
            }
        }
    }
}

fn paint(color: Color32, blend: BlendMode) -> Paint<'static> {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    paint.blend_mode = blend;
    paint
}

// ---------------------------------------------------------------------------------------------
// Schrift

/// Segoe UI Semibold (lesbar auf Screenshots), sonst die eingebaute egui-Schrift.
pub fn font() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        let dir = std::path::PathBuf::from(windir).join("Fonts");
        for file in ["seguisb.ttf", "segoeui.ttf", "arial.ttf"] {
            if let Ok(bytes) = std::fs::read(dir.join(file)) {
                if let Ok(font) = FontArc::try_from_vec(bytes) {
                    return font;
                }
            }
        }
        let defaults = eframe::egui::FontDefinitions::default();
        let data = defaults
            .families
            .get(&eframe::egui::FontFamily::Proportional)
            .and_then(|names| names.first())
            .and_then(|name| defaults.font_data.get(name))
            .expect("egui bringt eine Schrift mit");
        FontArc::try_from_vec((*data.font).as_ref().to_vec()).expect("egui-Schrift lesbar")
    })
}

/// Größe eines (mehrzeiligen) Textes in Pixeln.
pub fn measure_text(text: &str, size: f32) -> Vec2 {
    let scaled = font().as_scaled(PxScale::from(size));
    let line_height = scaled.height() + scaled.line_gap();
    let mut width: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        lines += 1;
        let mut x = 0.0;
        let mut previous = None;
        for ch in line.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(prev) = previous {
                x += scaled.kern(prev, id);
            }
            x += scaled.h_advance(id);
            previous = Some(id);
        }
        width = width.max(x);
    }
    vec2(width, scaled.height() + (lines - 1) as f32 * line_height)
}

// ---------------------------------------------------------------------------------------------
// Geometrie

/// Ganzzahliges Rechteck in Bildpixeln.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Kleinstes Pixelrechteck, das `rect` ganz enthält.
    pub fn around(rect: Rect) -> Self {
        let x0 = rect.min.x.floor() as i32;
        let y0 = rect.min.y.floor() as i32;
        let x1 = rect.max.x.ceil() as i32;
        let y1 = rect.max.y.ceil() as i32;
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Gerundet (für Flächen, die genau auf Pixeln liegen sollen).
    pub fn round(rect: Rect) -> Self {
        let x0 = rect.min.x.round() as i32;
        let y0 = rect.min.y.round() as i32;
        let x1 = rect.max.x.round() as i32;
        let y1 = rect.max.y.round() as i32;
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn intersect(&self, other: &IRect) -> Option<IRect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        (x1 > x0 && y1 > y0).then_some(IRect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn to_rect(self) -> Rect {
        Rect::from_min_size(pos2(self.x as f32, self.y as f32), vec2(self.w as f32, self.h as f32))
    }
}

// ---------------------------------------------------------------------------------------------
// Pfade

pub fn polyline(points: &[Pos2]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let first = points.first()?;
    pb.move_to(first.x, first.y);
    if points.len() == 1 {
        // Ein einzelner Klick soll einen Punkt ergeben.
        pb.line_to(first.x + 0.01, first.y);
    }
    for p in &points[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

pub fn polygon(points: &[Pos2]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let first = points.first()?;
    pb.move_to(first.x, first.y);
    for p in &points[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.close();
    pb.finish()
}

pub fn rect_path(rect: Rect, radius: f32) -> Option<Path> {
    if radius <= 0.0 {
        return PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(rect.min.x, rect.min.y, rect.max.x, rect.max.y)?)
            .into();
    }
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let (l, t, rr, b) = (rect.min.x, rect.min.y, rect.max.x, rect.max.y);
    // Kreisbogen-Näherung mit kubischen Kurven.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(l + r, t);
    pb.line_to(rr - r, t);
    pb.cubic_to(rr - r + k, t, rr, t + r - k, rr, t + r);
    pb.line_to(rr, b - r);
    pb.cubic_to(rr, b - r + k, rr - r + k, b, rr - r, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r - k, b, l, b - r + k, l, b - r);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r - k, l + r - k, t, l + r, t);
    pb.close();
    pb.finish()
}

pub fn ellipse_path(rect: Rect) -> Option<Path> {
    PathBuilder::from_oval(tiny_skia::Rect::from_ltrb(rect.min.x, rect.min.y, rect.max.x, rect.max.y)?)
}

pub fn circle_path(center: Pos2, radius: f32) -> Option<Path> {
    PathBuilder::from_circle(center.x, center.y, radius)
}
