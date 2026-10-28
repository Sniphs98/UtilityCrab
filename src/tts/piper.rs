//! Piper: kleine, schnelle Stimmen (ONNX). Spricht über eine Lautschrift (espeak-ng),
//! daher meist korrekte Aussprache. Läuft als eigenes Programm (piper.exe).

use std::fs;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::{pcm16_to_samples, Audio, Package, Provider, Request, VoiceInfo};
use crate::download::{self, FileSpec};

const ENGINE_URL: &str =
    "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
const ENGINE_SIZE: u64 = 22_500_000;
const VOICE_BASE_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";

/// Angebotene Stimmen: (ID, Anzeigename, Ordner im Repository, Größe in MB).
const VOICES: [(&str, &str, &str, u32); 7] = [
    ("de_DE-thorsten-high", "Thorsten – Deutsch, hohe Qualität", "de/de_DE/thorsten/high", 109),
    ("de_DE-thorsten-medium", "Thorsten – Deutsch", "de/de_DE/thorsten/medium", 60),
    ("de_DE-kerstin-low", "Kerstin – Deutsch", "de/de_DE/kerstin/low", 60),
    ("de_DE-ramona-low", "Ramona – Deutsch", "de/de_DE/ramona/low", 60),
    ("de_DE-eva_k-x_low", "Eva – Deutsch, sehr klein", "de/de_DE/eva_k/x_low", 20),
    ("en_US-lessac-medium", "Lessac – Englisch (US)", "en/en_US/lessac/medium", 60),
    ("en_GB-alba-medium", "Alba – Englisch (UK)", "en/en_GB/alba/medium", 60),
];

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn base_dir() -> PathBuf {
    crate::data_dir().join("piper")
}

fn engine_exe() -> PathBuf {
    base_dir().join("piper").join("piper.exe")
}

fn voice_model(id: &str) -> PathBuf {
    base_dir().join("voices").join(format!("{id}.onnx"))
}

fn voice_config(id: &str) -> PathBuf {
    base_dir().join("voices").join(format!("{id}.onnx.json"))
}

fn is_voice_installed(id: &str) -> bool {
    engine_exe().exists() && voice_model(id).exists() && voice_config(id).exists()
}

/// Sprache einer Stimme laut ID, z. B. "de" für "de_DE-thorsten-medium".
fn voice_language(id: &str) -> &str {
    id.split('_').next().unwrap_or(id)
}

/// Die gewählte Stimme, oder eine installierte Stimme in `lang`, falls die gewählte anders spricht.
fn matching_voice<'a>(selected: &'a str, lang: &str) -> &'a str {
    if voice_language(selected) == lang {
        return selected;
    }
    VOICES
        .iter()
        .map(|(id, ..)| *id)
        .find(|id| voice_language(id) == lang && is_voice_installed(id))
        .unwrap_or(selected)
}

pub struct Piper;

impl Provider for Piper {
    fn id(&self) -> &'static str {
        "piper"
    }

    fn name(&self) -> &'static str {
        "Piper"
    }

    fn description(&self) -> &'static str {
        "Kleine, schnelle Stimmen mit meist korrekter Aussprache, klingt etwas weniger natürlich. \
         Beim ersten Download kommt das Piper-Programm dazu (22 MB). Lizenz: MIT, Stimmen je nach Sprecher."
    }

    fn packages(&self) -> Vec<Package> {
        VOICES
            .iter()
            .map(|(id, name, _, size_mb)| Package {
                id: id.to_string(),
                name: name.to_string(),
                size_mb: *size_mb,
                installed: is_voice_installed(id),
            })
            .collect()
    }

    fn install(&self, package: &str, progress: &mut dyn FnMut(f32)) -> Result<(), String> {
        let (id, _, folder, size_mb) = VOICES
            .iter()
            .find(|(id, ..)| *id == package)
            .ok_or_else(|| format!("Unbekannte Piper-Stimme: {package}"))?;

        let zip = base_dir().join("piper_windows_amd64.zip");
        let mut files = Vec::new();
        if !engine_exe().exists() {
            files.push(FileSpec {
                url: ENGINE_URL.into(),
                target: zip.clone(),
                size: ENGINE_SIZE,
            });
        }
        files.push(FileSpec {
            url: format!("{VOICE_BASE_URL}/{folder}/{id}.onnx"),
            target: voice_model(id),
            size: *size_mb as u64 * 1_000_000,
        });
        files.push(FileSpec {
            url: format!("{VOICE_BASE_URL}/{folder}/{id}.onnx.json"),
            target: voice_config(id),
            size: 5_000,
        });
        download::download_all(&files, progress)?;

        if zip.exists() {
            download::unzip(&zip, &base_dir())?;
            let _ = fs::remove_file(&zip);
        }
        if !engine_exe().exists() {
            return Err("piper.exe fehlt nach dem Entpacken".into());
        }
        Ok(())
    }

    fn uninstall(&self, package: &str) -> Result<(), String> {
        for path in [voice_model(package), voice_config(package)] {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| format!("Piper-Stimme löschen: {e}"))?;
            }
        }
        // Ohne Stimmen wird auch das Piper-Programm nicht mehr gebraucht.
        if VOICES.iter().all(|(id, ..)| !voice_model(id).exists()) {
            fs::remove_dir_all(base_dir()).map_err(|e| format!("Piper löschen: {e}"))?;
        }
        Ok(())
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        VOICES
            .iter()
            .filter(|(id, ..)| is_voice_installed(id))
            .map(|(id, name, ..)| VoiceInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }

    fn synthesize(&self, text: &str, request: &Request) -> Result<Audio, String> {
        let voice = match request.lang {
            Some(lang) if request.auto_language => matching_voice(request.voice, lang),
            _ => request.voice,
        };
        let config = fs::read_to_string(voice_config(voice))
            .map_err(|e| format!("Piper-Stimme {voice}: {e}"))?;
        let sample_rate = serde_json::from_str::<serde_json::Value>(&config)
            .ok()
            .and_then(|c| c["audio"]["sample_rate"].as_u64())
            .unwrap_or(22_050) as u32;

        let mut child = Command::new(engine_exe())
            .arg("--model")
            .arg(voice_model(voice))
            .arg("--output_raw")
            .arg("--length_scale")
            .arg(format!("{:.3}", 1.0 / request.speed.max(0.1)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("piper.exe konnte nicht gestartet werden: {e}"))?;

        // Piper liest eine Zeile pro Äußerung.
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{line}").map_err(|e| format!("Piper: {e}"))?;
        }
        let output = child.wait_with_output().map_err(|e| format!("Piper: {e}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Piper: {}", err.lines().last().unwrap_or("unbekannter Fehler")));
        }
        Ok(Audio {
            samples: pcm16_to_samples(&output.stdout),
            sample_rate,
        })
    }
}
