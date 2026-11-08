//! Zeichnet das Krabben-Icon in beliebiger Größe. Ohne Abhängigkeiten, weil auch
//! `build.rs` es benutzt, um das Icon in die .exe einzubetten.

/// RGBA-Pixel (`size`×`size`). Die Form ist auf einem 32er-Raster definiert und wird
/// mit 4×4-Unterabtastung gezeichnet, damit die Kanten auch groß glatt sind.
pub fn crab_rgba(size: u32) -> Vec<u8> {
    const SUB: u32 = 4;
    let scale = 32.0 / size as f32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            // Vormultiplizierte Summe, damit halbtransparente Kanten keine dunklen Säume bekommen.
            let mut sum = [0f32; 4];
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let u = (x as f32 + (sx as f32 + 0.5) / SUB as f32) * scale - 0.5;
                    let v = (y as f32 + (sy as f32 + 0.5) / SUB as f32) * scale - 0.5;
                    if let Some([r, g, b]) = color_at(u, v) {
                        sum[0] += r as f32;
                        sum[1] += g as f32;
                        sum[2] += b as f32;
                        sum[3] += 1.0;
                    }
                }
            }
            if sum[3] > 0.0 {
                let i = ((y * size + x) * 4) as usize;
                for c in 0..3 {
                    rgba[i + c] = (sum[c] / sum[3]).round() as u8;
                }
                rgba[i + 3] = (sum[3] / (SUB * SUB) as f32 * 255.0).round() as u8;
            }
        }
    }
    rgba
}

/// Farbe an einer Stelle des 32er-Rasters, `None` = durchsichtig.
fn color_at(u: f32, v: f32) -> Option<[u8; 3]> {
    const SHELL: [u8; 3] = [0xE8, 0x6A, 0x1C];
    const DARK: [u8; 3] = [0x8A, 0x35, 0x08];
    const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

    // Augen
    let in_eye = |ex: f32| (ex - 0.5..ex + 1.5).contains(&u) && (14.5..16.5).contains(&v);
    if in_eye(12.0) || in_eye(19.0) {
        return Some(WHITE);
    }
    // Körper (Ellipse) und Scheren links und rechts oben
    let body = ((u - 15.5) / 11.0).powi(2) + ((v - 18.0) / 8.0).powi(2);
    let claw_l = ((u - 5.0) / 4.0).powi(2) + ((v - 7.0) / 4.0).powi(2);
    let claw_r = ((u - 26.0) / 4.0).powi(2) + ((v - 7.0) / 4.0).powi(2);
    if body <= 1.0 || claw_l <= 1.0 || claw_r <= 1.0 {
        let edge = body > 0.75 && claw_l > 0.5 && claw_r > 0.5;
        return Some(if edge { DARK } else { SHELL });
    }
    None
}
