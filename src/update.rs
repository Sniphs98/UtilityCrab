//! Update-Check über GitHub-Releases und Installation des neuen Setups.
//!
//! Die Release-Version (z. B. „0.1.0.7“) bekommt die .exe beim Bauen über die Umgebungsvariable
//! `UTILITYCRAB_VERSION` (siehe build.rs und den Release-Workflow). Lokale Builds haben keine
//! und suchen darum nicht automatisch nach Updates.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::download::{download_all, FileSpec};
use crate::popup::{self, Action, Block, Kind, Notice};

const REPO: &str = "Sniphs98/UtilityCrab";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Startargument: `--update-installieren <url> <version>`.
pub const INSTALL_ARG: &str = "--update-installieren";
/// Startargument: sofort suchen und das Ergebnis als Popup zeigen.
pub const CHECK_ARG: &str = "--update-suchen";

/// Einstieg für `--update-suchen`.
pub fn check_now() {
    let searching = Notice::new(Kind::Running, "Suche nach Updates …");
    let result = popup::run(searching, |handle| handle.set(check_notice(&check())));
    if let Err((e, _)) = result {
        crate::log_error(&e);
    }
}

/// Version dieser .exe aus dem Release-Build, `None` bei einem lokalen Build.
pub fn release_version() -> Option<&'static str> {
    Some(env!("UTILITYCRAB_VERSION")).filter(|v| !v.is_empty())
}

/// Version für die Anzeige, z. B. „0.1.0.7“ oder „0.1.0 (lokaler Build)“.
pub fn display_version() -> String {
    match release_version() {
        Some(v) => v.to_string(),
        None => format!("{} (lokaler Build)", env!("CARGO_PKG_VERSION")),
    }
}

/// Ein veröffentlichtes Release mit Setup.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub setup_url: String,
    pub setup_size: u64,
    /// Seite mit den Änderungen.
    pub page_url: String,
}

/// Fragt GitHub nach dem neuesten Release.
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let output = Command::new("curl.exe")
        .args(["-fsSL", "--max-time", "20", "-H", "Accept: application/vnd.github+json", "-A", "UtilityCrab"])
        .arg(&url)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("curl konnte nicht gestartet werden: {e}"))?;
    if !output.status.success() {
        return Err("GitHub ist gerade nicht erreichbar.".into());
    }
    parse_release(&String::from_utf8_lossy(&output.stdout))
}

fn parse_release(json: &str) -> Result<Release, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Antwort von GitHub: {e}"))?;
    let tag = value["tag_name"].as_str().ok_or("Release ohne Versionsnummer")?;
    let asset = value["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|a| {
                a["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("UtilityCrab-Setup") && n.ends_with(".exe"))
            })
        })
        .ok_or("Im neuesten Release fehlt das Setup")?;
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        setup_url: asset["browser_download_url"].as_str().unwrap_or_default().to_string(),
        setup_size: asset["size"].as_u64().unwrap_or(0),
        page_url: value["html_url"].as_str().unwrap_or_default().to_string(),
    })
}

/// „0.1.0.7“ → [0, 1, 0, 7]; fehlende Stellen zählen als 0.
fn parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// Ist `remote` neuer als `local`?
pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut a, mut b) = (parts(remote), parts(local));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// Version, mit der verglichen wird (lokale Builds: die aus Cargo.toml).
fn comparable_version() -> &'static str {
    release_version().unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// `Some(release)`, wenn es eine neuere Version gibt.
pub fn check() -> Result<Option<Release>, String> {
    let release = latest()?;
    Ok(is_newer(&release.version, comparable_version()).then_some(release))
}

/// Hinweis „Update verfügbar“ mit Knopf zum Installieren.
pub fn available_notice(release: &Release) -> Notice {
    let mut notice = Notice::new(Kind::Info, "Update verfügbar")
        .subtitle(format!("Version {} · installiert: {}", release.version, display_version()))
        .action(Action::Run {
            label: "Jetzt aktualisieren".into(),
            args: vec![INSTALL_ARG.into(), release.setup_url.clone(), release.version.clone()],
        });
    notice.timeout_secs = Some(30.0);
    notice
}

/// Hinweis nach einer Suche von Hand.
pub fn check_notice(result: &Result<Option<Release>, String>) -> Notice {
    match result {
        Ok(Some(release)) => available_notice(release),
        Ok(None) => Notice::new(Kind::Success, "UtilityCrab ist aktuell").subtitle(format!("Version {}", display_version())),
        Err(e) => Notice::new(Kind::Error, "Update-Suche fehlgeschlagen").block(Block::Text(e.clone())),
    }
}

