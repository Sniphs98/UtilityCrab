//! Was der Benutzer nach (und während) einer Konvertierung zu sehen bekommt.
//!
//! Regeln: Der Titel sagt, was passiert ist. Das Ergebnis ist die größte Zahl.
//! Unerwartetes (größer geworden, nichts geändert, Fehler) wird in einem Satz erklärt.

use std::path::{Path, PathBuf};

use super::batch::Job;
use super::{human_size, Format, Outcome};
use crate::popup::{Action, Block, Icon, Kind, Notice, Tone};

/// Während der Arbeit: Titel plus Fortschrittsbalken, z. B. „3 von 5 · IMG_9612.jpg“.
pub fn running(job: Job, files: &[PathBuf], progress: &[f32]) -> Notice {
    let n = files.len();
    let title = match (job, n) {
        (Job::ConvertTo(f), 1) => format!("Wird in {} umgewandelt", f.short()),
        (Job::ConvertTo(f), _) => format!("{n} Bilder werden in {} umgewandelt", f.short()),
        (_, 1) => "Bild wird verkleinert".to_string(),
        (_, _) => format!("{n} Bilder werden verkleinert"),
    };
    let fraction = progress.iter().sum::<f32>() / n.max(1) as f32;
    let complete = progress.iter().filter(|&&p| p >= 1.0).count();
    // Die Datei, an der gerade gearbeitet wird (bzw. die nächste).
    let current = progress
        .iter()
        .position(|&p| p > 0.0 && p < 1.0)
        .or_else(|| progress.iter().position(|&p| p < 1.0))
        .map(|i| file_name(&files[i]));
    let label = match (n, current) {
        (1, _) => file_name(&files[0]),
        (_, Some(name)) => format!("{complete} von {n} · {name}"),
        (_, None) => format!("{complete} von {n}"),
    };
    Notice::new(Kind::Running, title).block(Block::Progress { fraction, label })
}

/// Nach der Arbeit. `copied`: Die neuen Dateien liegen in der Zwischenablage.
pub fn finished(job: Job, files: &[PathBuf], results: &[Result<Outcome, String>], copied: bool) -> Notice {
    let n = files.len();
    let mut written = Vec::new();
    let mut unchanged = Vec::new();
    let mut errors = Vec::new();
    for (file, result) in files.iter().zip(results) {
        match result {
            Ok(Outcome::Written { path, before, after }) => written.push((file, path, *before, *after)),
            Ok(Outcome::AlreadySmall { .. }) => unchanged.push(file),
            Err(e) => errors.push((file, e.clone())),
        }
    }
    let w = written.len();
    let verb = match job {
        Job::ConvertTo(f) => format!("in {} umgewandelt", f.short()),
        _ => "verkleinert".to_string(),
    };

    // Nichts geschrieben: Fehler oder „schon optimal“.
    if w == 0 {
        if errors.is_empty() {
            let mut notice = Notice::new(Kind::Info, "Schon optimal – nichts geändert");
            if n == 1 {
                notice = notice.subtitle(file_name(files[0].as_path()));
            }
            return notice
                .block(Block::Text(if n == 1 {
                    "Mit den Standard-Einstellungen wird diese Datei nicht kleiner.".into()
                } else {
                    "Mit den Standard-Einstellungen wird keine dieser Dateien kleiner.".into()
                }))
                .block(Block::Hint(
                    "Mehr geht über „Weitere Optionen …“, z. B. mit kleinerer Bildgröße oder als WebP.".into(),
                ));
        }
        let title = if n == 1 {
            format!("Konnte nicht {verb} werden")
        } else {
            format!("Keine Bilder {verb}")
        };
        let mut notice = Notice::new(Kind::Error, title);
        if n == 1 {
            notice = notice
                .subtitle(file_name(&files[0]))
                .block(Block::Text(without_name(&errors[0].1).to_string()));
        } else {
            notice = notice.block(problems(&errors, &unchanged));
        }
        return notice;
    }

    let title = match (w, n) {
        (1, 1) => match job {
            Job::ConvertTo(f) => format!("In {} umgewandelt", f.short()),
            _ => "Bild verkleinert".to_string(),
        },
        (w, n) if w == n => format!("{w} Bilder {verb}"),
        (w, n) => format!("{w} von {n} Bildern {verb}"),
    };
    let kind = if errors.is_empty() && unchanged.is_empty() {
        Kind::Success
    } else {
        Kind::Warning
    };
    let mut notice = Notice::new(kind, title);

    if w == 1 {
        let (input, output, ..) = written[0];
        let mut subtitle = file_name(output);
        if Format::of_path(input) != Format::of_path(output) {
            subtitle = format!("{subtitle} · aus {}", ext_upper(input));
        }
        notice = notice.subtitle(subtitle);
    }

    let before: u64 = written.iter().map(|(_, _, b, _)| b).sum();
    let after: u64 = written.iter().map(|(_, _, _, a)| a).sum();
    // Größer geworden ist bei einem Formatwechsel normal; die Erklärung steht beim Badge.
    let badge_hint = if after > before {
        written
            .iter()
            .find(|(_, _, b, a)| a > b)
            .map(|(input, output, ..)| bigger_hint(input, output))
    } else {
        None
    };
    notice = notice.block(Block::Stat {
        before: human_size(before),
        after: human_size(after),
        badge: Some(change_badge(before, after)),
        badge_hint,
    });
    if copied {
        notice = notice.block(Block::Chip {
            icon: Icon::Clipboard,
            text: "In Zwischenablage".into(),
        });
    }
    if !errors.is_empty() || !unchanged.is_empty() {
        notice = notice.block(problems(&errors, &unchanged));
    }
    notice.action(Action::RevealFile {
        label: "Im Ordner zeigen".into(),
        path: written[0].1.clone(),
    })
}

