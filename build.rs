//! Bettet das Krabben-Icon in die .exe ein (Explorer, Startmenü, Kontextmenü „Konvertieren“).

#[path = "src/crab_pixels.rs"]
mod crab_pixels;

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/crab_pixels.rs");
    // Release-Version (z. B. 0.1.0.7) vom Workflow bzw. installer/build.ps1; leer = lokaler Build.
    println!("cargo:rerun-if-env-changed=UTILITYCRAB_VERSION");
    let version = std::env::var("UTILITYCRAB_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=UTILITYCRAB_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("crab.ico");
    std::fs::write(&ico, ico_file(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();

    let mut res = winresource::WindowsResource::new();
    res.set_icon(ico.to_str().unwrap());
    res.compile().expect("Ressourcen konnten nicht kompiliert werden (rc.exe aus dem Windows SDK nötig)");
}

/// ICO-Datei mit 32-Bit-BMP-Bildern in den angegebenen Größen.
fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp_entry(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, data) in sizes.iter().zip(&images) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // Ebenen
        out.extend_from_slice(&32u16.to_le_bytes()); // Bit pro Pixel
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for data in images {
        out.extend_from_slice(&data);
    }
    out
}

/// Ein Bild im ICO: BITMAPINFOHEADER, BGRA-Zeilen von unten nach oben, dann die (leere) AND-Maske.
fn bmp_entry(size: u32) -> Vec<u8> {
    let rgba = crab_pixels::crab_rgba(size);
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, size, size * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // keine Kompression
    out.extend_from_slice(&(size * size * 4 + mask_row * size).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    out
}
