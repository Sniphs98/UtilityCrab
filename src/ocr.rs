use std::process::Command;
use std::time::Duration;

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrResult};
use windows::Storage::Streams::DataWriter;

use crate::clipboard;

/// Öffnet das Windows-Ausschneidetool, wartet auf den Screenshot und
/// legt den darin erkannten Text in die Zwischenablage.
pub fn snip_to_clipboard() -> Result<String, String> {
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
pub fn clipboard_image_to_clipboard() -> Result<String, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let image = cb
        .get_image()
        .map_err(|_| "Kein Bild in der Zwischenablage".to_string())?;

    let text = recognize(image.width as u32, image.height as u32, &image.bytes)
        .map_err(|e| format!("Texterkennung: {e}"))?;
    if text.trim().is_empty() {
        return Err("Kein Text im Bild gefunden".into());
    }

    cb.set_text(text.clone()).map_err(|e| e.to_string())?;
    Ok(text)
}

/// Texterkennung mit der eingebauten Windows-OCR (Sprachen aus dem Benutzerprofil).
pub fn recognize(width: u32, height: u32, rgba: &[u8]) -> windows::core::Result<String> {
    let (result, _) = run_engine(width, height, rgba)?;
    let lines: Vec<String> = result
        .Lines()?
        .into_iter()
        .filter_map(|line| line.Text().ok())
        .map(|t| t.to_string())
        .collect();
    Ok(lines.join("\r\n"))
}

/// Ein erkanntes Wort mit seiner Lage im Bild (x, y, Breite, Höhe in Pixeln des Originals).
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub rect: [f32; 4],
}

/// Wie [`recognize`], aber mit Wortpositionen, zeilenweise (z. B. zum Unkenntlichmachen).
pub fn recognize_words(width: u32, height: u32, rgba: &[u8]) -> windows::core::Result<Vec<Vec<Word>>> {
    let (result, scale) = run_engine(width, height, rgba)?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut words = Vec::new();
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            words.push(Word {
                text: word.Text()?.to_string(),
                rect: [r.X / scale, r.Y / scale, r.Width / scale, r.Height / scale],
            });
        }
        lines.push(words);
    }
    Ok(lines)
}

/// Lässt die OCR laufen; zurück kommt auch der Verkleinerungsfaktor (1.0 = Originalgröße).
fn run_engine(width: u32, height: u32, rgba: &[u8]) -> windows::core::Result<(OcrResult, f32)> {
    // Zu große Bilder lehnt die OCR ab, also vorher verkleinern.
    let max = OcrEngine::MaxImageDimension()?;
    let (mut bgra, new_width, new_height) = shrink_to_fit(rgba.to_vec(), width, height, max);
    let scale = new_width as f32 / width.max(1) as f32;

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
        new_width as i32,
        new_height as i32,
    )?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    Ok((engine.RecognizeAsync(&bitmap)?.join()?, scale))
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

/// Hinweis nach einer Texterkennung; `None`, wenn der Benutzer nur abgebrochen hat.
pub fn notice(result: &Result<String, String>) -> Option<crate::popup::Notice> {
    use crate::popup::{Block, Icon, Kind, Notice};
    match result {
        Ok(text) => {
            let chars = text.chars().filter(|c| !c.is_whitespace()).count();
            let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
            let subtitle = format!(
                "{chars} {} · {lines} {}",
                "Zeichen",
                if lines == 1 { "Zeile" } else { "Zeilen" }
            );
            Some(
                Notice::new(Kind::Success, "Text kopiert")
                    .subtitle(subtitle)
                    .block(Block::Quote(text.clone()))
                    .block(Block::Chip {
                        icon: Icon::Clipboard,
                        text: "In Zwischenablage".into(),
                    }),
            )
        }
        Err(e) if e == "Kein Bildausschnitt aufgenommen" => None,
        Err(e) if e == "Kein Text im Bild gefunden" => Some(
            Notice::new(Kind::Info, "Kein Text gefunden")
                .block(Block::Text("Im Bildausschnitt wurde kein Text erkannt.".into()))
                .block(Block::Hint(
                    "Klappt am besten mit gut lesbarem, nicht zu kleinem Text. \
                     Die Sprache muss in Windows installiert sein."
                        .into(),
                )),
        ),
        Err(e) if e == "Kein Bild in der Zwischenablage" => Some(
            Notice::new(Kind::Info, "Kein Bild in der Zwischenablage")
                .block(Block::Text("Erst ein Bild oder einen Screenshot kopieren, dann noch einmal versuchen.".into())),
        ),
        Err(e) => Some(Notice::new(Kind::Error, "Texterkennung fehlgeschlagen").block(Block::Text(e.clone()))),
    }
}
