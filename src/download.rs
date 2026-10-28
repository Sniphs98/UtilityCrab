use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Eine herunterzuladende Datei.
pub struct FileSpec {
    pub url: String,
    pub target: PathBuf,
    /// Ungefähre Größe in Bytes, nur für die Fortschrittsanzeige.
    pub size: u64,
}

/// Lädt alle Dateien herunter, die noch fehlen, und meldet den Gesamtfortschritt (0..1).
/// Benutzt das in Windows eingebaute curl.exe.
pub fn download_all(files: &[FileSpec], progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let total: u64 = files.iter().map(|f| f.size.max(1)).sum();
    let mut done = 0u64;

    for file in files {
        if !file.target.exists() {
            download(file, &mut |bytes| {
                progress((done + bytes.min(file.size)) as f32 / total as f32)
            })?;
        }
        done += file.size.max(1);
        progress(done as f32 / total as f32);
    }
    Ok(())
}

fn download(file: &FileSpec, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    if let Some(parent) = file.target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Erst in eine .part-Datei laden, damit abgebrochene Downloads nicht als fertig gelten.
    let part = part_path(&file.target);
    let mut child = Command::new("curl.exe")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&part)
        .arg(&file.url)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("curl konnte nicht gestartet werden: {e}"))?;

    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        progress(fs::metadata(&part).map(|m| m.len()).unwrap_or(0));
        thread::sleep(Duration::from_millis(200));
    };
    if !status.success() {
        let _ = fs::remove_file(&part);
        return Err(format!("Download fehlgeschlagen: {} ({status})", file.url));
    }
    fs::rename(&part, &file.target).map_err(|e| e.to_string())
}

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    target.with_file_name(name)
}

/// Entpackt ein ZIP-Archiv mit dem in Windows eingebauten tar.exe.
pub fn unzip(archive: &Path, into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| e.to_string())?;
    let status = Command::new("tar.exe")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("tar konnte nicht gestartet werden: {e}"))?;
    if !status.success() {
        return Err(format!("Entpacken fehlgeschlagen ({status})"));
    }
    Ok(())
}
