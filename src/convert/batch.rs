//! Aufrufe aus dem Explorer-Kontextmenü.
//!
//! Explorer startet für jede markierte Datei einen eigenen Prozess. Damit trotzdem nur ein
//! Fenster bzw. eine Meldung erscheint, legt jeder Prozess seine Dateien in eine Warteschlange.
//! Wer die Sperrdatei bekommt, arbeitet alles ab; die anderen beenden sich sofort.

use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::thread;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use rayon::prelude::*;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

use super::{convert_file, notice, Format, Options, Outcome};
use crate::settings::{Notify, Settings};
use crate::{popup, toast};

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
            Job::ConvertTo(format) => process(&files, &Options::convert_to(format), job),
            Job::Shrink => process(&files, &Options::shrink(), job),
        }
        drop(lock);
        // Wer sich nach dem letzten Abholen eingereiht hat, sah die Sperre noch belegt –
        // also hier noch einmal nachsehen.
        if queue.is_empty() {
            return;
        }
    }
}

/// Wandelt die Dateien parallel um, legt die Ergebnisse in die Zwischenablage und zeigt
/// Fortschritt und Ergebnis – als eigenes Popup unten mittig oder als Windows-Benachrichtigung.
fn process(files: &[PathBuf], options: &Options, job: Job) {
    if files.is_empty() {
        return;
    }
    let initial = notice::running(job, files, &vec![0.0; files.len()]);

    if Settings::load().notify == Notify::Popup {
        let result = popup::run(initial, |handle| {
            let done = work(files, options, job, &mut |n| handle.set(n.clone()));
            handle.set(done);
        });
        match result {
            Ok(()) => return,
            // Kein Fenster möglich: Ergebnis (die Arbeit ist schon fertig) als Toast zeigen.
            Err((e, last)) => {
                crate::log_error(&e);
                finish_with_toast(&mut toast::NoticeSink::new(&toast_tag()), &last);
            }
        }
        return;
    }

    let mut sink = toast::NoticeSink::new(&toast_tag());
    let done = work(files, options, job, &mut |n| {
        let _ = sink.publish(n);
    });
    finish_with_toast(&mut sink, &done);
}

fn toast_tag() -> String {
    format!("konvertieren-{}", std::process::id())
}

/// Ergebnis als Toast; sind Benachrichtigungen abgeschaltet, Ton bzw. Meldungsfenster.
fn finish_with_toast(sink: &mut toast::NoticeSink, notice: &popup::Notice) {
    if sink.publish(notice).is_ok() {
        return;
    }
    if matches!(notice.kind, popup::Kind::Success | popup::Kind::Running) {
        crate::beep_ok();
    } else {
        let text = format!("{}\n\n{}", notice.title, notice.plain_lines().join("\n"));
        unsafe {
            MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("UtilityCrab"), MB_OK | MB_ICONWARNING);
        }
    }
}

/// Die eigentliche Arbeit: meldet laufend den Fortschritt und gibt das Ergebnis zurück.
fn work(files: &[PathBuf], options: &Options, job: Job, publish: &mut dyn FnMut(&popup::Notice)) -> popup::Notice {
    // Fortschritt je Datei in Promille.
    let progress: Vec<AtomicU32> = files.iter().map(|_| AtomicU32::new(0)).collect();
    let results: Mutex<Option<Vec<Result<Outcome, String>>>> = Mutex::new(None);

    thread::scope(|scope| {
        scope.spawn(|| {
            let done = files
                .par_iter()
                .zip(&progress)
                .map(|(file, slot)| convert_file(file, options, &|p| slot.store((p * 1000.0) as u32, Ordering::Relaxed)))
                .collect();
            *results.lock().unwrap() = Some(done);
        });
        while results.lock().unwrap().is_none() {
            let fractions: Vec<f32> = progress.iter().map(|p| p.load(Ordering::Relaxed) as f32 / 1000.0).collect();
            publish(&notice::running(job, files, &fractions));
            thread::sleep(Duration::from_millis(100));
        }
    });

    let results = results.into_inner().unwrap().unwrap_or_default();
    let written: Vec<PathBuf> = results
        .iter()
        .filter_map(|r| match r {
            Ok(Outcome::Written { path, .. }) => Some(path.clone()),
            _ => None,
        })
        .collect();
    for error in results.iter().filter_map(|r| r.as_ref().err()) {
        crate::log_error(error);
    }
    let copied = !written.is_empty() && super::copy_to_clipboard(&written).is_ok();
    notice::finished(job, files, &results, copied)
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
