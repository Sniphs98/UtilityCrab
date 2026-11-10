//! Einstellungsdialog (egui) im Stil der macOS-Systemeinstellungen: Seitenleiste links,
//! rechts Seiten aus gruppierten Zeilen. Läuft als eigener Prozess, damit das Tray-Programm
//! mit seinen Tastenkürzeln davon unberührt bleibt. Änderungen werden sofort gespeichert.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, vec2, Color32, RichText};
use global_hotkey::hotkey::HotKey;

use crate::icon;
use crate::settings::{hotkey_label, Notify, Pronunciation, Settings};
use crate::tts::{self, Package, VoiceInfo};
use crate::update;
use crate::ui_kit::{
    self, danger_button, keycaps, nav_item, page_header, primary_button, regular, section, semibold, toggle, NavIcon,
    SIDEBAR_BG, SMALL_SIZE, TEXT, TEXT_FAINT, TEXT_WEAK,
};

// Für die anderen Fenster (Konverter, Popup), die das gemeinsame Aussehen nutzen.
pub use ui_kit::ACCENT;
pub const PAGE_BG: Color32 = ui_kit::CONTENT_BG;
pub const CARD_BG: Color32 = ui_kit::GROUP_BG;

/// Gemeinsames Aussehen aller Fenster (Einstellungen, Konverter, Popup).
pub fn apply_theme(ctx: &egui::Context) {
    ui_kit::apply(ctx);
}

pub fn accent_button(text: &str) -> egui::Button<'_> {
    primary_button(text)
}

pub fn window_icon() -> egui::IconData {
    egui::IconData {
        rgba: icon::crab_rgba(),
        width: icon::SIZE,
        height: icon::SIZE,
    }
}

