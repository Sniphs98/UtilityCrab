//! Sprachausgabe mit austauschbaren Providern (Windows, Supertonic, Piper).

mod piper;
mod supertonic;
mod system;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use whatlang::Lang;
use windows::core::{Interface, HSTRING};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackItem, MediaPlaybackList, MediaPlaybackState, MediaPlayer};
use windows::Storage::Streams::{DataWriter, IRandomAccessStream, InMemoryRandomAccessStream};

use crate::settings::{Pronunciation, Settings};

/// Ein herunterladbares Paket eines Providers (z. B. ein Modell oder eine Stimme).
pub struct Package {
    pub id: String,
    pub name: String,
    pub size_mb: u32,
    pub installed: bool,
}

/// Eine sofort benutzbare Stimme.
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
}

/// Erzeugtes Audio (mono, -1..1).
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub struct Request<'a> {
    pub voice: &'a str,
    /// Sprechtempo, 1.0 = normal.
    pub speed: f32,
    /// Erkannte Sprache des Abschnitts (ISO 639-1), falls sicher.
    pub lang: Option<&'static str>,
    /// Bei anderer Sprache auf eine passende installierte Stimme wechseln.
    pub auto_language: bool,
}

/// Eine Sprachausgabe-Engine.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    /// Herunterladbare Pakete; leer, wenn nichts installiert werden muss.
    fn packages(&self) -> Vec<Package> {
        Vec::new()
    }
    fn install(&self, _package: &str, _progress: &mut dyn FnMut(f32)) -> Result<(), String> {
        Ok(())
    }
    fn uninstall(&self, _package: &str) -> Result<(), String> {
        Ok(())
    }

    /// Installierte Stimmen.
    fn voices(&self) -> Vec<VoiceInfo>;

    /// Lädt das Modell vorab, damit das erste Vorlesen nicht warten muss.
    fn preload(&self, _voice: &str) -> Result<(), String> {
        Ok(())
    }

    /// Spricht einen kurzen Abschnitt (etwa einen Satz).
    fn synthesize(&self, text: &str, request: &Request) -> Result<Audio, String>;
}

pub fn providers() -> &'static [Box<dyn Provider>] {
    static PROVIDERS: OnceLock<Vec<Box<dyn Provider>>> = OnceLock::new();
    PROVIDERS.get_or_init(|| {
        vec![
            Box::new(system::System),
            Box::new(supertonic::SupertonicProvider::default()),
            Box::new(piper::Piper),
        ]
    })
}

pub fn provider(id: &str) -> Option<&'static dyn Provider> {
    providers().iter().find(|p| p.id() == id).map(|p| p.as_ref())
}

/// Provider und Stimme laut Einstellungen; fällt auf die Windows-Stimme zurück,
/// wenn die gewählte Stimme nicht installiert ist.
fn resolve(settings: &Settings) -> Result<(&'static dyn Provider, String), String> {
    if let Some(provider) = provider(&settings.provider) {
        let voices = provider.voices();
        if let Some(voice) = voices
            .iter()
            .find(|v| v.id == settings.voice)
            .or_else(|| voices.first().filter(|_| settings.voice.is_empty()))
        {
            return Ok((provider, voice.id.clone()));
        }
        crate::log_error(&format!(
            "Stimme „{}“ von {} ist nicht installiert – Windows-Stimme wird benutzt",
            settings.voice,
            provider.name()
        ));
    }
    let fallback = provider("windows").expect("Windows-Provider fehlt");
    let voice = fallback.voices().into_iter().next().ok_or("Keine Windows-Stimme installiert")?;
    Ok((fallback, voice.id))
}

/// Lädt die eingestellte Stimme im Hintergrund vor.
pub fn preload(settings: &Settings) {
    if let Ok((provider, voice)) = resolve(settings) {
        thread::spawn(move || crate::report(provider.preload(&voice)));
    }
}

/// Eine laufende Synthese.
#[derive(Default)]
struct Job {
    cancel: AtomicBool,
    done: AtomicBool,
}

/// Liest Text vor: Abschnitte werden im Hintergrund berechnet und abgespielt, sobald sie fertig sind.
pub struct Speaker {
    player: Option<MediaPlayer>,
    job: Option<Arc<Job>>,
}

impl Speaker {
    pub fn new() -> Self {
        Self { player: None, job: None }
    }