/// Hintergrund-Prüfung der Tray-App: kurz nach dem Start, danach täglich.
/// Jede neue Version wird nur einmal pro Sitzung gemeldet.
pub fn spawn_background_checks(enabled: impl Fn() -> bool + Send + 'static) {
    if release_version().is_none() {
        return;
    }
    std::thread::spawn(move || {
        let mut announced = String::new();
        std::thread::sleep(Duration::from_secs(30));
        loop {
            if enabled() {
                match check() {
                    Ok(Some(release)) if release.version != announced => {
                        announced = release.version.clone();
                        popup::remote::notify(available_notice(&release));
                    }
                    Ok(_) => {}
                    Err(e) => crate::log_error(&format!("Update-Suche: {e}")),
                }
            }
            std::thread::sleep(Duration::from_secs(24 * 60 * 60));
        }
    });
}

/// Einstieg für `--update-installieren <url> <version>`: Setup laden (mit Fortschritt im Popup)
/// und still installieren. Das Setup beendet die laufende App und startet sie danach neu.
pub fn install(args: &[String]) {
    let (Some(url), Some(version)) = (args.first(), args.get(1)) else {
        return;
    };
    // Nur Setups aus den eigenen Releases ausführen.
    let allowed = format!("https://github.com/{REPO}/releases/download/");
    if !url.starts_with(&allowed) {
        crate::log_error(&format!("Update abgelehnt, unbekannte Quelle: {url}"));
        return;
    }
    let dir = crate::data_dir().join("updates");
    let target = dir.join(format!("UtilityCrab-Setup-{version}.exe"));
    // Setups früherer Updates aufräumen.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path() != target {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let size = latest()
        .ok()
        .filter(|r| r.setup_url == *url)
        .map(|r| r.setup_size)
        .unwrap_or(40_000_000);

    let downloading = |fraction: f32| {
        Notice::new(Kind::Running, "Update wird geladen").block(Block::Progress {
            fraction,
            label: format!("UtilityCrab {version}"),
        })
    };
    let result = popup::run(downloading(0.0), |handle| {
        let spec = FileSpec {
            url: url.clone(),
            target: target.clone(),
            size,
        };
        let loaded = download_all(&[spec], &mut |p| handle.set(downloading(p)));
        match loaded.and_then(|()| run_setup(&target)) {
            Ok(()) => {
                let mut done = Notice::new(Kind::Success, "Update wird installiert")
                    .subtitle(format!("UtilityCrab {version} startet gleich neu."));
                done.timeout_secs = Some(4.0);
                handle.set(done);
            }
            Err(e) => {
                crate::log_error(&format!("Update: {e}"));
                handle.set(Notice::new(Kind::Error, "Update fehlgeschlagen").block(Block::Text(e)));
            }
        }
    });
    if let Err((e, _)) = result {
        crate::log_error(&e);
    }
}

/// Startet das Setup ohne Rückfragen. Es beendet UtilityCrab (auch diesen Prozess) selbst.
fn run_setup(setup: &PathBuf) -> Result<(), String> {
    Command::new(setup)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Setup konnte nicht gestartet werden: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.0.8", "0.1.0.7"));
        assert!(is_newer("v0.2.0.1", "0.1.0.99"));
        assert!(is_newer("0.1.0.1", "0.1.0"));
        assert!(!is_newer("0.1.0.7", "0.1.0.7"));
        assert!(!is_newer("0.1.0.6", "0.1.0.10"));
    }

    #[test]
    fn parses_github_release() {
        let json = r#"{
            "tag_name": "v0.1.0.9",
            "html_url": "https://github.com/Sniphs98/UtilityCrab/releases/tag/v0.1.0.9",
            "assets": [
                {"name": "notes.txt", "browser_download_url": "x", "size": 1},
                {"name": "UtilityCrab-Setup-0.1.0.9.exe",
                 "browser_download_url": "https://github.com/Sniphs98/UtilityCrab/releases/download/v0.1.0.9/UtilityCrab-Setup-0.1.0.9.exe",
                 "size": 12345}
            ]
        }"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, "0.1.0.9");
        assert_eq!(release.setup_size, 12345);
        assert!(release.setup_url.ends_with("UtilityCrab-Setup-0.1.0.9.exe"));
    }
}