/// Aufklappbare Liste der Dateien, die nicht geklappt haben.
fn problems(errors: &[(&PathBuf, String)], unchanged: &[&PathBuf]) -> Block {
    let count = errors.len() + unchanged.len();
    let label = if count == 1 {
        "1 Datei übersprungen".to_string()
    } else {
        format!("{count} Dateien übersprungen")
    };
    let mut items: Vec<String> = errors.iter().map(|(_, e)| e.clone()).collect();
    items.extend(unchanged.iter().map(|f| format!("{}: schon optimal, nichts geändert", file_name(f))));
    Block::Details { label, items }
}

/// „−75 %“ grün, „+22 %“ gelb, „±0 %“ grau.
fn change_badge(before: u64, after: u64) -> (String, Tone) {
    let change = (after as f64 / before.max(1) as f64 - 1.0) * 100.0;
    let magnitude = change.abs();
    // Bei fast 100 % eine Nachkommastelle, sonst stünde dort „−100 %“.
    let number = if (99.0..100.0).contains(&magnitude) {
        format!("{magnitude:.1}").replace('.', ",")
    } else {
        format!("{magnitude:.0}")
    };
    if number == "0" {
        ("±0 %".into(), Tone::Neutral)
    } else if change < 0.0 {
        (format!("−{number} %"), Tone::Good)
    } else {
        (format!("+{number} %"), Tone::Warn)
    }
}

fn bigger_hint(input: &Path, output: &Path) -> String {
    let source = ext_upper(input);
    let target = ext_upper(output);
    let lossless = matches!(Format::of_path(output), Some(Format::Png | Format::Bmp | Format::Tiff | Format::Ico));
    if lossless {
        format!(
            "{target} speichert ohne Qualitätsverlust und ist darum größer als {source}. \
             Kleiner geht es mit JPG oder WebP."
        )
    } else {
        format!(
            "{target} speichert dieses Bild weniger effizient als {source}. \
             Kleiner geht es über „Weitere Optionen …“ mit niedrigerer Qualität."
        )
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn ext_upper(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("jpg" | "jpeg" | "jfif") => "JPG".into(),
        Some("tif" | "tiff") => "TIFF".into(),
        Some("webp") => "WebP".into(),
        Some(other) => other.to_ascii_uppercase(),
        None => "dem Original".into(),
    }
}

/// Fehlertexte beginnen mit „datei.jpg: …“ – im Popup steht der Name schon im Untertitel.
fn without_name(error: &str) -> &str {
    error.split_once(": ").map(|(_, rest)| rest).unwrap_or(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(input: &str, output: &str, before: u64, after: u64) -> (PathBuf, Result<Outcome, String>) {
        (
            PathBuf::from(input),
            Ok(Outcome::Written {
                path: PathBuf::from(output),
                before,
                after,
            }),
        )
    }

    #[test]
    fn shrink_single_file() {
        let (file, result) = written("a.jpg", "a-klein.jpg", 2_000_000, 500_000);
        let n = finished(Job::Shrink, &[file], &[result], true);
        assert_eq!(n.kind, Kind::Success);
        assert_eq!(n.title, "Bild verkleinert");
        assert_eq!(n.subtitle.as_deref(), Some("a-klein.jpg"));
        assert!(matches!(&n.blocks[0], Block::Stat { badge: Some((b, Tone::Good)), .. } if b == "−75 %"));
    }

    #[test]
    fn bigger_after_conversion_is_explained() {
        let (file, result) = written("IMG.heic", "IMG.jpg", 1_700_000, 2_100_000);
        let n = finished(Job::ConvertTo(Format::Jpg), &[file], &[result], true);
        assert_eq!(n.title, "In JPG umgewandelt");
        assert_eq!(n.subtitle.as_deref(), Some("IMG.jpg · aus HEIC"));
        assert!(matches!(&n.blocks[0], Block::Stat { badge: Some((b, Tone::Warn)), .. } if b == "+24 %"));
        assert!(matches!(&n.blocks[0], Block::Stat { badge_hint: Some(h), .. } if h.contains("HEIC")));
    }

    #[test]
    fn partial_failure_lists_problems() {
        let (a, ra) = written("a.png", "a.webp", 1000, 500);
        let b = PathBuf::from("b.heic");
        let rb = Err("b.heic: HEIC kann nicht gelesen werden".to_string());
        let n = finished(Job::ConvertTo(Format::Webp), &[a, b], &[ra, rb], false);
        assert_eq!(n.kind, Kind::Warning);
        assert_eq!(n.title, "1 von 2 Bildern in WebP umgewandelt");
        assert!(n.blocks.iter().any(|b| matches!(b, Block::Details { label, .. } if label == "1 Datei übersprungen")));
    }

    #[test]
    fn running_label_names_current_file() {
        let files = [PathBuf::from("a.jpg"), PathBuf::from("b.jpg"), PathBuf::from("c.jpg")];
        let n = running(Job::Shrink, &files, &[1.0, 0.4, 0.0]);
        assert_eq!(n.title, "3 Bilder werden verkleinert");
        assert_eq!(n.progress(), Some((0.46666667, "1 von 3 · b.jpg")));
    }
}
