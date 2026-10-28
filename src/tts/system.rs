//! Die in Windows eingebauten Stimmen.

use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceInformation};
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};

use super::{pcm16_to_samples, Audio, Provider, Request, VoiceInfo};

pub struct System;

impl Provider for System {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn name(&self) -> &'static str {
        "Windows-Stimmen"
    }

    fn description(&self) -> &'static str {
        "In Windows eingebaut, kein Download nötig. Weitere Stimmen gibt es in den \
         Windows-Einstellungen unter „Zeit und Sprache > Sprache“."
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        let Ok(voices) = SpeechSynthesizer::AllVoices() else {
            return Vec::new();
        };
        voices
            .into_iter()
            .filter_map(|v| {
                Some(VoiceInfo {
                    id: v.Id().ok()?.to_string(),
                    name: format!("{} ({})", v.DisplayName().ok()?, v.Language().ok()?),
                })
            })
            .collect()
    }

    fn synthesize(&self, text: &str, request: &Request) -> Result<Audio, String> {
        synthesize(text, request).map_err(|e| format!("Windows-Sprachausgabe: {e}"))
    }
}

fn synthesize(text: &str, request: &Request) -> windows::core::Result<Audio> {
    let synth = SpeechSynthesizer::new()?;
    let voices: Vec<_> = SpeechSynthesizer::AllVoices()?.into_iter().collect();
    let speaks = |v: &VoiceInformation, lang: &str| v.Language().is_ok_and(|l| l.to_string().starts_with(lang));
    let selected = voices.iter().find(|v| v.Id().is_ok_and(|id| id == request.voice));
    let voice = match request.lang {
        // Passt die gewählte Stimme nicht zur erkannten Sprache, eine passende nehmen.
        Some(lang) if request.auto_language && !selected.is_some_and(|v| speaks(v, lang)) => {
            voices.iter().find(|v| speaks(v, lang)).or(selected)
        }
        _ => selected,
    };
    if let Some(voice) = voice {
        synth.SetVoice(voice)?;
    }
    synth.Options()?.SetSpeakingRate(request.speed.clamp(0.5, 6.0) as f64)?;

    let stream = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text))?.join()?;
    let size = stream.Size()? as u32;
    let buffer = stream
        .ReadAsync(&Buffer::Create(size)?, size, InputStreamOptions::None)?
        .join()?;
    let mut wav = vec![0u8; buffer.Length()? as usize];
    DataReader::FromBuffer(&buffer)?.ReadBytes(&mut wav)?;

    parse_wav(&wav).ok_or_else(|| windows::core::Error::new(windows::core::HRESULT(-1), "unbekanntes Audioformat"))
}

/// Liest eine WAV-Datei mit 16-Bit-PCM; bei Stereo wird nur der erste Kanal benutzt.
fn parse_wav(bytes: &[u8]) -> Option<Audio> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);

    let (mut channels, mut sample_rate, mut bits) = (1usize, 0u32, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(bytes, pos + 4) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            channels = u16_at(body, 2).max(1) as usize;
            sample_rate = u32_at(body, 4);
            bits = u16_at(body, 14);
        } else if id == b"data" {
            if bits != 16 || sample_rate == 0 {
                return None;
            }
            let mono: Vec<u8> = body
                .chunks_exact(2 * channels)
                .flat_map(|frame| [frame[0], frame[1]])
                .collect();
            return Some(Audio {
                samples: pcm16_to_samples(&mono),
                sample_rate,
            });
        }
        pos += 8 + size + (size & 1);
    }
    None
}