    pub fn speak(&mut self, text: &str, settings: &Settings) -> Result<(), String> {
        self.stop();
        let (provider, voice) = resolve(settings)?;
        let text = apply_pronunciations(text, &settings.pronunciations);
        let (speed, auto_language) = (settings.speed, settings.auto_language);

        let list = MediaPlaybackList::new().map_err(|e| format!("Wiedergabe: {e}"))?;
        let player = MediaPlayer::new().map_err(|e| format!("Wiedergabe: {e}"))?;
        player.SetSource(&list).map_err(|e| format!("Wiedergabe: {e}"))?;

        let job = Arc::new(Job::default());
        let (thread_job, thread_player) = (job.clone(), player.clone());
        thread::spawn(move || {
            let result = stream(
                provider,
                &text,
                &voice,
                speed,
                auto_language,
                &list,
                &thread_player,
                &thread_job,
            );
            thread_job.done.store(true, Ordering::SeqCst);
            crate::report(result);
        });

        self.player = Some(player);
        self.job = Some(job);
        Ok(())
    }

    pub fn is_speaking(&self) -> bool {
        let synthesizing = self.job.as_ref().is_some_and(|job| !job.done.load(Ordering::SeqCst));
        synthesizing
            || self
                .player
                .as_ref()
                .and_then(|p| p.PlaybackSession().ok())
                .and_then(|s| s.PlaybackState().ok())
                .is_some_and(|state| {
                    state == MediaPlaybackState::Playing
                        || state == MediaPlaybackState::Opening
                        || state == MediaPlaybackState::Buffering
                })
    }

    pub fn stop(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::SeqCst);
        }
        if let Some(player) = self.player.take() {
            let _ = player.Pause();
            let _ = player.Close();
        }
    }
}

const PAUSE_SECS: f32 = 0.3;
const MAX_CHUNK_LEN: usize = 300;

fn stream(
    provider: &dyn Provider,
    text: &str,
    voice: &str,
    speed: f32,
    auto_language: bool,
    list: &MediaPlaybackList,
    player: &MediaPlayer,
    job: &Job,
) -> Result<(), String> {
    let text_lang = detect_language(text);
    for (i, chunk) in chunk_text(text, MAX_CHUNK_LEN).iter().enumerate() {
        if job.cancel.load(Ordering::SeqCst) {
            break;
        }
        // Pro Abschnitt erkennen, damit z. B. ein englischer Satz im deutschen Text passend klingt.
        let request = Request {
            voice,
            speed,
            lang: detect_language(chunk).or(text_lang),
            auto_language,
        };
        let audio = provider.synthesize(chunk, &request)?;
        let mut samples = Vec::new();
        if i > 0 {
            samples.resize((PAUSE_SECS * audio.sample_rate as f32) as usize, 0.0);
        }
        samples.extend(audio.samples);

        if job.cancel.load(Ordering::SeqCst) {
            break;
        }
        if let Err(e) = enqueue(&samples, audio.sample_rate, list, player) {
            // Nach einem Stopp ist der Player schon geschlossen – das ist kein Fehler.
            if job.cancel.load(Ordering::SeqCst) {
                break;
            }
            return Err(format!("Wiedergabe: {e}"));
        }
    }
    Ok(())
}

/// Hängt einen Abschnitt an die Wiedergabeliste an.
fn enqueue(
    samples: &[f32],
    sample_rate: u32,
    list: &MediaPlaybackList,
    player: &MediaPlayer,
) -> windows::core::Result<()> {
    let source = MediaSource::CreateFromStream(&wav_stream(samples, sample_rate)?, &HSTRING::from("audio/wav"))?;
    let items = list.Items()?;
    items.Append(&MediaPlaybackItem::Create(&source)?)?;

    let index = items.Size()? - 1;
    if index == 0 {
        player.Play()?;
    } else if player.PlaybackSession()?.PlaybackState()? == MediaPlaybackState::Paused {
        // Die Wiedergabe war schneller als die Berechnung und ist am Ende stehen geblieben.
        list.MoveTo(index)?;
        player.Play()?;
    }
    Ok(())
}

