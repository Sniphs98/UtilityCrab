//! Öffnen, Speichern und Kopieren von Bildern.

use std::path::{Path, PathBuf};

use image::{DynamicImage, RgbaImage};
use windows::core::PCWSTR;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use crate::convert::{self, Format};
use crate::settings::Settings;

/// Ordner für automatisch gespeicherte Screenshots (Standard: Bilder\Screenshots).
pub fn screenshot_folder(settings: &Settings) -> PathBuf {
    if !settings.snip_folder.trim().is_empty() {
        return PathBuf::from(settings.snip_folder.trim());
    }
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Pictures")
        .join("Screenshots")
}

/// „Screenshot 2026-11-10 143522.png“
pub fn default_name() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "Screenshot {:04}-{:02}-{:02} {:02}{:02}{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// Schreibt das Bild; das Format ergibt sich aus der Dateiendung (Standard PNG).
pub fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let format = Format::of_path(path).unwrap_or(Format::Png);
    let dynamic = DynamicImage::ImageRgba8(image.clone());
    // JPG kennt keine Transparenz: auf Weiß legen statt Schwarz.
    let dynamic = if format == Format::Jpg {
        let mut flat = RgbaImage::from_pixel(image.width(), image.height(), image::Rgba([255, 255, 255, 255]));
        image::imageops::overlay(&mut flat, image, 0, 0);
        DynamicImage::ImageRgba8(flat)
    } else {
        dynamic
    };
    let bytes = convert::encode(&dynamic, format, 92)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, bytes).map_err(|e| format!("Speichern: {e}"))
}

/// Speichert in den Screenshot-Ordner unter einem freien Namen.
pub fn autosave(image: &RgbaImage, settings: &Settings) -> Result<PathBuf, String> {
    let dir = screenshot_folder(settings);
    let name = default_name();
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(name.replace(".png", &format!(" ({n}).png")));
        n += 1;
    }
    save(image, &path)?;
    Ok(path)
}

pub fn copy(image: &RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Zwischenablage: {e}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.as_raw()),
        })
        .map_err(|e| format!("Zwischenablage: {e}"))
}

pub fn clipboard_image() -> Result<RgbaImage, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Zwischenablage: {e}"))?;
    let data = clipboard.get_image().map_err(|_| "Kein Bild in der Zwischenablage".to_string())?;
    RgbaImage::from_raw(data.width as u32, data.height as u32, data.bytes.into_owned())
        .ok_or_else(|| "Bild in der Zwischenablage ist ungültig".into())
}

/// Lädt ein Bild von der Platte (alle Formate des Konverters, auch HEIC/AVIF).
pub fn load(path: &Path) -> Result<RgbaImage, String> {
    convert::load(path).map(|image| image.to_rgba8())
}

// ---------------------------------------------------------------------------------------------
// Windows-Dateidialoge

/// Moderner „Ordner auswählen“-Dialog; `None`, wenn abgebrochen.
pub fn pick_folder(initial: &Path) -> Option<PathBuf> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM,
        FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    unsafe {
        // Meist schon durch das Fenster initialisiert; dann ist das hier ein harmloser Zweitaufruf.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let _ = dialog.SetTitle(&HSTRING::from("Ordner für Screenshots wählen"));
        if initial.is_dir() {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(initial.as_os_str()), None) {
                let _ = dialog.SetFolder(&item);
            }
        }
        dialog.Show(None).ok()?;
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(PathBuf::from)
    }
}

const OPEN_FILTER: &str = "Bilder\0*.png;*.jpg;*.jpeg;*.jfif;*.webp;*.bmp;*.gif;*.tif;*.tiff;*.ico;*.heic;*.heif;*.avif\0Alle Dateien\0*.*\0\0";
const SAVE_FILTER: &str = "PNG-Bild\0*.png\0JPG-Bild\0*.jpg\0WebP-Bild\0*.webp\0BMP-Bild\0*.bmp\0\0";
const SAVE_EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "bmp"];

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_buffer(buffer: &[u16]) -> PathBuf {
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    PathBuf::from(String::from_utf16_lossy(&buffer[..len]))
}

/// „Öffnen“-Dialog; `None`, wenn abgebrochen.
pub fn pick_open() -> Option<PathBuf> {
    let filter = wide(OPEN_FILTER);
    let title = wide("Bild zum Bearbeiten öffnen");
    let mut buffer = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    unsafe { GetOpenFileNameW(&mut ofn).as_bool() }.then(|| from_buffer(&buffer))
}

/// „Speichern unter“-Dialog mit Vorschlag; hängt die Endung des gewählten Typs an.
pub fn pick_save(suggestion: &Path) -> Option<PathBuf> {
    let filter = wide(SAVE_FILTER);
    let title = wide("Bild speichern");
    let mut buffer = vec![0u16; 4096];
    let name = suggestion.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for (i, c) in name.encode_utf16().take(buffer.len() - 1).enumerate() {
        buffer[i] = c;
    }
    let dir = suggestion.parent().map(|d| wide(&d.to_string_lossy()));
    let ext = suggestion.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let index = SAVE_EXTENSIONS.iter().position(|e| *e == ext || (ext == "jpeg" && *e == "jpg")).unwrap_or(0);
    let default_ext = wide("png");
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: index as u32 + 1,
        lpstrFile: windows::core::PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrInitialDir: dir.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null()),
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(default_ext.as_ptr()),
        Flags: OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut ofn).as_bool() } {
        return None;
    }
    let mut path = from_buffer(&buffer);
    // Typ gewechselt, aber Endung nicht angepasst: passende Endung setzen.
    let chosen = SAVE_EXTENSIONS.get(ofn.nFilterIndex.saturating_sub(1) as usize).copied().unwrap_or("png");
    if Format::of_path(&path).is_none() {
        path.set_extension(chosen);
    }
    Some(path)
}