/// Öffnet das Fenster; `tab` (z. B. „modelle“) wählt die Startseite.
pub fn run(tab: Option<&str>) {
    let start = Tab::ALL
        .into_iter()
        .find(|t| Some(t.id()) == tab)
        .unwrap_or(Tab::Voice);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Einstellungen")
            .with_inner_size([820.0, 580.0])
            .with_min_inner_size([680.0, 460.0])
            .with_icon(window_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab",
        options,
        Box::new(move |cc| Ok(Box::new(SettingsApp::new(cc, start)))),
    );
    if let Err(e) = result {
        crate::report(Err(format!("Einstellungsfenster: {e}")));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Voice,
    Models,
    Pronunciation,
    Hotkeys,
    Screenshot,
    Convert,
    About,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Voice,
        Tab::Models,
        Tab::Pronunciation,
        Tab::Hotkeys,
        Tab::Screenshot,
        Tab::Convert,
        Tab::About,
    ];

    fn id(self) -> &'static str {
        match self {
            Tab::Voice => "stimme",
            Tab::Models => "modelle",
            Tab::Pronunciation => "aussprache",
            Tab::Hotkeys => "tasten",
            Tab::Screenshot => "screenshot",
            Tab::Convert => "konvertieren",
            Tab::About => "info",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Voice => "Stimme",
            Tab::Models => "Modelle",
            Tab::Pronunciation => "Aussprache",
            Tab::Hotkeys => "Tastenkürzel",
            Tab::Screenshot => "Screenshot",
            Tab::Convert => "Konvertieren",
            Tab::About => "Info & Updates",
        }
    }

    fn icon(self) -> (NavIcon, Color32) {
        match self {
            Tab::Voice => (NavIcon::Speaker, Color32::from_rgb(0x3B, 0x82, 0xF6)),
            Tab::Models => (NavIcon::Download, Color32::from_rgb(0x8B, 0x5C, 0xF6)),
            Tab::Pronunciation => (NavIcon::Text, Color32::from_rgb(0x22, 0xA5, 0x5B)),
            Tab::Hotkeys => (NavIcon::Keyboard, Color32::from_rgb(0x6B, 0x72, 0x80)),
            Tab::Screenshot => (NavIcon::Camera, Color32::from_rgb(0xD9, 0x46, 0x6F)),
            Tab::Convert => (NavIcon::Convert, ui_kit::ACCENT),
            Tab::About => (NavIcon::Info, Color32::from_rgb(0x0E, 0x9F, 0xB5)),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum HotkeyTarget {
    Speak,
    Ocr,
    Snip,
}

enum DownloadState {
    Running(f32),
    Done,
    Failed(String),
}

/// Ergebnis der Update-Suche auf der Info-Seite.
#[derive(Clone)]
enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(update::Release),
    Failed(String),
}

/// Laufende Downloads, Schlüssel "provider/paket".
type Downloads = Arc<Mutex<HashMap<String, DownloadState>>>;

struct SettingsApp {
    tab: Tab,
    settings: Settings,
    saved: Settings,
    speaker: tts::Speaker,
    sample_text: String,
    downloads: Downloads,
    /// Pakete und Stimmen je Provider; neu eingelesen nach Download oder Löschen.
    catalog: Vec<(&'static str, Vec<Package>, Vec<VoiceInfo>)>,
    confirm_delete: Option<String>,
    capture: Option<HotkeyTarget>,
    message: Option<String>,
    crab: egui::TextureHandle,
    update: Arc<Mutex<UpdateState>>,
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext, tab: Tab) -> Self {
        apply_theme(&cc.egui_ctx);
        let crab = cc.egui_ctx.load_texture(
            "krabbe",
            egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::crab_pixels::crab_rgba(64)),
            egui::TextureOptions::LINEAR,
        );
        let settings = Settings::load();
        let mut app = Self {
            tab,
            saved: settings.clone(),
            settings,
            speaker: tts::Speaker::new(),
            sample_text: "Hallo! So klingt diese Stimme. Ist das privat oder öffentlich?".into(),
            downloads: Downloads::default(),
            catalog: Vec::new(),
            confirm_delete: None,
            capture: None,
            message: None,
            crab,
            update: Arc::new(Mutex::new(UpdateState::Idle)),
        };
        app.refresh_catalog();
        app
    }

    fn refresh_catalog(&mut self) {
        self.catalog = tts::providers()
            .iter()
            .map(|p| (p.id(), p.packages(), p.voices()))
            .collect();
    }

    fn voices_of(&self, provider: &str) -> &[VoiceInfo] {
        self.catalog
            .iter()
            .find(|(id, ..)| *id == provider)
            .map(|(_, _, voices)| voices.as_slice())
            .unwrap_or_default()
    }

    fn speak(&mut self, text: &str) {
        self.message = self.speaker.speak(text, &self.settings).err();
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Abgeschlossene Downloads übernehmen.
        let finished: Vec<(String, Option<String>)> = {
            let downloads = self.downloads.lock().unwrap();
            downloads
                .iter()
                .filter_map(|(key, state)| match state {
                    DownloadState::Done => Some((key.clone(), None)),
                    DownloadState::Failed(e) => Some((key.clone(), Some(e.clone()))),
                    DownloadState::Running(_) => None,
                })
                .collect()
        };
        if !finished.is_empty() {
            let mut downloads = self.downloads.lock().unwrap();
            for (key, error) in finished {
                downloads.remove(&key);
                if let Some(e) = error {
                    self.message = Some(e);
                }
            }
            drop(downloads);
            self.refresh_catalog();
        }

        egui::Panel::left("navigation")
            .exact_size(210.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(SIDEBAR_BG).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ui, |ui| self.navigation(ui));

        let page = egui::Frame::new()
            .fill(ui_kit::CONTENT_BG)
            .inner_margin(egui::Margin { left: 28, right: 22, top: 22, bottom: 12 });
        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            // Breite vorher festlegen: in der ScrollArea wäre sie sonst nicht begrenzt.
            let width = ui.available_width().min(620.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(width);
                if let Some(message) = self.message.clone() {
                    ui_kit::error_banner(ui, &message);
                    ui.add_space(14.0);
                }
                match self.tab {
                    Tab::Voice => self.voice_tab(ui),
                    Tab::Models => self.models_tab(ui),
                    Tab::Pronunciation => self.pronunciation_tab(ui),
                    Tab::Hotkeys => self.hotkeys_tab(ui),
                    Tab::Screenshot => self.screenshot_tab(ui),
                    Tab::Convert => self.convert_tab(ui),
                    Tab::About => self.about_tab(ui),
                }
            });
        });

        if self.settings != self.saved {
            match self.settings.save() {
                Ok(()) => self.saved = self.settings.clone(),
                Err(e) => self.message = Some(e),
            }
        }

        // Fortschritt und Abspielknopf aktuell halten.
        let busy = !self.downloads.lock().unwrap().is_empty()
            || self.speaker.is_speaking()
            || matches!(*self.update.lock().unwrap(), UpdateState::Checking);
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }
}

