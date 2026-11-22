//! Schnellsteuerung fürs Vorlesen: eine kleine Karte im Stil der Hinweise mit Pause/Weiter,
//! Stopp, Lautstärke und Stumm. Sie ist nur zu sehen, solange UtilityCrab etwas vorliest.
//!
//! Wie die Hinweise läuft das Fenster als eigener Prozess (`--wiedergabe`), weil die
//! Tray-App kein zweites Fenster-System starten kann:
//! - Tray → Steuerung: der aktuelle [`State`] als JSON-Zeilen über stdin. Schließt der Tray
//!   stdin (Vorlesen vorbei), verschwindet die Steuerung.
//! - Steuerung → Tray: Knopfdrücke als [`Control`]-JSON-Zeilen über stdout.

mod overlay;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub const ARG: &str = "--wiedergabe";

/// Was die Steuerung anzeigt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub paused: bool,
    /// 0.0 bis 1.0.
    pub volume: f32,
    pub muted: bool,
    /// Anfang des vorgelesenen Textes.
    pub text: String,
}

/// Ein Knopfdruck in der Steuerung.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Control {
    TogglePause,
    Stop,
    SetVolume(f32),
    SetMuted(bool),
    OpenSettings,
    /// Steuerung für dieses Vorlesen ausblenden (es läuft weiter).
    Hide,
}

/// Die Steuerung aus Sicht der Tray-App.
pub struct Overlay {
    stdin: ChildStdin,
    child: Child,
    last: Option<State>,
}

impl Overlay {
    /// Startet die Steuerung; `on_control` läuft in einem eigenen Thread für jeden Knopfdruck.
    pub fn spawn(on_control: impl Fn(Control) + Send + 'static) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(exe)
            .arg(ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Schnellsteuerung starten: {e}"))?;
        let stdin = child.stdin.take().ok_or("Schnellsteuerung: kein stdin")?;
        let stdout = child.stdout.take().ok_or("Schnellsteuerung: kein stdout")?;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                // Andere Ausgaben (z. B. von Bibliotheken) einfach überspringen.
                if let Ok(control) = serde_json::from_str::<Control>(line.trim()) {
                    on_control(control);
                }
            }
        });
        Ok(Self { stdin, child, last: None })
    }

    /// Schickt den Zustand, falls er sich geändert hat.
    pub fn set(&mut self, state: &State) -> Result<(), String> {
        if self.last.as_ref() == Some(state) {
            return Ok(());
        }
        let json = serde_json::to_string(state).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{json}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("Schnellsteuerung aktualisieren: {e}"))?;
        self.last = Some(state.clone());
        Ok(())
    }

    /// Läuft das Fenster noch (oder wurde es z. B. abgeschossen)?
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Kurzer Ausschnitt des vorgelesenen Textes für die Anzeige.
pub fn excerpt(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 80;
    if flat.chars().count() <= MAX {
        flat
    } else {
        let cut: String = flat.chars().take(MAX).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Zustand im Steuerungs-Prozess: zuletzt vom Tray gemeldet, und ob stdin zu ist.
struct Shared {
    state: State,
    version: u64,
    ended: bool,
}

#[derive(Clone)]
struct Link(Arc<Mutex<Shared>>);

impl Link {
    fn snapshot(&self) -> (State, u64, bool) {
        let shared = self.0.lock().unwrap();
        (shared.state.clone(), shared.version, shared.ended)
    }

    /// Schickt einen Knopfdruck an den Tray.
    fn send(&self, control: Control) {
        if let Ok(json) = serde_json::to_string(&control) {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{json}").and_then(|_| out.flush());
        }
    }
}

/// Einstieg für `--wiedergabe`.
pub fn serve() {
    let Some(first) = read_state() else {
        return;
    };
    let link = Link(Arc::new(Mutex::new(Shared {
        state: first,
        version: 0,
        ended: false,
    })));
    let updates = link.clone();
    std::thread::spawn(move || {
        while let Some(state) = read_state() {
            let mut shared = updates.0.lock().unwrap();
            shared.state = state;
            shared.version += 1;
        }
        updates.0.lock().unwrap().ended = true;
    });
    if let Err(e) = overlay::show(link) {
        crate::log_error(&e);
    }
    std::process::exit(0);
}

fn read_state() -> Option<State> {
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut line = String::new();
        if stdin.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<State>(line) {
            Ok(state) => return Some(state),
            Err(e) => crate::log_error(&format!("Ungültiger Wiedergabe-Zustand ({e}): {line}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_flattens_and_shortens() {
        assert_eq!(excerpt("Hallo\n  Welt"), "Hallo Welt");
        let long = "Wort ".repeat(40);
        let short = excerpt(&long);
        assert!(short.ends_with('…'));
        assert!(short.chars().count() <= 81);
    }

    #[test]
    fn controls_round_trip() {
        for control in [Control::TogglePause, Control::SetVolume(0.4), Control::SetMuted(true), Control::Hide] {
            let json = serde_json::to_string(&control).unwrap();
            assert_eq!(serde_json::from_str::<Control>(&json).unwrap(), control);
        }
    }
}
