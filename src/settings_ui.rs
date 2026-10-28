//! Einstellungsdialog (egui). Läuft als eigener Prozess, damit das Tray-Programm
//! mit seinen Tastenkürzeln davon unberührt bleibt. Änderungen werden sofort gespeichert.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use global_hotkey::hotkey::HotKey;

use crate::icon;
use crate::settings::{hotkey_label, Pronunciation, Settings};
use crate::tts::{self, Package, VoiceInfo};

const ACCENT: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x1C);

// Dunkelgraues Farbschema
const NAV_BG: Color32 = Color32::from_rgb(0x1F, 0x20, 0x23);
const PAGE_BG: Color32 = Color32::from_rgb(0x2B, 0x2D, 0x31);
const CARD_BG: Color32 = Color32::from_rgb(0x34, 0x36, 0x3B);
const FIELD_BG: Color32 = Color32::from_rgb(0x1E, 0x1F, 0x22);
const TEXT: Color32 = Color32::from_rgb(0xE3, 0xE5, 0xE8);

fn gray_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PAGE_BG;
    v.window_fill = PAGE_BG;
    v.extreme_bg_color = FIELD_BG;
    v.faint_bg_color = Color32::from_rgb(0x30, 0x32, 0x37);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = egui::Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;

    let radius = egui::CornerRadius::same(6);
    for (widget, fill) in [
        (&mut v.widgets.inactive, Color32::from_rgb(0x40, 0x42, 0x48)),
        (&mut v.widgets.hovered, Color32::from_rgb(0x4C, 0x4F, 0x56)),
        (&mut v.widgets.active, Color32::from_rgb(0x58, 0x5B, 0x63)),
        (&mut v.widgets.open, Color32::from_rgb(0x40, 0x42, 0x48)),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = egui::Stroke::NONE;
        widget.corner_radius = radius;
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(0x3A, 0x3C, 0x41));
    v
}

pub fn run() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Einstellungen")
            .with_inner_size([760.0, 540.0])
            .with_min_inner_size([600.0, 420.0])
            .with_icon(egui::IconData {
                rgba: icon::crab_rgba(),
                width: icon::SIZE,
                height: icon::SIZE,
            }),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab",
        options,
        Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))),
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
}

#[derive(Clone, Copy, PartialEq)]
enum HotkeyTarget {
    Speak,
    Ocr,
}

enum DownloadState {
    Running(f32),
    Done,
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
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.all_styles_mut(|style| {
            style.visuals = gray_visuals();
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 5.0);
            style.spacing.interact_size.y = 28.0;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
        });

        let settings = Settings::load();
        let mut app = Self {
            tab: Tab::Voice,
            saved: settings.clone(),
            settings,
            speaker: tts::Speaker::new(),
            sample_text: "Hallo! So klingt diese Stimme. Ist das privat oder öffentlich?".into(),
            downloads: Downloads::default(),
            catalog: Vec::new(),
            confirm_delete: None,
            capture: None,
            message: None,
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
            .exact_size(200.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(NAV_BG).inner_margin(egui::Margin::symmetric(12, 0)))
            .show(ui, |ui| self.navigation(ui));

        let page = egui::Frame::new().fill(PAGE_BG).inner_margin(egui::Margin::symmetric(24, 12));
        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);
                match self.tab {
                    Tab::Voice => self.voice_tab(ui),
                    Tab::Models => self.models_tab(ui),
                    Tab::Pronunciation => self.pronunciation_tab(ui),
                    Tab::Hotkeys => self.hotkeys_tab(ui),
                }
                if let Some(message) = &self.message {
                    ui.add_space(12.0);
                    ui.colored_label(Color32::from_rgb(0xD0, 0x40, 0x30), message);
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
        let busy = !self.downloads.lock().unwrap().is_empty() || self.speaker.is_speaking();
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }
}

