use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Einstellungen, die der Dialog schreibt und das Tray-Programm liest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Provider-ID, z. B. "windows", "supertonic" oder "piper".
    pub provider: String,
    /// Stimmen-ID innerhalb des Providers (leer = erste verfügbare Stimme).
    pub voice: String,
    /// Sprechtempo, 1.0 = normal.
    pub speed: f32,
    /// Pro Satz die Sprache erkennen und, wenn nötig, eine passende installierte Stimme nehmen.
    pub auto_language: bool,
    /// Tastenkürzel im Format von `global-hotkey`, z. B. "Ctrl+Alt+S".
    pub hotkey_speak: String,
    pub hotkey_ocr: String,
    /// Aussprache-Wörterbuch für falsch gesprochene Wörter.
    pub pronunciations: Vec<Pronunciation>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pronunciation {
    pub word: String,
    pub spoken: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "windows".into(),
            voice: String::new(),
            speed: 1.0,
            auto_language: true,
            hotkey_speak: "Ctrl+Alt+S".into(),
            hotkey_ocr: "Ctrl+Alt+O".into(),
            pronunciations: Vec::new(),
        }
    }
}

pub fn path() -> PathBuf {
    crate::data_dir().join("einstellungen.json")
}

/// Zeitpunkt der letzten Änderung, um Änderungen aus dem Dialog zu bemerken.
pub fn modified() -> Option<SystemTime> {
    fs::metadata(path()).and_then(|m| m.modified()).ok()
}

impl Settings {
    pub fn load() -> Self {
        let mut settings = match fs::read_to_string(path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::migrate_old_voice(),
        };
        // Von älteren Versionen falsch aufgenommene Kürzel (z. B. "Ctrl+ControlLeft") zurücksetzen.
        let defaults = Self::default();
        for (hotkey, default) in [
            (&mut settings.hotkey_speak, defaults.hotkey_speak),
            (&mut settings.hotkey_ocr, defaults.hotkey_ocr),
        ] {
            if ["Left", "Right"].iter().any(|side| hotkey.ends_with(side)) {
                *hotkey = default;
            }
        }
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = path();
        fs::create_dir_all(crate::data_dir()).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Erst in eine Hilfsdatei schreiben, damit das Tray-Programm nie eine halbe Datei liest.
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, json).map_err(|e| format!("Einstellungen speichern: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("Einstellungen speichern: {e}"))
    }

    /// Übernimmt die Stimmenwahl aus älteren Versionen (stimme.txt).
    fn migrate_old_voice() -> Self {
        let mut settings = Self::default();
        if let Ok(old) = fs::read_to_string(crate::data_dir().join("stimme.txt")) {
            let old = old.trim();
            if old != "windows" && !old.is_empty() {
                settings.provider = "supertonic".into();
                settings.voice = old.into();
            }
        }
        settings
    }
}

/// Zeigt ein Tastenkürzel deutsch an, z. B. "Strg+Alt+S".
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Strg".to_string(),
            "shift" => "Umschalt".to_string(),
            "super" => "Win".to_string(),
            _ => part.strip_prefix("Key").unwrap_or(part).to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}
