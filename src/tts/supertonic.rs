//! Neuronale Sprachausgabe mit Supertonic 3 (ONNX-Modell von Supertone, läuft lokal auf der CPU).
//! Aufbau nach dem offiziellen Rust-Beispiel: github.com/supertone-inc/supertonic/tree/main/rust

use std::f64::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use super::{Audio, Package, Provider, Request, VoiceInfo};
use crate::download::{self, FileSpec};

const PACKAGE: &str = "supertonic-3";
const BASE_URL: &str = "https://huggingface.co/Supertone/supertonic-3/resolve/main";
/// Modelldateien mit ungefährer Größe in Bytes.
const MODEL_FILES: [(&str, u64); 6] = [
    ("onnx/duration_predictor.onnx", 3_700_000),
    ("onnx/text_encoder.onnx", 36_400_000),
    ("onnx/vector_estimator.onnx", 256_500_000),
    ("onnx/vocoder.onnx", 101_400_000),
    ("onnx/tts.json", 8_000),
    ("onnx/unicode_indexer.json", 280_000),
];

/// Sprachen, die das Modell kennt.
const LANGUAGES: [&str; 31] = [
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi",
];

/// Mitgelieferte Stimmen: (Dateiname, Anzeigename).
const VOICES: [(&str, &str); 10] = [
    ("F1", "Frau 1"),
    ("F2", "Frau 2"),
    ("F3", "Frau 3"),
    ("F4", "Frau 4"),
    ("F5", "Frau 5"),
    ("M1", "Mann 1"),
    ("M2", "Mann 2"),
    ("M3", "Mann 3"),
    ("M4", "Mann 4"),
    ("M5", "Mann 5"),
];

// Standardwerte aus dem offiziellen Beispiel.
const TOTAL_STEPS: usize = 8;
const BASE_SPEED: f32 = 1.05;

/// Sprache, wenn die automatische Erkennung unsicher ist (z. B. bei sehr kurzen Texten).
const FALLBACK_LANG: &str = "de";

fn model_dir() -> PathBuf {
    crate::data_dir().join("supertonic-3")
}

fn all_files() -> Vec<FileSpec> {
    let dir = model_dir();
    let voice_files = VOICES.iter().map(|(id, _)| (format!("voice_styles/{id}.json"), 292_000));
    MODEL_FILES
        .iter()
        .map(|(f, size)| (f.to_string(), *size))
        .chain(voice_files)
        .map(|(file, size)| FileSpec {
            url: format!("{BASE_URL}/{file}"),
            target: dir.join(&file),
            size,
        })
        .collect()
}

fn is_installed() -> bool {
    all_files().iter().all(|f| f.target.exists())
}

/// Geladenes Modell und zuletzt benutzte Stimme.
#[derive(Default)]
struct State {
    engine: Option<Supertonic>,
    style: Option<(String, Style)>,
}

impl State {
    /// Lädt Modell und Stimme, falls noch nicht geschehen.
    fn prepare(&mut self, id: &str) -> Result<(&mut Supertonic, &Style), String> {
        if self.engine.is_none() {
            self.engine = Some(Supertonic::load()?);
        }
        if self.style.as_ref().map(|(s, _)| s.as_str()) != Some(id) {
            self.style = Some((id.to_string(), load_style(id)?));
        }
        match (self.engine.as_mut(), self.style.as_ref()) {
            (Some(engine), Some((_, style))) => Ok((engine, style)),
            _ => unreachable!(),
        }
    }
}

#[derive(Default)]
pub struct SupertonicProvider {
    state: Mutex<State>,
}

impl SupertonicProvider {
    fn state(&self) -> Result<std::sync::MutexGuard<'_, State>, String> {
        self.state.lock().map_err(|_| "Supertonic ist abgestürzt".to_string())
    }
}