impl SettingsApp {
    fn navigation(&mut self, ui: &mut egui::Ui) {
        ui.add_space(16.0);
        ui.label(RichText::new("UtilityCrab").size(22.0).strong().color(ACCENT));
        ui.label(RichText::new("Einstellungen").weak());
        ui.add_space(20.0);

        for (tab, label) in [
            (Tab::Voice, "Stimme"),
            (Tab::Models, "Modelle"),
            (Tab::Pronunciation, "Aussprache"),
            (Tab::Hotkeys, "Tastenkürzel"),
        ] {
            let selected = self.tab == tab;
            let text = RichText::new(label).size(16.0);
            let button = egui::Button::selectable(selected, text).min_size(egui::vec2(ui.available_width(), 36.0));
            if ui.add(button).clicked() {
                self.tab = tab;
                self.capture = None;
                self.message = None;
            }
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("Änderungen werden sofort übernommen.").small().weak());
        });
    }

    fn voice_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stimme");
        ui.label(RichText::new("Mit welcher Stimme markierter Text vorgelesen wird.").weak());
        ui.add_space(12.0);

        egui::Grid::new("voice_grid")
            .num_columns(2)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                ui.label("Engine");
                let current = tts::provider(&self.settings.provider)
                    .map(|p| p.name())
                    .unwrap_or("–");
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(current)
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        for provider in tts::providers() {
                            let installed = !self.voices_of(provider.id()).is_empty();
                            let label = if installed {
                                provider.name().to_string()
                            } else {
                                format!("{} (nicht installiert)", provider.name())
                            };
                            if ui
                                .selectable_label(self.settings.provider == provider.id(), label)
                                .clicked()
                            {
                                self.settings.provider = provider.id().into();
                                self.settings.voice = String::new();
                            }
                        }
                    });
                ui.end_row();

                ui.label("Stimme");
                let voices = self.voices_of(&self.settings.provider);
                if voices.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Noch keine Stimme installiert.").weak());
                        if ui.link("Zu den Modellen").clicked() {
                            self.tab = Tab::Models;
                        }
                    });
                } else {
                    let current = voices
                        .iter()
                        .find(|v| v.id == self.settings.voice)
                        .unwrap_or(&voices[0]);
                    let mut selected = current.id.clone();
                    egui::ComboBox::from_id_salt("voice")
                        .selected_text(&current.name)
                        .width(320.0)
                        .show_ui(ui, |ui| {
                            for voice in voices {
                                ui.selectable_value(&mut selected, voice.id.clone(), &voice.name);
                            }
                        });
                    if selected != self.settings.voice {
                        self.settings.voice = selected;
                    }
                }
                ui.end_row();

                ui.label("Tempo");
                ui.add(
                    egui::Slider::new(&mut self.settings.speed, 0.5..=2.0)
                        .step_by(0.05)
                        .suffix("×"),
                );
                ui.end_row();

                ui.label("Sprache");
                ui.checkbox(&mut self.settings.auto_language, "Automatisch erkennen")
                    .on_hover_text(
                        "Erkennt pro Satz die Sprache. Piper und Windows wechseln dann zu einer \
                         installierten Stimme dieser Sprache, z. B. für einen englischen Satz.",
                    );
                ui.end_row();
            });

        ui.add_space(20.0);
        ui.label(RichText::new("Probehören").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.sample_text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        if self.speaker.is_speaking() {
            if ui.button("Stopp").clicked() {
                self.speaker.stop();
            }
        } else if ui.add(accent_button("Probe hören")).clicked() {
            let text = self.sample_text.clone();
            self.speak(&text);
        }
    }

    fn models_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Modelle");
        ui.label(
            RichText::new("Stimmen-Modelle herunterladen oder löschen. Alles läuft lokal, kein Text verlässt den PC.")
                .weak(),
        );
        ui.add_space(12.0);

        let mut start_download = None;
        let mut delete = None;
        let downloads = self.downloads.lock().unwrap();

        for provider in tts::providers() {
            let (packages, voices) = self
                .catalog
                .iter()
                .find(|(id, ..)| *id == provider.id())
                .map(|(_, p, v)| (p, v))
                .expect("Katalog unvollständig");

            egui::Frame::new()
                .fill(CARD_BG)
                .inner_margin(egui::Margin::same(16))
                .corner_radius(10.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(provider.name()).size(18.0).strong());
                    ui.label(RichText::new(provider.description()).weak());
                    ui.add_space(6.0);

                    if packages.is_empty() {
                        ui.label(format!("Keine Installation nötig – {} Stimmen verfügbar", voices.len()));
                        return;
                    }
                    egui::Grid::new(provider.id())
                        .num_columns(3)
                        .spacing([16.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for package in packages {
                                let key = format!("{}/{}", provider.id(), package.id);
                                ui.label(&package.name);
                                ui.label(RichText::new(format!("{} MB", package.size_mb)).weak());

                                match downloads.get(&key) {
                                    Some(DownloadState::Running(progress)) => {
                                        ui.add(
                                            egui::ProgressBar::new(*progress)
                                                .desired_width(160.0)
                                                .show_percentage(),
                                        );
                                    }
                                    _ if package.installed => {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Installiert").color(Color32::from_rgb(0x3C, 0xA0, 0x50)));
                                            if self.confirm_delete.as_deref() == Some(key.as_str()) {
                                                if ui.button("Wirklich löschen?").clicked() {
                                                    delete = Some((provider.id(), package.id.clone()));
                                                }
                                                if ui.button("Abbrechen").clicked() {
                                                    self.confirm_delete = None;
                                                }
                                            } else if ui.button("Löschen").clicked() {
                                                self.confirm_delete = Some(key.clone());
                                            }
                                        });
                                    }
                                    _ => {
                                        if ui.add(accent_button("Herunterladen")).clicked() {
                                            start_download = Some((provider.id(), package.id.clone()));
                                        }
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.add_space(10.0);
        }
        drop(downloads);

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
        ui.heading("Aussprache");
        ui.label(
            RichText::new(
                "Wörter, die falsch ausgesprochen werden, so aufschreiben, wie sie klingen sollen \
                 (z. B. „privat“ als „priwaht“). Gilt für alle Stimmen.",
            )
            .weak(),
        );
        ui.add_space(12.0);

        let mut remove = None;
        let mut play = None;
        egui::Grid::new("pronunciations")
            .num_columns(3)
            .spacing([12.0, 8.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Wort").strong());
                ui.label(RichText::new("So sprechen").strong());
                ui.label("");
                ui.end_row();

                for (i, entry) in self.settings.pronunciations.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut entry.word).hint_text("privat").desired_width(200.0));
                    ui.add(egui::TextEdit::singleline(&mut entry.spoken).hint_text("priwaht").desired_width(200.0));
                    ui.horizontal(|ui| {
                        if ui.button("Anhören").clicked() {
                            play = Some(entry.word.clone());
                        }
                        if ui.button("Entfernen").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });

        if let Some(i) = remove {
            self.settings.pronunciations.remove(i);
        }
        if let Some(word) = play {
            self.speak(&word);
        }
        ui.add_space(8.0);
        if ui.add(accent_button("+ Eintrag hinzufügen")).clicked() {
            self.settings.pronunciations.push(Pronunciation::default());
        }
    }

    fn hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tastenkürzel");
        ui.label(RichText::new("Globale Tastenkürzel, die in jedem Programm funktionieren.").weak());
        ui.add_space(12.0);

        if let Some(target) = self.capture {
            if let Some(hotkey) = capture_hotkey(ui) {
                match hotkey {
                    Ok(hotkey) => {
                        match target {
                            HotkeyTarget::Speak => self.settings.hotkey_speak = hotkey,
                            HotkeyTarget::Ocr => self.settings.hotkey_ocr = hotkey,
                        }
                        self.message = None;
                        self.capture = None;
                    }
                    Err(Some(e)) => self.message = Some(e),
                    Err(None) => self.capture = None,
                }
            }
        }

        egui::Grid::new("hotkeys")
            .num_columns(3)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                for (target, label, value) in [
                    (HotkeyTarget::Speak, "Markierten Text vorlesen", &self.settings.hotkey_speak),
                    (HotkeyTarget::Ocr, "Text aus Bildausschnitt kopieren", &self.settings.hotkey_ocr),
                ] {
                    ui.label(label);
                    if self.capture == Some(target) {
                        ui.label(RichText::new("Tasten drücken … (Esc = Abbrechen)").color(ACCENT));
                        ui.label("");
                    } else {
                        ui.label(RichText::new(hotkey_label(value)).monospace().strong());
                        if ui.button("Ändern").clicked() {
                            self.capture = Some(target);
                            self.message = None;
                        }
                    }
                    ui.end_row();
                }
            });

        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Tipp: Kombinationen mit Strg und Alt eignen sich gut. \
                 Ist ein Kürzel schon von einem anderen Programm belegt, steht das im Fehlerprotokoll.",
            )
            .weak(),
        );
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

fn accent_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(ACCENT)
}
