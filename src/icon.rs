use tray_icon::Icon;

pub const SIZE: u32 = 32;

pub fn crab_icon() -> Icon {
    Icon::from_rgba(crab_rgba(), SIZE, SIZE).expect("Icon ungültig")
}

/// Zeichnet ein kleines orangefarbenes Krabben-Icon (RGBA, `SIZE`×`SIZE`), damit keine Bilddatei nötig ist.
pub fn crab_rgba() -> Vec<u8> {
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    let mut put = |x: i32, y: i32, color: [u8; 4]| {
        if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
            let i = ((y as u32 * SIZE + x as u32) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&color);
        }
    };

    let shell = [0xE8, 0x6A, 0x1C, 0xFF];
    let dark = [0x8A, 0x35, 0x08, 0xFF];
    let white = [0xFF, 0xFF, 0xFF, 0xFF];

    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 18.0);
            // Körper (Ellipse)
            let body = (dx / 11.0).powi(2) + (dy / 8.0).powi(2);
            // Scheren links und rechts oben
            let claw_l = ((x as f32 - 5.0) / 4.0).powi(2) + ((y as f32 - 7.0) / 4.0).powi(2);
            let claw_r = ((x as f32 - 26.0) / 4.0).powi(2) + ((y as f32 - 7.0) / 4.0).powi(2);

            if body <= 1.0 || claw_l <= 1.0 || claw_r <= 1.0 {
                let edge = body > 0.75 && claw_l > 0.5 && claw_r > 0.5;
                put(x, y, if edge { dark } else { shell });
            }
        }
    }
    // Augen
    for (ex, ey) in [(12, 15), (19, 15)] {
        for (ox, oy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            put(ex + ox, ey + oy, white);
        }
    }

    rgba
}
