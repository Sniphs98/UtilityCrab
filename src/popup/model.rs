//! Was ein Hinweis anzeigt – unabhängig davon, wer ihn erzeugt (Konverter, Texterkennung …).
//! Serialisierbar, damit die Tray-App ihn per `--hinweis` an einen eigenen Prozess schicken kann.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Läuft noch (Drehkreis, kein Timer).
    Running,
    Success,
    /// Teilweise geklappt oder unerwartetes Ergebnis.
    Warning,
    Error,
    Info,
}

/// Farbe eines Badges, z. B. „−75 %“.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tone {
    Good,
    Warn,
    Bad,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Icon {
    Clipboard,
    Info,
}

/// Bausteine, aus denen sich der Inhalt zusammensetzt. Reihenfolge = Anzeigereihenfolge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Block {
    /// Fortschrittsbalken mit Prozentangabe und kurzer Beschriftung darüber.
    Progress { fraction: f32, label: String },
    /// Vorher/Nachher, z. B. „1,7 MB → 420 KB“. Das Badge (z. B. „−75 %“) steht in der
    /// Titelzeile; `badge_hint` erscheint beim Darüberfahren (z. B. warum es größer wurde).
    Stat {
        before: String,
        after: String,
        badge: Option<(String, Tone)>,
        #[serde(default)]
        badge_hint: Option<String>,
    },
    Text(String),
    /// Längerer Text in Anführungszeichen, z. B. erkannter Text (höchstens drei Zeilen).
    Quote(String),
    /// Erklärung mit Info-Symbol, z. B. warum eine Datei größer wurde.
    Hint(String),
    /// Kleine Zeile mit Symbol, z. B. „In der Zwischenablage · Strg+V“.
    Chip { icon: Icon, text: String },
    /// Aufklappbare Liste, z. B. Fehler pro Datei.
    Details { label: String, items: Vec<String> },
}

/// Der eine Knopf eines Hinweises.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Datei im Explorer markieren.
    RevealFile { label: String, path: PathBuf },
    /// Datei oder Ordner mit dem Standardprogramm öffnen.
    Open { label: String, path: PathBuf },
}

impl Action {
    pub fn label(&self) -> &str {
        match self {
            Action::RevealFile { label, .. } | Action::Open { label, .. } => label,
        }
    }

    pub fn run(&self) {
        use std::os::windows::process::CommandExt;
        let _ = match self {
            // raw_arg, weil Explorer das Anführungszeichen erst nach dem Komma erwartet.
            Action::RevealFile { path, .. } => std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", path.display()))
                .spawn(),
            Action::Open { path, .. } => std::process::Command::new("explorer").arg(path).spawn(),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    pub kind: Kind,
    pub title: String,
    pub subtitle: Option<String>,
    pub blocks: Vec<Block>,
    pub action: Option<Action>,
    /// Schließt sich nach so vielen Sekunden (Maus darauf hält den Timer an). `None` = bleibt offen.
    pub timeout_secs: Option<f32>,
}

impl Notice {
    pub fn new(kind: Kind, title: impl Into<String>) -> Self {
        let timeout_secs = match kind {
            Kind::Running => None,
            Kind::Success => Some(5.0),
            Kind::Info | Kind::Warning => Some(9.0),
            Kind::Error => Some(15.0),
        };
        Self {
            kind,
            title: title.into(),
            subtitle: None,
            blocks: Vec::new(),
            action: None,
            timeout_secs,
        }
    }

    pub fn subtitle(mut self, text: impl Into<String>) -> Self {
        self.subtitle = Some(text.into());
        self
    }

    pub fn block(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_secs.map(Duration::from_secs_f32)
    }

    /// Reiner Text für die Windows-Benachrichtigung bzw. ein Meldungsfenster.
    pub fn plain_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.subtitle.iter().cloned().collect();
        for block in &self.blocks {
            match block {
                Block::Progress { label, .. } => lines.push(label.clone()),
                Block::Stat { before, after, badge, .. } => lines.push(match badge {
                    Some((b, _)) => format!("{before} → {after} ({b})"),
                    None => format!("{before} → {after}"),
                }),
                Block::Text(t) | Block::Hint(t) => lines.push(t.clone()),
                Block::Quote(t) => lines.push(format!("„{}“", t.lines().next().unwrap_or(""))),
                Block::Chip { text, .. } => lines.push(text.clone()),
                Block::Details { items, .. } => lines.extend(items.iter().take(2).cloned()),
            }
        }
        lines
    }

    /// Aktueller Fortschritt, falls ein Fortschrittsbalken enthalten ist.
    pub fn progress(&self) -> Option<(f32, &str)> {
        self.blocks.iter().find_map(|b| match b {
            Block::Progress { fraction, label } => Some((*fraction, label.as_str())),
            _ => None,
        })
    }
}
