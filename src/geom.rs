//! Kleine 2D-Geometrie und Farben für Bildbearbeitung und selbst gezeichnete Symbole.
//!
//! Unabhängig von der Oberfläche: Anmerkungen, Zeichenfläche und Werkzeuge rechnen damit
//! in Bildpixeln, die Fenster (GPUI) bekommen sie über die `From`-Umwandlungen unten.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

// ---------------------------------------------------------------------------------------------
// Punkte und Vektoren

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

pub const fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2 { x, y }
}

pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Pos2 {
    pub const ZERO: Pos2 = pos2(0.0, 0.0);

    pub fn distance(self, other: Pos2) -> f32 {
        (self - other).length()
    }

    pub fn to_vec2(self) -> Vec2 {
        vec2(self.x, self.y)
    }
}

impl Vec2 {
    pub const ZERO: Vec2 = vec2(0.0, 0.0);

    pub const fn new(x: f32, y: f32) -> Self {
        vec2(x, y)
    }

    pub const fn splat(v: f32) -> Self {
        vec2(v, v)
    }

    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn normalized(self) -> Vec2 {
        let len = self.length();
        if len > 0.0 {
            self / len
        } else {
            self
        }
    }

    pub fn dot(self, other: Vec2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    pub fn to_pos2(self) -> Pos2 {
        pos2(self.x, self.y)
    }
}

impl Add<Vec2> for Pos2 {
    type Output = Pos2;
    fn add(self, v: Vec2) -> Pos2 {
        pos2(self.x + v.x, self.y + v.y)
    }
}

impl AddAssign<Vec2> for Pos2 {
    fn add_assign(&mut self, v: Vec2) {
        self.x += v.x;
        self.y += v.y;
    }
}

impl Sub<Vec2> for Pos2 {
    type Output = Pos2;
    fn sub(self, v: Vec2) -> Pos2 {
        pos2(self.x - v.x, self.y - v.y)
    }
}

impl SubAssign<Vec2> for Pos2 {
    fn sub_assign(&mut self, v: Vec2) {
        self.x -= v.x;
        self.y -= v.y;
    }
}

impl Sub for Pos2 {
    type Output = Vec2;
    fn sub(self, other: Pos2) -> Vec2 {
        vec2(self.x - other.x, self.y - other.y)
    }
}

/// Punkte durch eine Zahl teilen (z. B. physische Pixel → Bildschirmpunkte).
impl Div<f32> for Pos2 {
    type Output = Pos2;
    fn div(self, f: f32) -> Pos2 {
        pos2(self.x / f, self.y / f)
    }
}

impl Mul<f32> for Pos2 {
    type Output = Pos2;
    fn mul(self, f: f32) -> Pos2 {
        pos2(self.x * f, self.y * f)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, v: Vec2) -> Vec2 {
        vec2(self.x + v.x, self.y + v.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, v: Vec2) {
        self.x += v.x;
        self.y += v.y;
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, v: Vec2) -> Vec2 {
        vec2(self.x - v.x, self.y - v.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        vec2(-self.x, -self.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, f: f32) -> Vec2 {
        vec2(self.x * f, self.y * f)
    }
}

/// Komponentenweise, z. B. für Größen mit unterschiedlichem Maßstab.
impl Mul<Vec2> for Vec2 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        vec2(self.x * v.x, self.y * v.y)
    }
}

impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, f: f32) -> Vec2 {
        vec2(self.x / f, self.y / f)
    }
}

// ---------------------------------------------------------------------------------------------
// Rechtecke

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Pos2,
    pub max: Pos2,
}

impl Rect {
    /// Alles (z. B. für Anmerkungen, die das ganze Bild betreffen).
    pub const EVERYTHING: Rect = Rect {
        min: pos2(-f32::INFINITY, -f32::INFINITY),
        max: pos2(f32::INFINITY, f32::INFINITY),
    };
    /// Leer; Vereinigungen damit ergeben das andere Rechteck.
    pub const NOTHING: Rect = Rect {
        min: pos2(f32::INFINITY, f32::INFINITY),
        max: pos2(-f32::INFINITY, -f32::INFINITY),
    };

    pub const fn from_min_max(min: Pos2, max: Pos2) -> Self {
        Rect { min, max }
    }

    pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
        Rect { min, max: min + size }
    }

    pub fn from_center_size(center: Pos2, size: Vec2) -> Self {
        Rect {
            min: center - size / 2.0,
            max: center + size / 2.0,
        }
    }

    /// Aus zwei beliebigen Ecken.
    pub fn from_two_pos(a: Pos2, b: Pos2) -> Self {
        Rect {
            min: pos2(a.x.min(b.x), a.y.min(b.y)),
            max: pos2(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Kleinstes Rechteck um alle Punkte.
    pub fn from_points(points: &[Pos2]) -> Self {
        points.iter().fold(Rect::NOTHING, |r, p| Rect {
            min: pos2(r.min.x.min(p.x), r.min.y.min(p.y)),
            max: pos2(r.max.x.max(p.x), r.max.y.max(p.y)),
        })
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn center(&self) -> Pos2 {
        pos2((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }

    pub fn left(&self) -> f32 {
        self.min.x
    }

    pub fn right(&self) -> f32 {
        self.max.x
    }

    pub fn top(&self) -> f32 {
        self.min.y
    }

    pub fn bottom(&self) -> f32 {
        self.max.y
    }

    pub fn left_top(&self) -> Pos2 {
        self.min
    }

    pub fn right_top(&self) -> Pos2 {
        pos2(self.max.x, self.min.y)
    }

    pub fn left_bottom(&self) -> Pos2 {
        pos2(self.min.x, self.max.y)
    }

    pub fn right_bottom(&self) -> Pos2 {
        self.max
    }

    pub fn left_center(&self) -> Pos2 {
        pos2(self.min.x, self.center().y)
    }

    pub fn right_center(&self) -> Pos2 {
        pos2(self.max.x, self.center().y)
    }

    /// Hat das Rechteck eine echte Fläche?
    pub fn is_positive(&self) -> bool {
        self.min.x < self.max.x && self.min.y < self.max.y
    }

    pub fn contains(&self, p: Pos2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn expand(&self, amount: f32) -> Rect {
        Rect {
            min: self.min - Vec2::splat(amount),
            max: self.max + Vec2::splat(amount),
        }
    }

    pub fn shrink(&self, amount: f32) -> Rect {
        self.expand(-amount)
    }

    pub fn translate(&self, delta: Vec2) -> Rect {
        Rect {
            min: self.min + delta,
            max: self.max + delta,
        }
    }

    pub fn intersect(&self, other: Rect) -> Rect {
        Rect {
            min: pos2(self.min.x.max(other.min.x), self.min.y.max(other.min.y)),
            max: pos2(self.max.x.min(other.max.x), self.max.y.min(other.max.y)),
        }
    }

    pub fn union(&self, other: Rect) -> Rect {
        Rect {
            min: pos2(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: pos2(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Farben

/// sRGB-Farbe mit (nicht vormultipliziertem) Alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color32 {
    pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);
    pub const BLACK: Color32 = Color32::from_rgb(0, 0, 0);
    pub const TRANSPARENT: Color32 = Color32::from_rgba_unmultiplied(0, 0, 0, 0);

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_gray(v: u8) -> Self {
        Self::from_rgb(v, v, v)
    }

    pub const fn from_black_alpha(a: u8) -> Self {
        Self::from_rgba_unmultiplied(0, 0, 0, a)
    }

    pub const fn from_white_alpha(a: u8) -> Self {
        Self::from_rgba_unmultiplied(255, 255, 255, a)
    }

    pub fn r(&self) -> u8 {
        self.r
    }

    pub fn g(&self) -> u8 {
        self.g
    }

    pub fn b(&self) -> u8 {
        self.b
    }

    pub fn to_array(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn to_srgba_unmultiplied(&self) -> [u8; 4] {
        self.to_array()
    }

    /// Deckkraft mit `factor` multiplizieren (0 = durchsichtig).
    pub fn gamma_multiply(self, factor: f32) -> Self {
        Self {
            a: (self.a as f32 * factor.clamp(0.0, 1.0)).round() as u8,
            ..self
        }
    }

    /// Dieselbe Farbe mit anderer Deckkraft (0–1).
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (alpha.clamp(0.0, 1.0) * 255.0) as u8,
            ..self
        }
    }
}

/// Lineare Mischung zweier Farben, `t` = 0 ergibt `a`.
/// Linear zwischen `a` und `b`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::from_two_pos(pos2(10.0, 20.0), pos2(0.0, 0.0));
        assert_eq!(r.min, pos2(0.0, 0.0));
        assert_eq!(r.size(), vec2(10.0, 20.0));
        assert!(r.contains(pos2(5.0, 5.0)));
        assert!(!r.shrink(6.0).is_positive());
        let all = Rect::from_points(&[pos2(1.0, 2.0), pos2(-1.0, 5.0)]);
        assert_eq!(all, Rect::from_min_max(pos2(-1.0, 2.0), pos2(1.0, 5.0)));
        assert_eq!(Rect::NOTHING.union(r), r);
    }

    #[test]
    fn color_alpha() {
        let c = Color32::from_rgb(10, 20, 30).gamma_multiply(0.5);
        assert_eq!(c.to_array(), [10, 20, 30, 128]);
    }
}
