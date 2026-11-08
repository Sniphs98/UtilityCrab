//! Untermenü „Konvertieren“ im Explorer-Kontextmenü von Bilddateien.
//!
//! Klassische Registry-Verben unter HKCU (keine Admin-Rechte nötig). Unter Windows 11
//! erscheinen sie unter „Weitere Optionen anzeigen“ bzw. mit Umschalt+Rechtsklick.

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::convert::batch::Job;
use crate::convert::{Format, CLI_ARG, INPUT_EXTENSIONS};

/// Startargumente für den Installer.
pub const INSTALL_ARG: &str = "--kontextmenue-einrichten";
pub const UNINSTALL_ARG: &str = "--kontextmenue-entfernen";

const VERB: &str = "UtilityCrab";
/// Trennlinie über dem Eintrag (ECF_SEPARATORBEFORE).
const SEPARATOR_BEFORE: u32 = 0x20;

/// Formate, die direkt im Untermenü stehen; alle anderen gibt es im Fenster.
const QUICK_FORMATS: [Format; 4] = [Format::Png, Format::Jpg, Format::Webp, Format::Ico];
/// Dateien, für die „Verkleinern“ angeboten wird.
const SHRINKABLE: [&str; 8] = ["png", "jpg", "jpeg", "jfif", "webp", "bmp", "tif", "tiff"];

fn key_path(ext: &str) -> String {
    format!(r"Software\Classes\SystemFileAssociations\.{ext}\shell\{VERB}")
}

/// Trägt das Menü ein bzw. entfernt es, passend zur Einstellung.
pub fn sync(enabled: bool) -> Result<(), String> {
    if enabled {
        install()
    } else {
        uninstall()
    }
}

pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.display();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let fail = |e: std::io::Error| format!("Kontextmenü eintragen: {e}");

    for ext in INPUT_EXTENSIONS {
        let path = key_path(ext);
        // Alte Einträge (z. B. mit anderem Programmpfad) vollständig ersetzen.
        let _ = hkcu.delete_subkey_all(&path);
        let (root, _) = hkcu.create_subkey(&path).map_err(fail)?;
        root.set_value("MUIVerb", &"Konvertieren").map_err(fail)?;
        root.set_value("SubCommands", &"").map_err(fail)?;
        // Das in die .exe eingebettete Krabben-Icon (siehe build.rs).
        root.set_value("Icon", &format!("\"{exe}\",0")).map_err(fail)?;

        let source = Format::from_id(ext);
        let mut entries: Vec<(Job, String)> = QUICK_FORMATS
            .iter()
            .filter(|f| Some(**f) != source)
            .map(|f| (Job::ConvertTo(*f), format!("In {} umwandeln", f.label())))
            .collect();
        if SHRINKABLE.contains(ext) {
            entries.push((Job::Shrink, "Verkleinern".into()));
        }
        entries.push((Job::Dialog, "Weitere Optionen …".into()));

        // Explorer sortiert die Unterschlüssel alphabetisch, daher die Nummern.
        for (i, (job, label)) in entries.iter().enumerate() {
            let (verb, _) = root.create_subkey(format!(r"shell\{i:02}-{}", job.id())).map_err(fail)?;
            verb.set_value("MUIVerb", label).map_err(fail)?;
            if matches!(job, Job::Shrink) || matches!(job, Job::Dialog) && !SHRINKABLE.contains(ext) {
                verb.set_value("CommandFlags", &SEPARATOR_BEFORE).map_err(fail)?;
            }
            let (command, _) = verb.create_subkey("command").map_err(fail)?;
            command
                .set_value("", &format!("\"{exe}\" {CLI_ARG} {} \"%1\"", job.id()))
                .map_err(fail)?;
        }
    }
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in INPUT_EXTENSIONS {
        match hkcu.delete_subkey_all(key_path(ext)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Kontextmenü entfernen: {e}")),
        }
    }
    Ok(())
}