impl SettingsApp {
    fn navigation(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(egui::Image::new(&self.crab).fit_to_exact_size(vec2(30.0, 30.0)));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("UtilityCrab").font(semibold(15.0)).color(TEXT));
                ui.label(RichText::new("Einstellungen").font(regular(SMALL_SIZE)).color(TEXT_WEAK));
            });
        });
        ui.add_space(18.0);

        ui.spacing_mut().item_spacing.y = 2.0;
        for tab in Tab::ALL {
            let (icon, color) = tab.icon();
            if nav_item(ui, self.tab == tab, icon, color, tab.label()) {
                self.tab = tab;
                self.capture = None;
                self.message = None;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(RichText::new("Änderungen werden sofort gespeichert.").font(regular(11.5)).color(TEXT_FAINT))
                        .wrap(),
                );
            });
        });
    }

    fn voice_tab(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Stimme", "Mit welcher Stimme markierter Text vorgelesen wird.");

        let mut go_to_models = false;
        let voices = self.voices_of(&self.settings.provider).to_vec();
        let providers: Vec<(&'static str, &'static str, bool)> = tts::providers()
            .iter()
            .map(|p| (p.id(), p.name(), !self.voices_of(p.id()).is_empty()))
            .collect();
        section(ui, None, None, |g| {
            g.row("Engine", None, |ui| {
                let current = providers
                    .iter()
                    .find(|(id, ..)| *id == self.settings.provider)
                    .map(|(_, name, _)| *name)
                    .unwrap_or("–");
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(current)
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for (id, name, installed) in &providers {
                            let label = if *installed {
                                name.to_string()
                            } else {
                                format!("{name} (nicht installiert)")
                            };
                            if ui.selectable_label(self.settings.provider == *id, label).clicked() {
                                self.settings.provider = id.to_string();
                                self.settings.voice = String::new();
                            }
                        }
                    });
            });
            g.row("Stimme", None, |ui| {
                if voices.is_empty() {
                    if ui.button("Modelle laden …").clicked() {
                        go_to_models = true;
                    }
                    ui.label(RichText::new("Noch keine installiert").color(TEXT_WEAK));
                } else {
                    let current = voices
                        .iter()
                        .find(|v| v.id == self.settings.voice)
                        .unwrap_or(&voices[0]);
                    let mut selected = current.id.clone();
                    egui::ComboBox::from_id_salt("voice")
                        .selected_text(&current.name)
                        .width(230.0)
                        .show_ui(ui, |ui| {
                            for voice in &voices {
                                ui.selectable_value(&mut selected, voice.id.clone(), &voice.name);
                            }
                        });
                    if selected != self.settings.voice {
                        self.settings.voice = selected;
                    }
                }
            });
            g.row("Tempo", None, |ui| {
                ui.spacing_mut().slider_width = 180.0;
                ui.add(
                    egui::Slider::new(&mut self.settings.speed, 0.5..=2.0)
                        .step_by(0.05)
                        .fixed_decimals(2)
                        .suffix("×"),
                );
            });
            g.row(
                "Sprache automatisch erkennen",
                Some("Wechselt pro Satz zu einer installierten Stimme der erkannten Sprache, z. B. für einen englischen Satz."),
                |ui| {
                    toggle(ui, &mut self.settings.auto_language);
                },
            );
        });
        if go_to_models {
            self.tab = Tab::Models;
        }

        let mut play = false;
        let speaking = self.speaker.is_speaking();
        section(ui, Some("Probehören"), None, |g| {
            g.custom(|ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.sample_text)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    let clicked = if speaking {
                        ui.button("Stopp").clicked()
                    } else {
                        ui.add(primary_button("Probe hören")).clicked()
                    };
                    play = clicked;
                });
            });
        });
        if play {
            if speaking {
                self.speaker.stop();
            } else {
                let text = self.sample_text.clone();
                self.speak(&text);
            }
        }
    }

    fn models_tab(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Modelle",
            "Stimmen-Modelle herunterladen oder löschen. Alles läuft lokal, kein Text verlässt den PC.",
        );

        let mut start_download = None;
        let mut delete = None;
        let mut confirm = self.confirm_delete.clone();
        {
            let downloads = self.downloads.lock().unwrap();
            for provider in tts::providers() {
                let (packages, voices) = self
                    .catalog
                    .iter()
                    .find(|(id, ..)| *id == provider.id())
                    .map(|(_, p, v)| (p, v))
                    .expect("Katalog unvollständig");

                section(ui, Some(provider.name()), Some(provider.description()), |g| {
                    if packages.is_empty() {
                        g.row(
                            "Keine Installation nötig",
                            Some(&format!("{} Stimmen verfügbar", voices.len())),
                            |ui| {
                                ui.label(RichText::new("Bereit").color(ui_kit::GREEN));
                            },
                        );
                        return;
                    }
                    for package in packages {
                        let key = format!("{}/{}", provider.id(), package.id);
                        let size = format!("{} MB", package.size_mb);
                        g.row(&package.name, Some(&size), |ui| match downloads.get(&key) {
                            Some(DownloadState::Running(progress)) => {
                                ui.add(egui::ProgressBar::new(*progress).desired_width(150.0).show_percentage());
                            }
                            _ if package.installed => {
                                if confirm.as_deref() == Some(key.as_str()) {
                                    if ui.button("Abbrechen").clicked() {
                                        confirm = None;
                                    }
                                    if ui.add(danger_button("Löschen")).clicked() {
                                        delete = Some((provider.id(), package.id.clone()));
                                    }
                                } else {
                                    if ui.button("Löschen").clicked() {
                                        confirm = Some(key.clone());
                                    }
                                    ui.label(RichText::new("Installiert").color(ui_kit::GREEN));
                                }
                            }
                            _ => {
                                if ui.add(primary_button("Laden")).clicked() {
                                    start_download = Some((provider.id(), package.id.clone()));
                                }
                            }
                        });
                    }
                });
            }
        }
        self.confirm_delete = confirm;

        if let Some((provider_id, package)) = start_download {
            self.start_download(ui.ctx().clone(), provider_id, package);
        }
        if let Some((provider_id, package)) = delete {
            self.confirm_delete = None;
            self.speaker.stop();
            if let Some(provider) = tts::provider(provider_id) {
                if let Err(e) = provider.uninstall(&package) {
                    self.message = Some(e);
                }
            }
            self.refresh_catalog();
        }
    }

    fn start_download(&mut self, ctx: egui::Context, provider_id: &'static str, package: String) {
        let key = format!("{provider_id}/{package}");
        let downloads = self.downloads.clone();
        downloads.lock().unwrap().insert(key.clone(), DownloadState::Running(0.0));
        thread::spawn(move || {
            let Some(provider) = tts::provider(provider_id) else {
                return;
            };
            let result = provider.install(&package, &mut |progress| {
                downloads
                    .lock()
                    .unwrap()
                    .insert(key.clone(), DownloadState::Running(progress));
                ctx.request_repaint();
            });
            let state = match result {
                Ok(()) => DownloadState::Done,
                Err(e) => DownloadState::Failed(e),
            };
            downloads.lock().unwrap().insert(key, state);
            ctx.request_repaint();
        });
    }

    fn pronunciation_tab(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Aussprache",
            "Wörter, die falsch ausgesprochen werden, so aufschreiben, wie sie klingen sollen. Gilt für alle Stimmen.",
        );

        let mut remove = None;
        let mut play = None;
        let mut add = false;
        section(
            ui,
            None,
            Some("Beispiel: „privat“ als „priwaht“. Groß- und Kleinschreibung spielt keine Rolle."),
            |g| {
                for (i, entry) in self.settings.pronunciations.iter_mut().enumerate() {
                    g.custom(|ui| {
                        ui.horizontal(|ui| {
                            let field = ((ui.available_width() - 180.0) / 2.0).max(100.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.word)
                                    .hint_text("Wort")
                                    .desired_width(field),
                            );
                            let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "→",
                                regular(13.0),
                                TEXT_FAINT,
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.spoken)
                                    .hint_text("So sprechen")
                                    .desired_width(field),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Entfernen").clicked() {
                                    remove = Some(i);
                                }
                                if ui.button("Anhören").clicked() {
                                    play = Some(entry.word.clone());
                                }
                            });
                        });
                    });
                }
                if self.settings.pronunciations.is_empty() {
                    g.row("Noch keine Einträge", None, |_| {});
                }
                g.custom(|ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("+ Eintrag hinzufügen").color(ui_kit::ACCENT)).frame(false))
                        .clicked()
                    {
                        add = true;
                    }
                });
            },
        );
        if let Some(i) = remove {
            self.settings.pronunciations.remove(i);
        }
        if let Some(word) = play {
            self.speak(&word);
        }
        if add {
            self.settings.pronunciations.push(Pronunciation::default());
        }
    }

    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Tastenkürzel", "Globale Tastenkürzel, die in jedem Programm funktionieren.");

        if let Some(target) = self.capture {
            if let Some(hotkey) = capture_hotkey(ui) {
                match hotkey {
                    Ok(hotkey) => {
                        match target {
                            HotkeyTarget::Speak => self.settings.hotkey_speak = hotkey,
                            HotkeyTarget::Ocr => self.settings.hotkey_ocr = hotkey,
                            HotkeyTarget::Snip => self.settings.hotkey_snip = hotkey,
                        }
                        self.message = None;
                        self.capture = None;
                    }
                    Err(Some(e)) => self.message = Some(e),
                    Err(None) => self.capture = None,
                }
            }
        }

        let mut start_capture = None;
        let capture = self.capture;
        let rows = [
            (HotkeyTarget::Speak, "Markierten Text vorlesen", "Nochmal drücken stoppt das Vorlesen.", self.settings.hotkey_speak.clone()),
            (HotkeyTarget::Ocr, "Text aus Bildausschnitt kopieren", "Bereich auswählen, der Text landet in der Zwischenablage.", self.settings.hotkey_ocr.clone()),
            (HotkeyTarget::Snip, "Screenshot aufnehmen", "Bereich, Fenster oder Bildschirm wählen, danach bearbeiten.", self.settings.hotkey_snip.clone()),
        ];
        section(
            ui,
            None,
            Some("Kombinationen mit Strg und Alt eignen sich gut. Ist ein Kürzel schon von einem anderen Programm belegt, steht das im Fehlerprotokoll."),
            |g| {
                for (target, label, description, value) in &rows {
                    g.row(label, Some(description), |ui| {
                        if capture == Some(*target) {
                            ui.label(RichText::new("Tasten drücken … (Esc bricht ab)").color(ui_kit::ACCENT));
                        } else {
                            if ui.button("Ändern").clicked() {
                                start_capture = Some(*target);
                            }
                            keycaps(ui, &hotkey_label(value));
                        }
                    });
                }
            },
        );
        if let Some(target) = start_capture {
            self.capture = Some(target);
            self.message = None;
        }
    }

    fn screenshot_tab(&mut self, ui: &mut egui::Ui) {
        page_header(
            ui,
            "Screenshot",
            "Bildschirmausschnitte aufnehmen, beschriften, unkenntlich machen und teilen.",
        );
        let spawn = |args: &[&str]| {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).args(args).spawn();
            }
        };

        section(ui, Some("Nach der Aufnahme"), None, |g| {
            g.row(
                "Editor öffnen",
                Some("Zum Beschriften, Zuschneiden und Unkenntlichmachen. Sonst erscheint nur ein Hinweis mit „Bearbeiten“."),
                |ui| {
                    toggle(ui, &mut self.settings.snip_open_editor);
                },
            );
            g.row(
                "In die Zwischenablage kopieren",
                Some("Auch Änderungen im Editor landen automatisch dort – einfach mit Strg+V einfügen."),
                |ui| {
                    toggle(ui, &mut self.settings.snip_copy);
                },
            );
            g.row("Automatisch speichern", Some("Jeden Screenshot als PNG im Ordner unten ablegen."), |ui| {
                toggle(ui, &mut self.settings.snip_autosave);
            });
            let default_folder = crate::screenshot::default_folder();
            g.custom(|ui| {
                ui.label(RichText::new("Ordner für Screenshots").color(TEXT));
                ui.label(
                    RichText::new("Hier landen automatisch gespeicherte Screenshots; „Speichern unter“ im Editor schlägt ihn ebenfalls vor.")
                        .font(regular(SMALL_SIZE))
                        .color(TEXT_WEAK),
                );
                let custom = !self.settings.snip_folder.trim().is_empty();
                let current = if custom {
                    std::path::PathBuf::from(self.settings.snip_folder.trim())
                } else {
                    default_folder.clone()
                };
                ui.horizontal(|ui| {
                    // Platz für die Knöpfe rechts lassen.
                    let width = ui.available_width() - if custom { 260.0 } else { 175.0 };
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.snip_folder)
                            .hint_text(format!("{} (Standard)", default_folder.display()))
                            .desired_width(width.max(120.0)),
                    );
                    if ui.button("Durchsuchen …").clicked() {
                        if let Some(folder) = crate::screenshot::pick_folder(&current) {
                            // Wieder den Standard gewählt: leer lassen, dann folgt er z. B. einem neuen Benutzerprofil.
                            self.settings.snip_folder =
                                if folder == default_folder { String::new() } else { folder.display().to_string() };
                        }
                    }
                    if ui.button("Öffnen").on_hover_text("Ordner im Explorer anzeigen").clicked() {
                        let _ = std::fs::create_dir_all(&current);
                        let _ = std::process::Command::new("explorer").arg(&current).spawn();
                    }
                    if custom && ui.button("Standard").on_hover_text("Zurück zu Bilder\\Screenshots").clicked() {
                        self.settings.snip_folder.clear();
                    }
                });
            });
        });

        section(ui, Some("Explorer"), None, |g| {
            g.row(
                "„Mit UtilityCrab bearbeiten“ im Kontextmenü",
                Some("Rechtsklick auf ein Bild öffnet es im Editor. Unter Windows 11 unter „Weitere Optionen anzeigen“."),
                |ui| {
                    toggle(ui, &mut self.settings.edit_context_menu);
                },
            );
        });

        section(
            ui,
            None,
            Some(
                "Im Editor: Stift, Textmarker, Pfeile, Formen, Text, Schrittnummern, Verpixeln/Weichzeichnen/Schwärzen, \
                 Spotlight, Zuschneiden, Text erkennen, sensible Daten automatisch unkenntlich machen und Anheften.",
            ),
            |g| {
                g.row("Screenshot aufnehmen", Some("Auch über das Tastenkürzel oder das Tray-Menü."), |ui| {
                    if ui.button("In 5 Sekunden").clicked() {
                        spawn(&[crate::screenshot::CAPTURE_ARG, crate::screenshot::Prefs::load().mode.id(), "5"]);
                    }
                    if ui.button("Jetzt").clicked() {
                        spawn(&[crate::screenshot::CAPTURE_ARG, crate::screenshot::Prefs::load().mode.id(), "0"]);
                    }
                });
                g.row("Vorhandenes Bild bearbeiten", Some("Öffnet den Editor; Bilder lassen sich auch hineinziehen."), |ui| {
                    if ui.button("Editor öffnen").clicked() {
                        spawn(&[crate::screenshot::EDIT_ARG]);
                    }
                });
            },
        );
    }

    fn convert_tab(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Konvertieren", "Bilder in andere Formate umwandeln und Dateien verkleinern.");

        section(ui, Some("Explorer"), None, |g| {
            g.row(
                "Im Kontextmenü anzeigen",
                Some("Rechtsklick auf ein Bild → „Konvertieren“. Unter Windows 11 unter „Weitere Optionen anzeigen“."),
                |ui| {
                    toggle(ui, &mut self.settings.context_menu);
                },
            );
        });

        section(ui, Some("Wenn eine Konvertierung fertig ist"), None, |g| {
            if g.choice(
                "Popup unten in der Mitte",
                Some("Mit Fortschritt in %, verschwindet nach ein paar Sekunden."),
                self.settings.notify == Notify::Popup,
            ) {
                self.settings.notify = Notify::Popup;
            }
            if g.choice(
                "Windows-Benachrichtigung",
                Some("Unten rechts, bleibt im Benachrichtigungscenter."),
                self.settings.notify == Notify::Windows,
            ) {
                self.settings.notify = Notify::Windows;
            }
        });

        section(
            ui,
            None,
            Some(
                "Lesen: PNG, JPG, WebP, BMP, GIF, TIFF, ICO, HEIC und AVIF (dafür die „HEIF-Bilderweiterungen“ \
                 bzw. „AV1 Video Extension“ aus dem Microsoft Store). Schreiben: PNG, JPG, WebP, BMP, GIF, TIFF, ICO.",
            ),
            |g| {
                g.row("Konverter", Some("Mehrere Bilder mit allen Optionen umwandeln."), |ui| {
                    if ui.button("Öffnen").clicked() {
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe)
                                .args([crate::convert::CLI_ARG, "fenster"])
                                .spawn();
                        }
                    }
                });
            },
        );
    }
}