/// Ersetzt Wörter aus dem Aussprache-Wörterbuch (ganze Wörter, Groß-/Kleinschreibung egal).
pub fn apply_pronunciations(text: &str, entries: &[Pronunciation]) -> String {
    let entries: Vec<(String, &str)> = entries
        .iter()
        .filter(|e| !e.word.trim().is_empty())
        .map(|e| (e.word.trim().to_lowercase(), e.spoken.as_str()))
        .collect();
    if entries.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let lower = word.to_lowercase();
        match entries.iter().find(|(w, _)| *w == lower) {
            Some((_, spoken)) => out.push_str(spoken),
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '-' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Erkennt die Sprache des Textes (ISO 639-1), sofern die Erkennung sicher ist.
fn detect_language(text: &str) -> Option<&'static str> {
    let info = whatlang::detect(text).filter(|i| i.is_reliable())?;
    Some(match info.lang() {
        Lang::Deu => "de",
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Spa => "es",
        Lang::Ita => "it",
        Lang::Por => "pt",
        Lang::Nld => "nl",
        Lang::Pol => "pl",
        Lang::Ces => "cs",
        Lang::Slk => "sk",
        Lang::Slv => "sl",
        Lang::Hrv => "hr",
        Lang::Hun => "hu",
        Lang::Ron => "ro",
        Lang::Bul => "bg",
        Lang::Ell => "el",
        Lang::Rus => "ru",
        Lang::Ukr => "uk",
        Lang::Tur => "tr",
        Lang::Swe => "sv",
        Lang::Dan => "da",
        Lang::Fin => "fi",
        Lang::Est => "et",
        Lang::Lit => "lt",
        Lang::Lav => "lv",
        Lang::Ind => "id",
        Lang::Vie => "vi",
        Lang::Hin => "hi",
        Lang::Ara => "ar",
        Lang::Kor => "ko",
        Lang::Jpn => "ja",
        _ => return None,
    })
}

/// Zerlegt Text in Abschnitte von höchstens `max_len` Bytes, bevorzugt an Satzenden.
/// Der erste Satz ist ein eigener Abschnitt, damit das Vorlesen schnell beginnt.
fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        // Überlange Sätze notfalls zwischen zwei Wörtern trennen.
        if !current.is_empty() && current.len() + 1 + word.len() > max_len {
            sentences.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        if word.ends_with(['.', '!', '?']) {
            sentences.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in sentences {
        if !current.is_empty() && current.len() + 1 + sentence.len() > max_len {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&sentence);
        if chunks.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Verpackt Samples als WAV (16 Bit, mono) in einen Speicher-Stream.
fn wav_stream(samples: &[f32], sample_rate: u32) -> windows::core::Result<IRandomAccessStream> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(&wav_bytes(samples, sample_rate))?;
    let buffer = writer.DetachBuffer()?;

    let stream = InMemoryRandomAccessStream::new()?;
    stream.WriteAsync(&buffer)?.join()?;
    stream.Seek(0)?;
    stream.cast()
}

fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // Länge des fmt-Blocks
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // Bytes pro Sekunde
    bytes.extend_from_slice(&2u16.to_le_bytes()); // Bytes pro Sample
    bytes.extend_from_slice(&16u16.to_le_bytes()); // Bits pro Sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    bytes
}

/// Wandelt 16-Bit-PCM (little endian, mono) in Samples um.
fn pcm16_to_samples(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_respect_max_len() {
        let text = "Erster Satz. Zweiter Satz ist etwas länger! Dritter?";
        assert_eq!(
            chunk_text(text, 1000),
            vec!["Erster Satz.", "Zweiter Satz ist etwas länger! Dritter?"]
        );
        assert_eq!(chunk_text(text, 35).len(), 3);
        assert!(chunk_text(text, 20).iter().all(|c| c.len() <= 20));
    }

    #[test]
    fn detects_language_per_sentence() {
        let text = "Heute habe ich ein langes Gespräch mit meinem Kollegen über das Projekt geführt. \
                    The meeting was really productive and everyone agreed on the next steps.";
        let chunks = chunk_text(text, MAX_CHUNK_LEN);
        assert_eq!(detect_language(&chunks[0]), Some("de"));
        assert_eq!(detect_language(&chunks[1]), Some("en"));
    }

    #[test]
    fn pronunciations_replace_whole_words() {
        let entries = vec![Pronunciation {
            word: "privat".into(),
            spoken: "priwaht".into(),
        }];
        assert_eq!(
            apply_pronunciations("Das ist Privat, nicht privatisiert.", &entries),
            "Das ist priwaht, nicht privatisiert."
        );
    }
}