impl Provider for SupertonicProvider {
    fn id(&self) -> &'static str {
        "supertonic"
    }

    fn name(&self) -> &'static str {
        "Supertonic 3"
    }

    fn description(&self) -> &'static str {
        "Sehr natürlicher Klang, 31 Sprachen, 10 Stimmen. Läuft lokal auf der CPU. \
         Lizenz: OpenRAIL-M. Wird nicht mehr weiterentwickelt."
    }

    fn packages(&self) -> Vec<Package> {
        vec![Package {
            id: PACKAGE.into(),
            name: "Modell mit allen 10 Stimmen".into(),
            size_mb: 400,
            installed: is_installed(),
        }]
    }

    fn install(&self, _package: &str, progress: &mut dyn FnMut(f32)) -> Result<(), String> {
        download::download_all(&all_files(), progress)
    }

    fn uninstall(&self, _package: &str) -> Result<(), String> {
        // Geladenes Modell freigeben, damit die Dateien gelöscht werden können.
        *self.state()? = State::default();
        fs::remove_dir_all(model_dir()).map_err(|e| format!("Supertonic löschen: {e}"))
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        if !is_installed() {
            return Vec::new();
        }
        VOICES
            .iter()
            .map(|(id, name)| VoiceInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }

    fn preload(&self, voice: &str) -> Result<(), String> {
        self.state()?.prepare(voice).map(|_| ())
    }

    fn synthesize(&self, text: &str, request: &Request) -> Result<Audio, String> {
        let lang = request
            .lang
            .filter(|l| LANGUAGES.contains(l))
            .unwrap_or(FALLBACK_LANG);
        let mut state = self.state()?;
        let (engine, style) = state.prepare(request.voice)?;
        let samples = engine
            .infer(text, lang, style, request.speed)
            .map_err(|e| format!("Supertonic: {e}"))?;
        Ok(Audio {
            samples,
            sample_rate: engine.sample_rate,
        })
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Klangfarbe einer Stimme: zwei Tensoren als (Form, Werte).
struct Style {
    ttl: (Vec<usize>, Vec<f32>),
    dp: (Vec<usize>, Vec<f32>),
}

fn load_style(id: &str) -> Result<Style, String> {
    let path = model_dir().join(format!("voice_styles/{id}.json"));
    let json = read_json(&path)?;
    let part = |key: &str| -> Result<(Vec<usize>, Vec<f32>), String> {
        let dims: Vec<usize> = json[key]["dims"]
            .as_array()
            .map(|d| d.iter().filter_map(|n| n.as_u64()).map(|n| n as usize).collect())
            .unwrap_or_default();
        let mut data = Vec::new();
        flatten(&json[key]["data"], &mut data);
        if dims.is_empty() || dims.iter().product::<usize>() != data.len() {
            return Err(format!("Stimme {id}: ungültige Daten in {key}"));
        }
        Ok((dims, data))
    };
    Ok(Style {
        ttl: part("style_ttl")?,
        dp: part("style_dp")?,
    })
}

fn flatten(value: &Value, out: &mut Vec<f32>) {
    match value {
        Value::Array(items) => items.iter().for_each(|v| flatten(v, out)),
        Value::Number(n) => out.push(n.as_f64().unwrap_or(0.0) as f32),
        _ => {}
    }
}

struct Supertonic {
    sample_rate: u32,
    chunk_size: usize,
    latent_dim: usize,
    indexer: Vec<i64>,
    duration_predictor: Session,
    text_encoder: Session,
    vector_estimator: Session,
    vocoder: Session,
    _env: Environment,
}

impl Supertonic {
    fn load() -> Result<Self, String> {
        let dir = model_dir().join("onnx");
        let cfg = read_json(&dir.join("tts.json"))?;
        let num = |v: &Value| {
            v.as_u64()
                .map(|n| n as usize)
                .ok_or_else(|| "tts.json ist unvollständig".to_string())
        };
        let chunk_compress = num(&cfg["ttl"]["chunk_compress_factor"])?;

        let indexer: Vec<i64> = serde_json::from_value(read_json(&dir.join("unicode_indexer.json"))?)
            .map_err(|e| format!("unicode_indexer.json: {e}"))?;

        let env = ort::init()
            .with_name("utility_crab")
            .build()
            .map_err(|e| format!("ONNX Runtime: {e}"))?;
        let session = |name: &str| -> Result<Session, String> {
            Session::builder(&env)
                .and_then(|mut b| b.commit_from_file(dir.join(name)))
                .map_err(|e| format!("{name}: {e}"))
        };

        Ok(Self {
            sample_rate: num(&cfg["ae"]["sample_rate"])? as u32,
            chunk_size: num(&cfg["ae"]["base_chunk_size"])? * chunk_compress,
            latent_dim: num(&cfg["ttl"]["latent_dim"])? * chunk_compress,
            indexer,
            duration_predictor: session("duration_predictor.onnx")?,
            text_encoder: session("text_encoder.onnx")?,
            vector_estimator: session("vector_estimator.onnx")?,
            vocoder: session("vocoder.onnx")?,
            _env: env,
        })
    }

    /// Erzeugt Audio-Samples (mono, -1..1) für einen kurzen Abschnitt.
    fn infer(&mut self, text: &str, lang: &str, style: &Style, speed: f32) -> ort::Result<Vec<f32>> {
        let ids: Vec<i64> = preprocess(text, lang)
            .chars()
            .map(|c| self.indexer.get(c as usize).copied().unwrap_or(-1))
            .collect();
        let len = ids.len();
        let text_ids = Tensor::from_array(([1, len], ids))?;
        let text_mask = Tensor::from_array(([1, 1, len], vec![1.0f32; len]))?;
        let style_ttl = Tensor::from_array(style.ttl.clone())?;
        let style_dp = Tensor::from_array(style.dp.clone())?;

        // Sprechdauer vorhersagen
        let duration = {
            let out = self.duration_predictor.run(ort::inputs! {
                "text_ids" => &text_ids,
                "style_dp" => &style_dp,
                "text_mask" => &text_mask,
            })?;
            out["duration"].try_extract_tensor::<f32>()?.1[0] / (BASE_SPEED * speed.max(0.1))
        };

        // Text kodieren
        let text_emb = {
            let out = self.text_encoder.run(ort::inputs! {
                "text_ids" => &text_ids,
                "style_ttl" => &style_ttl,
                "text_mask" => &text_mask,
            })?;
            let (shape, data) = out["text_emb"].try_extract_tensor::<f32>()?;
            Tensor::from_array((shape.to_vec(), data.to_vec()))?
        };

        // Aus Rauschen schrittweise das Audio-Latent berechnen
        let wav_len = (duration * self.sample_rate as f32) as usize;
        let latent_len = wav_len.div_ceil(self.chunk_size).max(1);
        let latent_shape = [1, self.latent_dim, latent_len];
        let latent_mask = Tensor::from_array(([1, 1, latent_len], vec![1.0f32; latent_len]))?;
        let total_step = Tensor::from_array(([1], vec![TOTAL_STEPS as f32]))?;

        let mut latent = gaussian_noise(self.latent_dim * latent_len);
        for step in 0..TOTAL_STEPS {
            let out = self.vector_estimator.run(ort::inputs! {
                "noisy_latent" => Tensor::from_array((latent_shape, latent))?,
                "text_emb" => &text_emb,
                "style_ttl" => &style_ttl,
                "latent_mask" => &latent_mask,
                "text_mask" => &text_mask,
                "current_step" => Tensor::from_array(([1], vec![step as f32]))?,
                "total_step" => &total_step,
            })?;
            latent = out["denoised_latent"].try_extract_tensor::<f32>()?.1.to_vec();
        }

        // Latent in Audio umwandeln
        let out = self.vocoder.run(ort::inputs! {
            "latent" => Tensor::from_array((latent_shape, latent))?,
        })?;
        let wav = out["wav_tts"].try_extract_tensor::<f32>()?.1;
        Ok(wav[..wav_len.min(wav.len())].to_vec())
    }
}

/// Bereitet den Text so auf, wie das Modell ihn erwartet (vereinfachte Fassung des Originals).
fn preprocess(text: &str, lang: &str) -> String {
    let mut text: String = text
        .nfkd()
        .filter(|c| !is_emoji(*c))
        .map(|c| match c {
            '–' | '‑' | '—' => '-',
            '“' | '”' => '"',
            '‘' | '’' | '´' | '`' => '\'',
            '_' | '[' | ']' | '|' | '/' | '#' | '→' | '←' => ' ',
            c => c,
        })
        .filter(|c| !matches!(c, '♥' | '☆' | '♡' | '©' | '\\'))
        .collect();

    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    for p in [',', '.', '!', '?', ';', ':', '\''] {
        text = text.replace(&format!(" {p}"), &p.to_string());
    }
    for q in ["\"", "'"] {
        while text.contains(&q.repeat(2)) {
            text = text.replace(&q.repeat(2), q);
        }
    }
    if !text.ends_with(['.', '!', '?', ';', ':', ',', '\'', '"', ')', ']', '}', '…', '。', '」', '』', '】', '〉', '》', '›', '»']) {
        text.push('.');
    }
    format!("<{lang}>{text}</{lang}>")
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F1E6..=0x1F1FF | 0x1F300..=0x1F64F | 0x1F680..=0x1F6FF | 0x1F700..=0x1FAFF
        | 0x2600..=0x26FF | 0x2700..=0x27BF)
}

/// Normalverteiltes Rauschen (Box-Muller mit einfachem Xorshift-Zufallsgenerator).
fn gaussian_noise(n: usize) -> Vec<f32> {
    let mut state = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        | 1;
    let mut uniform = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| {
            let (u1, u2) = (uniform(), uniform());
            ((-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos()) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Braucht das heruntergeladene Modell: `cargo test --release -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn synthesizes_german_text() {
        let provider = SupertonicProvider::default();
        let start = std::time::Instant::now();
        provider.preload("F1").unwrap();
        println!("Laden: {:.2?}", start.elapsed());

        let start = std::time::Instant::now();
        let request = Request { voice: "F1", speed: 1.0, lang: Some("de"), auto_language: true };
        let audio = provider.synthesize("Hallo, das ist ein Test der neuen Stimme.", &request).unwrap();
        let secs = audio.samples.len() as f32 / audio.sample_rate as f32;
        let rms = (audio.samples.iter().map(|s| s * s).sum::<f32>() / audio.samples.len() as f32).sqrt();
        println!("{secs:.1} s Audio in {:.2?}, RMS {rms:.3}", start.elapsed());

        assert!((1.0..10.0).contains(&secs));
        assert!(rms > 0.01);
    }
}
