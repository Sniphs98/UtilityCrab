// Im Release-Build kein Konsolenfenster öffnen – das Programm lebt nur im Tray.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod download;
mod icon;
mod ocr;
mod settings;
mod settings_ui;
mod tts;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, SetTimer, TranslateMessage, MB_ICONHAND,
    MB_OK, MSG, WM_QUIT, WM_TIMER,
};

use settings::{hotkey_label, Settings};

/// Startargument für den Einstellungsdialog (läuft als eigener Prozess).
const SETTINGS_ARG: &str = "--einstellungen";

/// Aufgaben, die der Worker-Thread abarbeitet.
#[derive(Debug, Clone, Copy)]
enum Command {
    /// Markierten Text vorlesen (bzw. stoppen, falls gerade vorgelesen wird).
    SpeakSelection,
    /// Bildschirmausschnitt aufnehmen und den erkannten Text kopieren.
    OcrSnip,
    /// Bild aus der Zwischenablage erkennen und den Text kopieren.
    OcrClipboard,
    OpenSettings,
    /// Die Einstellungen wurden im Dialog geändert.
    SettingsChanged,
    /// Fehlerprotokoll im Editor öffnen.
    OpenLog,
    Quit,
}

/// Verhindert, dass mehrere Texterkennungen gleichzeitig laufen.
static OCR_BUSY: AtomicBool = AtomicBool::new(false);

/// IDs der registrierten Tastenkürzel.
static SPEAK_HOTKEY: AtomicU32 = AtomicU32::new(0);
static OCR_HOTKEY: AtomicU32 = AtomicU32::new(0);

fn main() {
    if std::env::args().any(|a| a == SETTINGS_ARG) {
        settings_ui::run();
        return;
    }

    let main_thread = unsafe { GetCurrentThreadId() };
    let (tx, rx) = mpsc::channel::<Command>();

    let worker = thread::spawn(move || worker_loop(rx, main_thread));

    // --- Globale Shortcuts -------------------------------------------------
    let mut settings = Settings::load();
    let hotkeys = GlobalHotKeyManager::new().expect("Hotkey-Manager konnte nicht starten");
    let mut registered = Vec::new();
    register_hotkeys(&hotkeys, &settings, &mut registered);

    let hotkey_tx = tx.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }
        let cmd = if event.id == SPEAK_HOTKEY.load(Ordering::SeqCst) {
            Command::SpeakSelection
        } else if event.id == OCR_HOTKEY.load(Ordering::SeqCst) {
            Command::OcrSnip
        } else {
            return;
        };
        let _ = hotkey_tx.send(cmd);
    }));

    // --- Tray-Icon mit Menü -----------------------------------------------
    let speak_item = MenuItem::new("", true, None);
    let snip_item = MenuItem::new("", true, None);
    let clip_item = MenuItem::new("Text aus Bild in Zwischenablage kopieren", true, None);
    let settings_item = MenuItem::new("Einstellungen …", true, None);
    let log_item = MenuItem::new("Fehlerprotokoll öffnen", true, None);
    let quit_item = MenuItem::new("Beenden", true, None);

    let menu = Menu::new();
    menu.append_items(&[
        &speak_item,
        &snip_item,
        &clip_item,
        &PredefinedMenuItem::separator(),
        &settings_item,
        &log_item,
        &quit_item,
    ])
    .expect("Menü konnte nicht erstellt werden");

    let menu_ids = [
        (speak_item.id().clone(), Command::SpeakSelection),
        (snip_item.id().clone(), Command::OcrSnip),
        (clip_item.id().clone(), Command::OcrClipboard),
        (settings_item.id().clone(), Command::OpenSettings),
        (log_item.id().clone(), Command::OpenLog),
        (quit_item.id().clone(), Command::Quit),
    ];
    let menu_tx = tx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, cmd)) = menu_ids.iter().find(|(id, _)| *id == event.id) {
            let _ = menu_tx.send(*cmd);
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon::crab_icon())
        .build()
        .expect("Tray-Icon konnte nicht erstellt werden");
    update_labels(&settings, &tray, &speak_item, &snip_item);

    // Einmal pro Sekunde nachsehen, ob der Dialog die Einstellungen geändert hat.
    let mut settings_modified = settings::modified();
    unsafe {
        SetTimer(None, 0, 1000, None);
    }

    // --- Win32-Nachrichtenschleife (nötig für Tray und Hotkeys) -------------
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                let modified = settings::modified();
                if modified != settings_modified {
                    settings_modified = modified;
                    let new_settings = Settings::load();
                    if new_settings.hotkey_speak != settings.hotkey_speak
                        || new_settings.hotkey_ocr != settings.hotkey_ocr
                    {
                        register_hotkeys(&hotkeys, &new_settings, &mut registered);
                        update_labels(&new_settings, &tray, &speak_item, &snip_item);
                    }
                    settings = new_settings;
                    let _ = tx.send(Command::SettingsChanged);
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let _ = tx.send(Command::Quit);
    let _ = worker.join();
}

