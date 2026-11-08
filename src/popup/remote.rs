//! Hinweise aus der Tray-App: Sie hat schon eine eigene Nachrichtenschleife und kann kein
//! zweites Fenster-System starten. Darum zeigt ein eigener Prozess (`--hinweis`) das Popup;
//! der Inhalt kommt als JSON-Zeilen über stdin (jede Zeile ersetzt den vorherigen Inhalt).

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use super::{show, Handle, Notice};
use crate::settings::{Notify, Settings};

pub const ARG: &str = "--hinweis";

/// Einstieg für `--hinweis`: liest Hinweise von stdin und zeigt sie an.
pub fn serve() {
    let Some(first) = read_notice() else {
        return;
    };
    let handle = Handle::new(first);
    let updates = handle.clone();
    // Nicht im Scope: Das Lesen von stdin kann blockieren, nachdem das Popup schon zu ist.
    std::thread::spawn(move || {
        while let Some(notice) = read_notice() {
            updates.set(notice);
        }
    });
    if let Err(e) = show(&handle) {
        crate::log_error(&e);
    }
    std::process::exit(0);
}

/// Liest den nächsten gültigen Hinweis von stdin; `None` am Ende.
/// Ungültiges UTF-8 wird ersetzt statt abzubrechen, ungültiges JSON übersprungen.
fn read_notice() -> Option<Notice> {
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut bytes = Vec::new();
        if stdin.read_until(b'\n', &mut bytes).ok()? == 0 {
            return None;
        }
        let line = String::from_utf8_lossy(&bytes);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Notice>(line) {
            Ok(notice) => return Some(notice),
            Err(e) => crate::log_error(&format!("Ungültiger Hinweis ({e}): {line}")),
        }
    }
}

/// Ein Popup in eigenem Prozess, das sich weiter aktualisieren lässt.
pub struct Remote {
    stdin: ChildStdin,
    _child: Child,
}

impl Remote {
    pub fn spawn(notice: &Notice) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(exe)
            .arg(ARG)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Hinweis starten: {e}"))?;
        let stdin = child.stdin.take().ok_or("Hinweis: kein stdin")?;
        let mut remote = Self { stdin, _child: child };
        remote.set(notice)?;
        Ok(remote)
    }

    pub fn set(&mut self, notice: &Notice) -> Result<(), String> {
        let json = serde_json::to_string(notice).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{json}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("Hinweis aktualisieren: {e}"))
    }
}

/// Zeigt einen einmaligen Hinweis so, wie es in den Einstellungen gewählt ist
/// (eigenes Popup oder Windows-Benachrichtigung). Fällt bei Problemen auf das andere zurück.
pub fn notify(notice: Notice) {
    let popup_first = Settings::load().notify == Notify::Popup;
    if popup_first && Remote::spawn(&notice).is_ok() {
        return;
    }
    if crate::toast::show_notice("hinweis", &notice).is_ok() {
        return;
    }
    if !popup_first && Remote::spawn(&notice).is_ok() {
        return;
    }
    crate::beep_ok();
}
