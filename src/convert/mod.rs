//! Bilder umwandeln und verkleinern.
//!
//! Gelesen wird mit dem `image`-Crate; was es nicht kennt (HEIC, AVIF, RAW …),
//! liest die Windows Imaging Component, sofern die passende Windows-Erweiterung installiert ist.
//! Geschrieben werden PNG (mit oxipng optimiert), JPG, WebP (libwebp), BMP, GIF, TIFF und ICO.

pub mod batch;
mod notice;
pub mod ui;
mod wic;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};

/// Startargument: `--konvertieren <auftrag> <dateien …>`, siehe [`batch::Job`].
pub const CLI_ARG: &str = "--konvertieren";

/// Dateiendungen, die gelesen werden können (HEIC/HEIF/AVIF über Windows).
pub const INPUT_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "tif", "tiff", "ico", "heic", "heif", "avif",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Png,
    Jpg,
    Webp,
    Bmp,
    Gif,
    Tiff,
    Ico,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Png,
        Format::Jpg,
        Format::Webp,
        Format::Bmp,
        Format::Gif,
        Format::Tiff,
        Format::Ico,
    ];

    /// Kurzname für die Kommandozeile, gleichzeitig die Dateiendung.
    pub fn id(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Bmp => "bmp",
            Format::Gif => "gif",
            Format::Tiff => "tif",
            Format::Ico => "ico",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpg => "JPG",
            Format::Webp => "WebP",
            Format::Bmp => "BMP",
            Format::Gif => "GIF",
            Format::Tiff => "TIFF",
            Format::Ico => "ICO (Symbol)",
        }
    }

    /// Kurzname für Fließtext, z. B. „In ICO umgewandelt“.
    pub fn short(self) -> &'static str {
        match self {
            Format::Ico => "ICO",
            other => other.label(),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" | "jfif" => Some(Format::Jpg),
            "webp" => Some(Format::Webp),
            "bmp" => Some(Format::Bmp),
            "gif" => Some(Format::Gif),
            "tif" | "tiff" => Some(Format::Tiff),
            "ico" => Some(Format::Ico),
            _ => None,
        }
    }

    /// Format anhand der Dateiendung; `None` bei Formaten, die nur gelesen werden (HEIC …).
    pub fn of_path(path: &Path) -> Option<Self> {
        path.extension().and_then(|e| e.to_str()).and_then(Self::from_id)
    }

    /// Verlustbehaftet, d. h. mit Qualitätsregler und Zielgröße.
    pub fn lossy(self) -> bool {
        matches!(self, Format::Jpg | Format::Webp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Resize {
    None,
    /// Längste Seite höchstens so viele Pixel.
    LongEdge(u32),
    /// Auf diesen Prozentsatz skalieren.
    Percent(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// `None` = Format des Originals behalten (HEIC/AVIF werden zu JPG).
    pub format: Option<Format>,
    /// 1–100, nur für JPG und WebP. 100 bei WebP = verlustfrei.
    pub quality: u8,
    pub resize: Resize,
    /// Höchstens so viele KB (nur JPG und WebP).
    pub max_kb: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Some(Format::Jpg),
            quality: 85,
            resize: Resize::None,
            max_kb: None,
        }
    }
}

impl Options {
    /// Nur Formatwechsel mit guter Qualität (Kontextmenü „In … umwandeln“).
    pub fn convert_to(format: Format) -> Self {
        Self {
            format: Some(format),
            quality: 90,
            resize: Resize::None,
            max_kb: None,
        }
    }

    /// Kontextmenü „Verkleinern“: gleiches Format, gleiche Auflösung, aber sparsamer gespeichert.
    /// BMP und TIFF werden zu (verlustfreiem) PNG.
    pub fn shrink() -> Self {
        Self {
            format: None,
            quality: 80,
            resize: Resize::None,
            max_kb: None,
        }
    }

    /// Das tatsächliche Zielformat für eine Eingabedatei.
    pub fn target_format(&self, input: &Path) -> Format {
        if let Some(format) = self.format {
            return format;
        }
        match Format::of_path(input) {
            // Unkomprimierte Formate lassen sich verlustfrei viel kleiner als PNG speichern.
            Some(Format::Bmp | Format::Tiff) => Format::Png,
            Some(format) => format,
            None => Format::Jpg,
        }
    }
}

/// Ergebnis für eine Datei.
#[derive(Debug, Clone)]
pub enum Outcome {
    Written { path: PathBuf, before: u64, after: u64 },
    /// Gleiches Format, aber nicht kleiner geworden – nichts geschrieben.
    AlreadySmall { before: u64 },
}

/// Meldet den Fortschritt einer Datei (0–1).
pub type OnProgress<'a> = &'a (dyn Fn(f32) + Sync);

/// Wandelt eine Datei um und legt das Ergebnis daneben ab. Das Original bleibt unverändert.
pub fn convert_file(input: &Path, options: &Options, progress: OnProgress) -> Result<Outcome, String> {
    let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let fail = |e: String| format!("{name}: {e}");

    let before = fs::metadata(input).map_err(|e| fail(e.to_string()))?.len();
    let format = options.target_format(input);
    let same_format = Format::of_path(input) == Some(format);

    progress(0.02);
    let image = load(input).map_err(fail)?;
    progress(0.3);
    let image = resize(image, options.resize);
    progress(0.4);
    let bytes = encode_with_limit(&image, format, options, progress).map_err(fail)?;
    progress(0.95);

    if same_format && options.resize == Resize::None && bytes.len() as u64 >= before {
        return Ok(Outcome::AlreadySmall { before });
    }

    let path = output_path(input, format);
    fs::write(&path, &bytes).map_err(|e| fail(format!("Speichern fehlgeschlagen: {e}")))?;
    progress(1.0);
    Ok(Outcome::Written {
        path,
        before,
        after: bytes.len() as u64,
    })
}

/// Liest ein Bild und dreht es gemäß EXIF-Ausrichtung (Handyfotos).
pub fn load(path: &Path) -> Result<DynamicImage, String> {
    let from_image_crate = || -> Result<DynamicImage, String> {
        let reader = ImageReader::open(path)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
        let orientation = decoder.orientation().ok();
        let mut image = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
        if let Some(orientation) = orientation {
            image.apply_orientation(orientation);
        }
        Ok(image)
    };
    match from_image_crate() {
        Ok(image) => Ok(image),
        Err(first) => wic::load(path).map_err(|_| {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            match ext.as_str() {
                "heic" | "heif" => "HEIC kann nicht gelesen werden. Bitte die „HEIF-Bilderweiterungen“ \
                                    aus dem Microsoft Store installieren."
                    .to_string(),
                "avif" => "AVIF kann nicht gelesen werden. Bitte die „AV1 Video Extension“ \
                           aus dem Microsoft Store installieren."
                    .to_string(),
                _ => format!("Bild kann nicht gelesen werden ({first})"),
            }
        }),
    }
}

fn resize(image: DynamicImage, resize: Resize) -> DynamicImage {
    let (w, h) = (image.width(), image.height());
    let (nw, nh) = match resize {
        Resize::None => return image,
        Resize::LongEdge(max) => {
            let max = max.max(1);
            if w.max(h) <= max {
                return image;
            }
            let scale = max as f64 / w.max(h) as f64;
            (scaled(w, scale), scaled(h, scale))
        }
        Resize::Percent(p) => {
            if p == 100 || p == 0 {
                return image;
            }
            let scale = p as f64 / 100.0;
            (scaled(w, scale), scaled(h, scale))
        }
    };
    image.resize_exact(nw, nh, FilterType::Lanczos3)
}

fn scaled(v: u32, scale: f64) -> u32 {
    ((v as f64 * scale).round() as u32).max(1)
}

/// Kodiert das Bild; bei einer Größenobergrenze wird erst die Qualität und dann die Auflösung gesenkt.
fn encode_with_limit(
    image: &DynamicImage,
    format: Format,
    options: &Options,
    progress: OnProgress,
) -> Result<Vec<u8>, String> {
    let limit = match options.max_kb {
        Some(kb) if format.lossy() => kb as usize * 1024,
        _ => return encode(image, format, options.quality),
    };
    // Wie viele Versuche nötig sind, ist vorher unbekannt: Der Balken nähert sich 95 % an.
    let attempts = std::cell::Cell::new(0);
    let encode = |image: &DynamicImage, format, quality| {
        attempts.set(attempts.get() + 1);
        progress(0.4 + 0.55 * (1.0 - 0.85f32.powi(attempts.get())));
        encode(image, format, quality)
    };

    let mut current = image.clone();
    for _ in 0..8 {
        let max_quality = options.quality.clamp(10, 99);
        let best = encode(&current, format, max_quality)?;
        if best.len() <= limit {
            return Ok(best);
        }
        // Binäre Suche nach der höchsten Qualität, die noch passt.
        let (mut lo, mut hi) = (10u8, max_quality);
        let mut fitting = None;
        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            let bytes = encode(&current, format, mid)?;
            if bytes.len() <= limit {
                fitting = Some(bytes);
                lo = mid + 1;
            } else {
                if mid == 10 {
                    break;
                }
                hi = mid - 1;
            }
        }
        // Erst ab mittlerer Qualität ist das Ergebnis brauchbar, sonst lieber verkleinern.
        if let Some(bytes) = fitting {
            if lo > 50 || current.width().max(current.height()) <= 320 {
                return Ok(bytes);
            }
        }
        let (w, h) = (current.width(), current.height());
        if w.max(h) <= 64 {
            break;
        }
        current = current.resize_exact(scaled(w, 0.8), scaled(h, 0.8), FilterType::Lanczos3);
    }
    Err(format!("Kleiner als {} KB ist nicht möglich", limit / 1024))
}

