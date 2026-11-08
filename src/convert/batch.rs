//! Aufrufe aus dem Explorer-Kontextmenü.
//!
//! Explorer startet für jede markierte Datei einen eigenen Prozess. Damit trotzdem nur ein
//! Fenster bzw. eine Meldung erscheint, legt jeder Prozess seine Dateien in eine Warteschlange.
//! Wer die Sperrdatei bekommt, arbeitet alles ab; die anderen beenden sich sofort.

use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

use rayon::prelude::*;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

use super::{convert_file, describe, size_change, Format, Options, Outcome};
use crate::toast;

/// Was mit den Dateien passieren soll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Job {
    ConvertTo(Format),
    Shrink,
    /// Fenster mit allen Einstellungen öffnen.
    Dialog,
}

impl Job {
    pub fn id(self) -> &'static str {
        match self {
            Job::ConvertTo(format) => format.id(),
            Job::Shrink => "verkleinern",
            Job::Dialog => "fenster",
        }
    }

    fn parse(id: &str) -> Option<Self> {
        match id {
            "verkleinern" => Some(Job::Shrink),
            "fenster" => Some(Job::Dialog),
            other => Format::from_id(other).map(Job::ConvertTo),
        }
    }
}

/// Einstieg für `--konvertieren <auftrag> [dateien …]`.
pub fn run(args: &[String]) {
    let Some(job) = args.first().and_then(|a| Job::parse(a)) else {
        crate::report(Err(format!("Unbekannter Konvertier-Auftrag: {args:?}")));
        return;
    };
    let files: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
    let queue = Queue::new(job);
    if !files.is_empty() {
        if let Err(e) = queue.push(&files) {
            crate::report(Err(e));
            return;
        }
    }

    loop {
        let Some(lock) = queue.try_lock() else {
            // Ein anderer Prozess arbeitet die Warteschlange ab (bzw. das Fenster ist schon offen).
            return;
        };
        // Kurz warten, bis die Prozesse der übrigen markierten Dateien sich eingereiht haben.
        if !queue.is_empty() {
            thread::sleep(Duration::from_millis(400));
        }
        let files = queue.drain();
        match job {
            Job::Dialog => super::ui::run(files, Some(queue.clone())),
            Job::ConvertTo(format) => process(&files, &Options::convert_to(format)),
            Job::Shrink => process(&files, &Options::shrink()),
        }
        drop(lock);
        // Wer sich nach dem letzten Abholen eingereiht hat, sah die Sperre noch belegt –
        // also hier noch einmal nachsehen.
        if queue.is_empty() {
            return;
        }
    }
}