/// Registriert die Tastenkürzel aus den Einstellungen neu.
fn register_hotkeys(manager: &GlobalHotKeyManager, settings: &Settings, registered: &mut Vec<HotKey>) {
    let _ = manager.unregister_all(registered);
    registered.clear();
    for (text, slot) in [
        (&settings.hotkey_speak, &SPEAK_HOTKEY),
        (&settings.hotkey_ocr, &OCR_HOTKEY),
    ] {
        slot.store(0, Ordering::SeqCst);
        let key = match text.parse::<HotKey>() {
            Ok(key) => key,
            Err(e) => {
                log_error(&format!("Ungültiges Tastenkürzel „{text}“: {e}"));
                continue;
            }
        };
        match manager.register(key) {
            Ok(()) => {
                slot.store(key.id(), Ordering::SeqCst);
                registered.push(key);
            }
            Err(e) => log_error(&format!(
                "Tastenkürzel {} konnte nicht registriert werden (schon von einem anderen Programm belegt?): {e}",
                hotkey_label(text)
            )),
        }
    }
}

fn update_labels(settings: &Settings, tray: &TrayIcon, speak_item: &MenuItem, snip_item: &MenuItem) {
    let speak = hotkey_label(&settings.hotkey_speak);
    let ocr = hotkey_label(&settings.hotkey_ocr);
    speak_item.set_text(format!("Markierten Text vorlesen\t{speak}"));
    snip_item.set_text(format!("Text aus Bildausschnitt kopieren\t{ocr}"));
    let _ = tray.set_tooltip(Some(format!("UtilityCrab – {speak} vorlesen, {ocr} Text aus Bild")));
}

fn worker_loop(rx: mpsc::Receiver<Command>, main_thread: u32) {
    let mut speaker = tts::Speaker::new();
    let mut settings_window: Option<Child> = None;
    tts::preload(&Settings::load());

    for cmd in rx {
        let result = match cmd {
            Command::SpeakSelection => speak_selection(&mut speaker),
            Command::OcrSnip => spawn_ocr(ocr::snip_to_clipboard),
            Command::OcrClipboard => spawn_ocr(ocr::clipboard_image_to_clipboard),
            Command::OpenSettings => open_settings(&mut settings_window),
            Command::SettingsChanged => {
                tts::preload(&Settings::load());
                Ok(())
            }
            Command::OpenLog => open_log(),
            Command::Quit => {
                speaker.stop();
                unsafe {
                    let _ = PostThreadMessageW(main_thread, WM_QUIT, Default::default(), Default::default());
                }
                break;
            }
        };
        report(result);
    }
}

fn speak_selection(speaker: &mut tts::Speaker) -> Result<(), String> {
    if speaker.is_speaking() {
        speaker.stop();
        return Ok(());
    }
    let text = clipboard::copy_selection()?;
    speaker.speak(&text, &Settings::load())
}

/// Startet den Einstellungsdialog als eigenen Prozess (höchstens einmal gleichzeitig).
fn open_settings(window: &mut Option<Child>) -> Result<(), String> {
    if let Some(child) = window {
        if matches!(child.try_wait(), Ok(None)) {
            return Ok(());
        }
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let child = std::process::Command::new(exe)
        .arg(SETTINGS_ARG)
        .spawn()
        .map_err(|e| format!("Einstellungen konnten nicht geöffnet werden: {e}"))?;
    *window = Some(child);
    Ok(())
}

/// Startet eine Texterkennung in eigenem Thread, weil dabei auf den Benutzer gewartet wird.
fn spawn_ocr(job: fn() -> Result<(), String>) -> Result<(), String> {
    if OCR_BUSY.swap(true, Ordering::SeqCst) {
        return Err("Texterkennung läuft bereits".into());
    }
    thread::spawn(move || {
        let result = job();
        OCR_BUSY.store(false, Ordering::SeqCst);
        report(result);
    });
    Ok(())
}

fn report(result: Result<(), String>) {
    match result {
        Ok(()) => {}
        Err(e) => {
            log_error(&e);
            unsafe {
                let _ = MessageBeep(MB_ICONHAND);
            }
        }
    }
}

/// Ordner für Protokoll, Einstellungen und heruntergeladene Modelle.
fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("UtilityCrab")
}

fn log_path() -> PathBuf {
    data_dir().join("fehler.log")
}

/// Hängt einen Fehler mit Zeitstempel ans Protokoll an – im Release-Build gibt es keine Konsole.
fn log_error(msg: &str) {
    eprintln!("Fehler: {msg}");
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let t = unsafe { GetLocalTime() };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {msg}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        );
    }
}

fn open_log() -> Result<(), String> {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Protokoll konnte nicht angelegt werden: {e}"))?;
    std::process::Command::new("notepad")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Editor konnte nicht gestartet werden: {e}"))?;
    Ok(())
}

/// Kurzer Bestätigungston.
pub fn beep_ok() {
    unsafe {
        let _ = MessageBeep(MB_OK);
    }
}