pub fn encode(image: &DynamicImage, format: Format, quality: u8) -> Result<Vec<u8>, String> {
    let quality = quality.clamp(1, 100);
    let mut out = Vec::new();
    match format {
        Format::Png => {
            PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive)
                .write_image(
                    image.to_rgba8().as_raw(),
                    image.width(),
                    image.height(),
                    ExtendedColorType::Rgba8,
                )
                .map_err(|e| e.to_string())?;
            // oxipng wählt Farbtiefe, Palette und Filter selbst und entfernt Metadaten.
            let mut opts = oxipng::Options::from_preset(2);
            opts.strip = oxipng::StripChunks::Safe;
            out = oxipng::optimize_from_memory(&out, &opts).map_err(|e| format!("PNG-Optimierung: {e}"))?;
        }
        Format::Jpg => {
            let rgb = flatten(image);
            JpegEncoder::new_with_quality(&mut out, quality)
                .encode_image(&rgb)
                .map_err(|e| e.to_string())?;
        }
        Format::Webp => {
            let (w, h) = (image.width(), image.height());
            let memory = if image.color().has_alpha() {
                let rgba = image.to_rgba8();
                encode_webp(webp::Encoder::from_rgba(rgba.as_raw(), w, h), quality)?
            } else {
                let rgb = image.to_rgb8();
                encode_webp(webp::Encoder::from_rgb(rgb.as_raw(), w, h), quality)?
            };
            out = memory;
        }
        Format::Bmp | Format::Gif | Format::Tiff => {
            let image_format = match format {
                Format::Bmp => ImageFormat::Bmp,
                Format::Gif => ImageFormat::Gif,
                _ => ImageFormat::Tiff,
            };
            let converted = if image.color().has_alpha() || format == Format::Gif {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            converted
                .write_to(&mut Cursor::new(&mut out), image_format)
                .map_err(|e| e.to_string())?;
        }
        Format::Ico => out = encode_ico(image)?,
    }
    Ok(out)
}

