use std::process::Command;
use std::time::Duration;

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use crate::clipboard;

/// Öffnet das Windows-Ausschneidetool, wartet auf den Screenshot und
/// legt den darin erkannten Text in die Zwischenablage.
pub fn snip_to_clipboard() -> Result<(), String> {
    let before = clipboard::sequence_number();
    Command::new("explorer")
        .arg("ms-screenclip:")
        .spawn()
        .map_err(|e| format!("Ausschneidetool konnte nicht gestartet werden: {e}"))?;

    if !clipboard::wait_for_change(before, Duration::from_secs(60)) {
        return Err("Kein Bildausschnitt aufgenommen".into());
    }
    // Das Ausschneidetool legt das Bild leicht verzögert ab.
    std::thread::sleep(Duration::from_millis(300));
    clipboard_image_to_clipboard()
}

/// Erkennt Text im Bild der Zwischenablage und ersetzt das Bild durch den Text.
pub fn clipboard_image_to_clipboard() -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let image = cb
        .get_image()
        .map_err(|_| "Kein Bild in der Zwischenablage".to_string())?;

    let text = recognize(image.width as u32, image.height as u32, &image.bytes)
        .map_err(|e| format!("Texterkennung: {e}"))?;
    if text.trim().is_empty() {
        return Err("Kein Text im Bild gefunden".into());
    }

    cb.set_text(text).map_err(|e| e.to_string())?;
    crate::beep_ok();
    Ok(())
}

/// Texterkennung mit der eingebauten Windows-OCR (Sprachen aus dem Benutzerprofil).
fn recognize(width: u32, height: u32, rgba: &[u8]) -> windows::core::Result<String> {
    // Zu große Bilder lehnt die OCR ab, also vorher verkleinern.
    let max = OcrEngine::MaxImageDimension()?;
    let (mut bgra, width, height) = shrink_to_fit(rgba.to_vec(), width, height, max);

    // Windows erwartet BGRA statt RGBA.
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
    )?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;

    let lines: Vec<String> = result
        .Lines()?
        .into_iter()
        .filter_map(|line| line.Text().ok())
        .map(|t| t.to_string())
        .collect();
    Ok(lines.join("\r\n"))
}

/// Verkleinert ein 4-Kanal-Bild (Mittelwert über Pixelblöcke), sodass keine Seite `max` überschreitet.
fn shrink_to_fit(pixels: Vec<u8>, w: u32, h: u32, max: u32) -> (Vec<u8>, u32, u32) {
    if w <= max && h <= max {
        return (pixels, w, h);
    }
    let scale = max as f64 / w.max(h) as f64;
    let nw = ((w as f64 * scale) as u32).clamp(1, max);
    let nh = ((h as f64 * scale) as u32).clamp(1, max);

    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let y0 = y * h / nh;
        let y1 = ((y + 1) * h / nh).max(y0 + 1);
        for x in 0..nw {
            let x0 = x * w / nw;
            let x1 = ((x + 1) * w / nw).max(x0 + 1);

            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += pixels[i + c] as u32;
                    }
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    (out, nw, nh)
}
