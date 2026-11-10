//! Screenshots aufnehmen und Bilder bearbeiten – Ersatz für das Snipping Tool.
//!
//! Ablauf: Die Tray-App startet `--screenshot` als eigenen Prozess. Der nimmt den Bildschirm
//! auf, zeigt darüber die Auswahl ([`overlay`]: Rechteck, Fenster, Bildschirm, Freihand),
//! kopiert das Ergebnis und öffnet den Editor ([`editor`], `--bild-bearbeiten`).
//! Der Editor öffnet auch vorhandene Bilder (Öffnen-Dialog, Drag & Drop, Explorer-Kontextmenü).
//!
//! Aufbau zum Erweitern:
//! - `annotations`: was auf dem Bild landet (Pfeil, Text, Verpixeln …), je ein Typ.
//! - `tools`: Werkzeuge, die Anmerkungen erzeugen. Registry: `tools::all()`.
//! - `editor::ACTIONS`: Knöpfe/Kürzel für Aktionen (Kopieren, Text erkennen …).
//! - `redact::PATTERNS`: Muster für „Sensible Daten unkenntlich machen“.
//! - `overlay::CaptureMode`: Aufnahmearten.

mod annotations;
mod canvas;
mod capture;
mod document;
pub mod editor;
mod export;
mod icons;
pub mod overlay;
mod pin;
mod redact;
mod tools;
mod view;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use annotations::Style;
use overlay::CaptureMode;

/// `--screenshot [rechteck|fenster|vollbild|freihand] [sekunden]`
pub const CAPTURE_ARG: &str = "--screenshot";
/// `--bild-bearbeiten [datei | --zwischenablage]`
pub const EDIT_ARG: &str = "--bild-bearbeiten";
pub const CLIPBOARD_ARG: &str = "--zwischenablage";
/// `--anheften <datei>`: Bild als schwebendes Fenster immer im Vordergrund.
pub const PIN_ARG: &str = "--anheften";

/// Gemerkte Einstellungen von Aufnahme und Editor (getrennt von den App-Einstellungen,
/// damit der Tray nicht bei jedem Werkzeugwechsel neu lädt).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: CaptureMode,
    /// Verzögerung für „Neu“ im Editor, Sekunden.
    pub delay: u32,
    pub tool: String,
    pub styles: HashMap<String, Style>,
    pub redact_patterns: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Rect,
            delay: 0,
            tool: "pfeil".into(),
            styles: HashMap::new(),
            redact_patterns: redact::default_selection(),
        }
    }
}

impl Prefs {
    fn path() -> PathBuf {
        crate::data_dir().join("screenshot.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(crate::data_dir());
            let _ = std::fs::write(Self::path(), json);
        }
    }
}

/// Einstieg für `--screenshot`.
pub fn run_capture(args: &[String]) {
    let mode = args.first().and_then(|a| CaptureMode::from_id(a));
    let delay = args.get(1).and_then(|a| a.parse::<u64>().ok()).unwrap_or(0);
    overlay::run(mode, Duration::from_secs(delay.min(60)));
}

/// Einstieg für `--bild-bearbeiten`.
pub fn run_editor(args: &[String]) {
    capture::make_dpi_aware();
    let source = match args.first().map(String::as_str) {
        Some(CLIPBOARD_ARG) => editor::Source::Clipboard,
        Some(path) => editor::Source::File(PathBuf::from(path)),
        None => editor::Source::Empty,
    };
    editor::run(source);
}

pub fn run_pin(args: &[String]) {
    capture::make_dpi_aware();
    match args.first() {
        Some(path) => pin::run(PathBuf::from(path)),
        None => crate::report(Err("Anheften: keine Datei angegeben".into())),
    }
}

/// Ordnerauswahl von Windows (für die Einstellungen).
pub fn pick_folder(initial: &std::path::Path) -> Option<PathBuf> {
    export::pick_folder(initial)
}

/// Standardordner für automatisch gespeicherte Screenshots (für die Einstellungen).
pub fn default_folder() -> PathBuf {
    export::screenshot_folder(&crate::settings::Settings {
        snip_folder: String::new(),
        ..crate::settings::Settings::default()
    })
}

/// Startet dieses Programm mit anderen Argumenten (jedes Fenster ist ein eigener Prozess).
pub fn spawn(args: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Fenster konnte nicht geöffnet werden: {e}"))
}

/// Zwischenablage für Bilder, die zwischen Prozessen weitergereicht werden.
fn temp_dir() -> PathBuf {
    crate::data_dir().join("screenshots-temp")
}

/// Schreibt ein Bild in den Zwischenordner (für Editor und Anheften); räumt alte auf.
fn write_temp(image: &image::RgbaImage) -> Result<PathBuf, String> {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Zwischenordner: {e}"))?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let old = SystemTime::now() - Duration::from_secs(24 * 3600);
        for entry in entries.flatten() {
            if entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < old) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path = dir.join(format!("bild-{nanos}.png"));
    image
        .save_with_format(&path, image::ImageFormat::Png)
        .map_err(|e| format!("Zwischenspeichern: {e}"))?;
    Ok(path)
}

/// Liegt die Datei im Zwischenordner (also noch nirgends richtig gespeichert)?
fn is_temp(path: &std::path::Path) -> bool {
    path.starts_with(temp_dir())
}