fn encode_webp(encoder: webp::Encoder, quality: u8) -> Result<Vec<u8>, String> {
    let lossless = quality >= 100;
    encoder
        .encode_simple(lossless, quality as f32)
        .map(|memory| memory.to_vec())
        .map_err(|e| format!("WebP: {e:?}"))
}

/// JPG kennt keine Transparenz: durchsichtige Bereiche werden weiß.
fn flatten(image: &DynamicImage) -> image::RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let a = a as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}

/// Windows-Symbol mit mehreren Größen (16–256 px), nicht-quadratische Bilder werden zentriert.
fn encode_ico(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let side = image.width().max(image.height()).min(256);
    let fitted = image.resize(side, side, FilterType::Lanczos3).to_rgba8();
    let mut square = RgbaImage::new(side, side);
    let (ox, oy) = ((side - fitted.width()) / 2, (side - fitted.height()) / 2);
    image::imageops::overlay(&mut square, &fitted, ox as i64, oy as i64);
    let square = DynamicImage::ImageRgba8(square);

    let mut frames = Vec::new();
    for size in [16, 24, 32, 48, 64, 128, 256] {
        if size > side {
            break;
        }
        let icon = if size == side {
            square.to_rgba8()
        } else {
            square.resize_exact(size, size, FilterType::Lanczos3).to_rgba8()
        };
        let frame = IcoFrame::as_png(icon.as_raw(), size, size, ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        frames.push(frame);
    }
    // Ungewöhnliche Größe (z. B. ein 100-px-Bild): zusätzlich in Originalgröße.
    if !matches!(side, 16 | 24 | 32 | 48 | 64 | 128 | 256) {
        let frame = IcoFrame::as_png(square.to_rgba8().as_raw(), side, side, ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        frames.push(frame);
    }
    let mut out = Vec::new();
    IcoEncoder::new(&mut out).encode_images(&frames).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Neuer Dateiname neben dem Original, ohne etwas zu überschreiben:
/// `foto.png` → `foto.jpg`, bei gleichem Format `foto-klein.jpg`, sonst `foto (2).jpg` usw.
fn output_path(input: &Path, format: Format) -> PathBuf {
    let dir = input.parent().unwrap_or(Path::new("."));
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = if Format::of_path(input) == Some(format) {
        format!("{stem}-klein")
    } else {
        stem
    };
    let ext = format.id();
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Legt die Dateien in die Zwischenablage, wie nach Strg+C im Explorer.
pub fn copy_to_clipboard(paths: &[PathBuf]) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set().file_list(paths))
        .map_err(|e| format!("Zwischenablage: {e}"))
}

/// Dateigröße lesbar, z. B. „2,4 MB“.
pub fn human_size(bytes: u64) -> String {
    let text = if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    };
    text.replace('.', ",")
}

/// Z. B. „von 2,4 MB auf 610 KB (−75 %)“.
pub fn size_change(before: u64, after: u64) -> String {
    let change = (after as f64 / before.max(1) as f64 - 1.0) * 100.0;
    let sign = if change < 0.0 { "−" } else { "+" };
    // Bei fast 100 % eine Nachkommastelle, sonst stünde dort „−100 %“.
    let percent = if change.abs() >= 99.0 && change.abs() < 100.0 {
        format!("{:.1}", change.abs()).replace('.', ",")
    } else {
        format!("{:.0}", change.abs())
    };
    format!("von {} auf {} ({sign}{percent} %)", human_size(before), human_size(after))
}

/// Kurzbeschreibung eines Ergebnisses, z. B. „von 2,4 MB auf 610 KB (−75 %)“.
pub fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Written { before, after, .. } => size_change(*before, *after),
        Outcome::AlreadySmall { before } => format!("{} – lässt sich nicht weiter verkleinern", human_size(*before)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Farbverlauf mit Rauschen, damit die Kompression etwas zu tun hat.
    fn sample(w: u32, h: u32) -> DynamicImage {
        let mut seed = 1u32;
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let n = (seed >> 24) as u8 / 4;
            image::Rgb([(x % 256) as u8 ^ n, (y % 256) as u8, ((x + y) % 256) as u8 ^ n])
        }))
    }

    #[test]
    fn target_size_is_respected() {
        let image = sample(1600, 1200);
        for format in [Format::Jpg, Format::Webp] {
            let options = Options {
                format: Some(format),
                quality: 90,
                resize: Resize::None,
                max_kb: Some(150),
            };
            let bytes = encode_with_limit(&image, format, &options, &|_| {}).unwrap();
            assert!(bytes.len() <= 150 * 1024, "{format:?}: {} Bytes", bytes.len());
        }
    }

    #[test]
    fn resize_keeps_aspect_ratio() {
        let image = resize(sample(4000, 3000), Resize::LongEdge(1000));
        assert_eq!((image.width(), image.height()), (1000, 750));
        let image = resize(sample(400, 300), Resize::Percent(50));
        assert_eq!((image.width(), image.height()), (200, 150));
    }

    #[test]
    fn output_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("utilitycrab-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("bild.png");
        fs::write(&input, b"x").unwrap();
        assert_eq!(output_path(&input, Format::Jpg), dir.join("bild.jpg"));
        assert_eq!(output_path(&input, Format::Png), dir.join("bild-klein.png"));
        fs::write(dir.join("bild.jpg"), b"x").unwrap();
        assert_eq!(output_path(&input, Format::Jpg), dir.join("bild (2).jpg"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn all_formats_roundtrip() {
        let image = sample(300, 200);
        for format in Format::ALL {
            let bytes = encode(&image, format, 80).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            if format != Format::Ico {
                assert_eq!((decoded.width(), decoded.height()), (300, 200), "{format:?}");
            }
        }
    }
}
