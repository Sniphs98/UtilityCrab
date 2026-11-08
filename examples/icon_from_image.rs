//! Macht aus einem Bild mit weißem Hintergrund ein Icon-PNG mit Transparenz.
//!
//! `cargo run --release --example icon_from_image -- <eingabe> <ausgabe.png> [vorschau-ordner]`
//!
//! - Weißer Hintergrund wird durchsichtig – aber nur, was vom Rand aus zusammenhängt,
//!   weiße Stellen im Motiv (z. B. Glanzpunkte in den Augen) bleiben.
//! - Halbtransparente Kanten entstehen aus den grauen Übergangspixeln zur dunklen Umrandung.
//! - Zugeschnitten auf das Motiv, quadratisch, 1024 px.
//! - Optional: Vorschau in den Icon-Größen 16–256 px.

use std::collections::VecDeque;

use image::imageops::FilterType;
use image::{GenericImageView, Rgba, RgbaImage};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(output)) = (args.first(), args.get(1)) else {
        eprintln!("Aufruf: icon_from_image <eingabe> <ausgabe.png> [vorschau-ordner]");
        std::process::exit(1);
    };
    let source = match image::open(input) {
        Ok(image) => image,
        Err(e) => {
            eprintln!("„{input}“ konnte nicht gelesen werden: {e}");
            // Hilfe: Bilder im selben Ordner auflisten.
            let dir = std::path::Path::new(input).parent().filter(|d| !d.as_os_str().is_empty());
            let dir = dir.unwrap_or(std::path::Path::new("."));
            if let Ok(entries) = std::fs::read_dir(dir) {
                let images: Vec<String> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| ["png", "jpg", "jpeg", "webp", "bmp"].contains(&e.to_ascii_lowercase().as_str()))
                    })
                    .map(|p| p.display().to_string())
                    .collect();
                if !images.is_empty() {
                    eprintln!("Bilder in {}:", dir.display());
                    for image in images {
                        eprintln!("  {image}");
                    }
                }
            }
            std::process::exit(1);
        }
    };
    if let Some(parent) = std::path::Path::new(output).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).expect("Zielordner konnte nicht angelegt werden");
    }
    let (w, h) = source.dimensions();
    let rgb = source.to_rgb8();

    let lum = |x: u32, y: u32| {
        let [r, g, b] = rgb.get_pixel(x, y).0;
        0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
    };
    let sat = |x: u32, y: u32| {
        let [r, g, b] = rgb.get_pixel(x, y).0;
        r.max(g).max(b) - r.min(g).min(b)
    };
    let is_background = |x: u32, y: u32| lum(x, y) > 225.0 && sat(x, y) < 30;

    // Hintergrund vom Rand aus füllen.
    let mut background = vec![false; (w * h) as usize];
    let mut queue = VecDeque::new();
    for x in 0..w {
        queue.push_back((x, 0));
        queue.push_back((x, h - 1));
    }
    for y in 0..h {
        queue.push_back((0, y));
        queue.push_back((w - 1, y));
    }
    while let Some((x, y)) = queue.pop_front() {
        let i = (y * w + x) as usize;
        if background[i] || !is_background(x, y) {
            continue;
        }
        background[i] = true;
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if x + 1 < w {
            queue.push_back((x + 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if y + 1 < h {
            queue.push_back((x, y + 1));
        }
    }

    // Abstand zum Hintergrund (in Pixeln, max. 3) für die weiche Kante.
    let near_background = |x: u32, y: u32| {
        let mut best = u32::MAX;
        for dy in -3i32..=3 {
            for dx in -3i32..=3 {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                if background[(ny as u32 * w + nx as u32) as usize] {
                    best = best.min(dx.unsigned_abs().max(dy.unsigned_abs()));
                }
            }
        }
        best
    };

    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = rgb.get_pixel(x, y).0;
            let pixel = if background[(y * w + x) as usize] {
                Rgba([0, 0, 0, 0])
            } else if near_background(x, y) <= 3 && lum(x, y) > 60.0 {
                // Übergang Weiß → schwarze Umrandung: als halbtransparentes Schwarz.
                let alpha = ((255.0 - lum(x, y)) / (255.0 - 40.0)).clamp(0.0, 1.0);
                Rgba([12, 12, 12, (alpha * 255.0) as u8])
            } else {
                Rgba([r, g, b, 255])
            };
            out.put_pixel(x, y, pixel);
        }
    }

    // Auf das Motiv zuschneiden, quadratisch mit etwas Rand.
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in out.enumerate_pixels() {
        if p.0[3] > 10 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let side = (x1 - x0).max(y1 - y0) + 1;
    let margin = side / 50;
    let canvas_side = side + 2 * margin;
    let mut square = RgbaImage::new(canvas_side, canvas_side);
    let ox = margin + (side - (x1 - x0 + 1)) / 2;
    let oy = margin + (side - (y1 - y0 + 1)) / 2;
    let cropped = image::imageops::crop_imm(&out, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    image::imageops::overlay(&mut square, &cropped, ox as i64, oy as i64);
    let icon = image::imageops::resize(&square, 1024, 1024, FilterType::Lanczos3);
    icon.save(output).expect("Speichern fehlgeschlagen");
    println!("{output}: Motiv {}×{} px, zugeschnitten auf {canvas_side}×{canvas_side}", x1 - x0 + 1, y1 - y0 + 1);

    if let Some(dir) = args.get(2) {
        for size in [16, 24, 32, 48, 64, 256] {
            let small = image::imageops::resize(&icon, size, size, FilterType::Lanczos3);
            small
                .save(std::path::Path::new(dir).join(format!("icon-{size}.png")))
                .expect("Vorschau speichern");
        }
    }
}