impl SettingsApp {
    fn about_tab(&mut self, ui: &mut egui::Ui) {
        page_header(ui, "Info & Updates", "Version und Aktualisierungen.");

        let state = self.update.lock().unwrap().clone();
        let mut check = false;
        let mut install = None;
        section(ui, None, None, |g| {
            g.row("Version", None, |ui| {
                ui.label(RichText::new(update::display_version()).color(TEXT_WEAK));
            });
            g.row(
                "Automatisch nach Updates suchen",
                Some("Einmal täglich. Ein Update wird erst nach deiner Bestätigung installiert."),
                |ui| {
                    toggle(ui, &mut self.settings.auto_update);
                },
            );
            let (label, description): (&str, Option<String>) = match &state {
                UpdateState::Idle => ("Nach Updates suchen", None),
                UpdateState::Checking => ("Suche läuft …", None),
                UpdateState::Current => ("UtilityCrab ist aktuell", None),
                UpdateState::Available(r) => ("Update verfügbar", Some(format!("Version {}", r.version))),
                UpdateState::Failed(e) => ("Suche fehlgeschlagen", Some(e.clone())),
            };
            g.row(label, description.as_deref(), |ui| match &state {
                UpdateState::Checking => {
                    ui.add(egui::Spinner::new());
                }
                UpdateState::Available(release) => {
                    if ui.add(primary_button("Jetzt aktualisieren")).clicked() {
                        install = Some(release.clone());
                    }
                    if !release.page_url.is_empty() && ui.button("Änderungen").clicked() {
                        let _ = std::process::Command::new("explorer").arg(&release.page_url).spawn();
                    }
                }
                _ => {
                    if ui.button("Jetzt suchen").clicked() {
                        check = true;
                    }
                }
            });
        });
        if update::release_version().is_none() {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Dies ist ein lokaler Build: Er sucht nicht automatisch nach Updates. \
                             Installierte Versionen aus GitHub-Releases tun das.",
                        )
                        .font(regular(SMALL_SIZE))
                        .color(TEXT_FAINT),
                    )
                    .wrap(),
                );
            });
        }

        if check {
            *self.update.lock().unwrap() = UpdateState::Checking;
            let state = self.update.clone();
            let ctx = ui.ctx().clone();
            thread::spawn(move || {
                let result = match update::check() {
                    Ok(Some(release)) => UpdateState::Available(release),
                    Ok(None) => UpdateState::Current,
                    Err(e) => UpdateState::Failed(e),
                };
                *state.lock().unwrap() = result;
                ctx.request_repaint();
            });
        }
        if let Some(release) = install {
            if let Ok(exe) = std::env::current_exe() {
                let spawned = std::process::Command::new(exe)
                    .args([update::INSTALL_ARG, &release.setup_url, &release.version])
                    .spawn();
                if let Err(e) = spawned {
                    self.message = Some(format!("Update konnte nicht gestartet werden: {e}"));
                }
            }
        }
    }
}

/// Liest eine gedrückte Tastenkombination. `Err(None)` = abgebrochen.
fn capture_hotkey(ui: &egui::Ui) -> Option<Result<String, Option<String>>> {
    use egui::Key;
    let (key, modifiers) = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            // Strg, Alt usw. allein sind noch keine Kombination – auf die eigentliche Taste warten.
            egui::Event::Key {
                key:
                    Key::ShiftLeft
                    | Key::ShiftRight
                    | Key::ControlLeft
                    | Key::ControlRight
                    | Key::AltLeft
                    | Key::AltRight
                    | Key::SuperLeft
                    | Key::SuperRight,
                ..
            } => None,
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Some((*key, *modifiers)),
            _ => None,
        })
    })?;

    if key == egui::Key::Escape {
        return Some(Err(None));
    }
    if !(modifiers.ctrl || modifiers.alt) {
        return Some(Err(Some("Bitte mit Strg und/oder Alt kombinieren.".into())));
    }
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    parts.push(key.name());
    let hotkey = parts.join("+");
    if hotkey.parse::<HotKey>().is_err() {
        return Some(Err(Some(format!("Die Taste „{}“ wird nicht unterstützt.", key.name()))));
    }
    Some(Ok(hotkey))
}