/// Wandelt die Dateien parallel um, legt die Ergebnisse in die Zwischenablage und meldet
/// sich per Toast. Dauert es länger als eine Sekunde, läuft vorher ein Fortschrittsbalken mit.
fn process(files: &[PathBuf], options: &Options) {
    if files.is_empty() {
        return;
    }
    // Fortschritt je Datei in Promille.
    let progress: Vec<AtomicU32> = files.iter().map(|_| AtomicU32::new(0)).collect();
    let finished = AtomicBool::new(false);
    let tag = format!("konvertieren-{}", std::process::id());

    let results: Vec<Result<Outcome, String>> = thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let results = files
                .par_iter()
                .zip(&progress)
                .map(|(file, slot)| convert_file(file, options, &|p| slot.store((p * 1000.0) as u32, Ordering::Relaxed)))
                .collect();
            finished.store(true, Ordering::SeqCst);
            results
        });

        let started = Instant::now();
        let mut toast = None;
        while !finished.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
            if started.elapsed() < Duration::from_secs(1) {
                continue;
            }
            let done: Vec<u32> = progress.iter().map(|p| p.load(Ordering::Relaxed)).collect();
            let fraction = done.iter().sum::<u32>() as f32 / (1000 * files.len()) as f32;
            let status = if files.len() == 1 {
                file_name(&files[0])
            } else {
                let complete = done.iter().filter(|&&p| p >= 1000).count();
                format!("{complete} von {} Bildern", files.len())
            };
            if toast.is_none() {
                let title = if files.len() == 1 { "Bild wird konvertiert …" } else { "Bilder werden konvertiert …" };
                // Ein fehlgeschlagener Toast wird nicht jedes Mal neu versucht.
                toast = Some(toast::progress(&tag, title).ok());
            }
            if let Some(Some(toast)) = toast.as_mut() {
                toast.update(fraction, &status);
            }
            thread::sleep(Duration::from_millis(150));
        }
        worker.join().expect("Konvertierung abgestürzt")
    });

    let mut written = Vec::new();
    let (mut before, mut after) = (0, 0);
    let mut problems = Vec::new();
    for (file, result) in files.iter().zip(&results) {
        match result {
            Ok(Outcome::Written { path, before: b, after: a }) => {
                written.push(path.clone());
                before += b;
                after += a;
            }
            Ok(outcome @ Outcome::AlreadySmall { .. }) => problems.push(format!("{}: {}", file_name(file), describe(outcome))),
            Err(e) => {
                crate::log_error(e);
                problems.push(e.clone());
            }
        }
    }

    let copied = !written.is_empty() && super::copy_to_clipboard(&written).is_ok();
    let title = match (written.len(), files.len()) {
        (0, 1) => "Nicht konvertiert".to_string(),
        (0, _) => "Keine Bilder konvertiert".to_string(),
        (1, 1) => format!("Fertig: {}", file_name(&written[0])),
        (w, n) if w == n => format!("{w} Bilder fertig"),
        (w, n) => format!("{w} von {n} Bildern fertig"),
    };
    let mut lines = Vec::new();
    if !written.is_empty() {
        let sizes = size_change(before, after);
        lines.push(if copied { format!("{sizes} · in der Zwischenablage") } else { sizes });
    }
    lines.extend(problems.iter().take(2).cloned());
    if problems.len() > 2 {
        lines.push(format!("… und {} weitere (siehe Fehlerprotokoll)", problems.len() - 2));
    }

    if toast::show(&tag, &title, &lines.join("
")).is_err() {
        // Benachrichtigungen abgeschaltet: wie früher Ton bzw. Meldungsfenster.
        if problems.is_empty() {
            crate::beep_ok();
        } else {
            let text = problems.join("

");
            unsafe {
                MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("UtilityCrab"), MB_OK | MB_ICONWARNING);
            }
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Warteschlange pro Auftrag: ein Ordner mit je einer Textdatei pro Prozess.
#[derive(Clone)]
pub struct Queue {
    dir: PathBuf,
}

impl Queue {
    pub fn new(job: Job) -> Self {
        Self {
            dir: crate::data_dir().join("warteschlange").join(job.id()),
        }
    }

    fn push(&self, files: &[PathBuf]) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("Warteschlange anlegen: {e}"))?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("{}-{stamp}", std::process::id());
        let text: Vec<String> = files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        // Erst unter anderem Namen schreiben, damit nie eine halbe Datei gelesen wird.
        let tmp = self.dir.join(format!("{name}.tmp"));
        fs::write(&tmp, text.join("\n")).map_err(|e| format!("Warteschlange: {e}"))?;
        fs::rename(&tmp, self.dir.join(format!("{name}.txt"))).map_err(|e| format!("Warteschlange: {e}"))
    }

    /// Exklusiv geöffnete Sperrdatei; solange sie offen ist, gehört die Warteschlange diesem Prozess.
    fn try_lock(&self) -> Option<File> {
        fs::create_dir_all(&self.dir).ok()?;
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .open(self.dir.join("sperre"))
            .ok()
    }

    fn entries(&self) -> Vec<PathBuf> {
        let Ok(dir) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut entries: Vec<PathBuf> = dir
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "txt"))
            .collect();
        entries.sort();
        entries
    }

    fn is_empty(&self) -> bool {
        self.entries().is_empty()
    }

    /// Holt alle eingereihten Dateien ab. Liegengebliebene Einträge (älter als eine Minute) werden verworfen.
    pub fn drain(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in self.entries() {
            let fresh = fs::metadata(&entry)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age < Duration::from_secs(60));
            if fresh {
                if let Ok(text) = fs::read_to_string(&entry) {
                    files.extend(text.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
                }
            }
            let _ = fs::remove_file(&entry);
        }
        files
    }
}
