//! `--hinweis-demo [name]`: zeigt Beispiel-Hinweise, um das Design zu prüfen.
//! Ohne Namen laufen alle nacheinander durch, mit Namen bleibt einer stehen.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use super::{run as run_popup, show, Action, Block, Handle, Icon, Kind, Notice, Tone};

pub const ARG: &str = "--hinweis-demo";

fn samples() -> Vec<(&'static str, Notice)> {
    let reveal = Action::RevealFile {
        label: "Im Ordner zeigen".into(),
        path: PathBuf::from(r"C:\Windows\explorer.exe"),
    };
    let clipboard = Block::Chip {
        icon: Icon::Clipboard,
        text: "In Zwischenablage".into(),
    };
    vec![
        (
            "lauf",
            Notice::new(Kind::Running, "5 Bilder werden verkleinert").block(Block::Progress {
                fraction: 0.58,
                label: "3 von 5 · IMG_9612.jpg".into(),
            }),
        ),
        (
            "klein",
            Notice::new(Kind::Success, "Bild verkleinert")
                .subtitle("IMG_9610-klein.jpg")
                .block(Block::Stat {
                    before: "1,7 MB".into(),
                    after: "420 KB".into(),
                    badge: Some(("−75 %".into(), Tone::Good)),
                    badge_hint: None,
                })
                .block(clipboard.clone())
                .action(reveal.clone()),
        ),
        (
            "groesser",
            Notice::new(Kind::Success, "In JPG umgewandelt")
                .subtitle("IMG_9610 (2).jpg · aus HEIC")
                .block(Block::Stat {
                    before: "1,7 MB".into(),
                    after: "2,1 MB".into(),
                    badge: Some(("+22 %".into(), Tone::Warn)),
                    badge_hint: Some(
                        "JPG speichert dieses Bild weniger effizient als HEIC. \
                         Kleiner geht es über „Weitere Optionen …“ mit niedrigerer Qualität."
                            .into(),
                    ),
                })
                .block(clipboard.clone())
                .action(reveal.clone()),
        ),
        (
            "teilweise",
            Notice::new(Kind::Warning, "4 von 5 Bildern verkleinert")
                .block(Block::Stat {
                    before: "12,4 MB".into(),
                    after: "3,1 MB".into(),
                    badge: Some(("−75 %".into(), Tone::Good)),
                    badge_hint: None,
                })
                .block(clipboard.clone())
                .block(Block::Details {
                    label: "1 Datei übersprungen".into(),
                    items: vec![
                        "urlaub.heic: HEIC kann nicht gelesen werden. Bitte die „HEIF-Bilderweiterungen“ \
                         aus dem Microsoft Store installieren."
                            .into(),
                    ],
                })
                .action(reveal.clone()),
        ),
        (
            "optimal",
            Notice::new(Kind::Info, "Schon optimal – nichts geändert")
                .subtitle("logo.png")
                .block(Block::Text("Mit den Standard-Einstellungen wird diese Datei nicht kleiner.".into()))
                .block(Block::Hint(
                    "Mehr geht über „Weitere Optionen …“, z. B. mit kleinerer Bildgröße oder als WebP.".into(),
                )),
        ),
        (
            "fehler",
            Notice::new(Kind::Error, "Konnte nicht in PNG umgewandelt werden")
                .subtitle("urlaub.heic")
                .block(Block::Text(
                    "HEIC kann nicht gelesen werden. Bitte die „HEIF-Bilderweiterungen“ aus dem Microsoft Store installieren."
                        .into(),
                )),
        ),
        (
            "text",
            Notice::new(Kind::Success, "Text kopiert")
                .subtitle("128 Zeichen · 3 Zeilen")
                .block(Block::Quote(
                    "Sehr geehrte Damen und Herren,\nvielen Dank für Ihre Nachricht vom 3. November.\n\
                     Anbei finden Sie die gewünschten Unterlagen."
                        .into(),
                ))
                .block(clipboard),
        ),
        (
            "update",
            crate::update::available_notice(&crate::update::Release {
                version: "0.1.0.13".into(),
                setup_url: String::new(),
                setup_size: 0,
                page_url: String::new(),
            }),
        ),
        ("update-laden", crate::update::downloading_notice("0.1.0.13", 0.64)),
        ("update-voll", crate::update::downloading_notice("0.1.0.13", 1.0)),
        ("update-installieren", crate::update::installing_notice("0.1.0.13")),
        (
            "update-fehler",
            crate::update::failed("Das Setup konnte nicht geladen werden. Bitte später noch einmal versuchen."),
        ),
        ("update-aktuell", crate::update::check_notice(&Ok(None))),
        (
            "update-suche-fehler",
            crate::update::check_notice(&Err("GitHub ist gerade nicht erreichbar.".into())),
        ),
        ("update-suche", Notice::new(Kind::Running, "Suche nach Updates …")),
    ]
}

/// Namen aller Beispiele, für die Fehlermeldung bei einem unbekannten Namen.
fn names() -> String {
    samples().iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}

pub fn run(name: Option<&str>) {
    let all = samples();
    if let Some(name) = name {
        let Some((_, mut notice)) = all.into_iter().find(|(n, _)| *n == name) else {
            eprintln!("Unbekannt. Möglich: {}", names());
            return;
        };
        notice.timeout_secs = None;
        let _ = show(&Handle::new(notice));
        return;
    }
    let first = all[0].1.clone();
    let _ = run_popup(first, |handle| {
        // Fortschritt hochlaufen lassen …
        for i in 0..=40 {
            if handle.is_closed() {
                return;
            }
            let mut n = all[0].1.clone();
            n.blocks = vec![Block::Progress {
                fraction: i as f32 / 40.0,
                label: format!("{} von 5 · IMG_96{:02}.jpg", i / 8, 10 + i / 8),
            }];
            handle.set(n);
            thread::sleep(Duration::from_millis(60));
        }
        // … dann jedes Ergebnis ein paar Sekunden zeigen.
        for (_, mut notice) in all.into_iter().skip(1) {
            if handle.is_closed() {
                return;
            }
            notice.timeout_secs = Some(3.5);
            handle.set(notice);
            thread::sleep(Duration::from_millis(3200));
        }
    });
}

