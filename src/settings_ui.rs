//! Einstellungsdialog (GPUI + GPUI Kit) im Stil der macOS-Systemeinstellungen: Seitenleiste
//! links, rechts Seiten aus gruppierten Zeilen. Läuft als eigener Prozess, damit das
//! Tray-Programm mit seinen Tastenkürzeln davon unberührt bleibt. Änderungen werden sofort
//! gespeichert.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::geom::Color32;
use crate::settings::{hotkey_label, Notify, Pronunciation, Settings};
use crate::tts::{self, Package, VoiceInfo};
use crate::ui_kit::{
    self, button, choice, custom, danger_button, error_banner, h_flex, keycaps, nav_item, page_header, primary_button,
    section, strong, text, v_flex, NavIcon, ACCENT, BODY_SIZE, GREEN, SIDEBAR_BG, SMALL_SIZE,
    TEXT, TEXT_FAINT, TEXT_WEAK,
};
use crate::update;

/// Öffnet das Fenster; `tab` (z. B. „modelle“) wählt die Startseite.
pub fn run(tab: Option<&str>) {
    let start = Tab::ALL
        .into_iter()
        .find(|t| Some(t.id()) == tab)
        .unwrap_or(Tab::Voice);
    ui_kit::run_app(move |cx| {
        let options = ui_kit::window_options("UtilityCrab – Einstellungen", (820.0, 580.0), (680.0, 460.0), cx);
        ui_kit::open_window(options, cx, move |window, cx| cx.new(|cx| SettingsApp::new(start, window, cx)));
    });
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
            Tab::Convert => (NavIcon::Convert, ACCENT),
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

/// Zwei Textfelder je Ausspracheeintrag: Wort → so sprechen.
struct PronunciationRow {
    word: Entity<InputState>,
    spoken: Entity<InputState>,
}

struct SettingsApp {
    tab: Tab,
    settings: Settings,
    saved: Settings,
    speaker: tts::Speaker,
    downloads: Downloads,
    /// Pakete und Stimmen je Provider; neu eingelesen nach Download oder Löschen.
    catalog: Vec<(&'static str, Vec<Package>, Vec<VoiceInfo>)>,
    confirm_delete: Option<String>,
    capture: Option<HotkeyTarget>,
    message: Option<String>,
    crab: Arc<RenderImage>,
    update: Arc<Mutex<UpdateState>>,
    focus: FocusHandle,
    sample_text: Entity<TextareaState>,
    speed: Entity<SliderState>,
    snip_folder: Entity<InputState>,
    pronunciation_rows: Vec<PronunciationRow>,
    pronunciation_subscriptions: Vec<Subscription>,
    was_busy: bool,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl SettingsApp {
    fn new(tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let crab = image::RgbaImage::from_raw(64, 64, crate::crab_pixels::crab_rgba(64))
            .map(|img| ui_kit::render_image(&img))
            .unwrap_or_else(|| ui_kit::render_image(&image::RgbaImage::new(1, 1)));
        let settings = Settings::load();

        let sample_text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(2)
                .default_value("Hallo! So klingt diese Stimme. Ist das privat oder öffentlich?")
        });
        let speed = cx.new(|_| {
            SliderState::new()
                .min(0.5)
                .max(2.0)
                .step(0.05)
                .default_value(settings.speed)
        });
        let default_folder = crate::screenshot::default_folder();
        let snip_folder = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(format!("{} (Standard)", default_folder.display()))
                .default_value(settings.snip_folder.clone())
        });

        let subscriptions = vec![
            cx.subscribe_in(&speed, window, |this, _, event: &SliderEvent, _, cx| {
                if let SliderEvent::Change(value) = event {
                    this.settings.speed = (value.end() * 20.0).round() / 20.0;
                    this.save(cx);
                }
            }),
            cx.subscribe_in(&snip_folder, window, |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.settings.snip_folder = state.read(cx).value().to_string();
                    this.save(cx);
                }
            }),
        ];

        // Fortschritt, Abspielknopf und Update-Suche aktuell halten.
        let poll = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            if this.update_in(cx, |this, _, cx| this.poll(cx)).is_err() {
                break;
            }
        });

        let mut app = Self {
            tab,
            saved: settings.clone(),
            settings,
            speaker: tts::Speaker::new(),
            downloads: Downloads::default(),
            catalog: Vec::new(),
            confirm_delete: None,
            capture: None,
            message: None,
            crab,
            update: Arc::new(Mutex::new(UpdateState::Idle)),
            focus: cx.focus_handle(),
            sample_text,
            speed,
            snip_folder,
            pronunciation_rows: Vec::new(),
            pronunciation_subscriptions: Vec::new(),
            was_busy: false,
            _subscriptions: subscriptions,
            _poll: poll,
        };
        app.refresh_catalog();
        app.rebuild_pronunciation_rows(window, cx);
        app
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
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
            cx.notify();
        }
        let busy = !self.downloads.lock().unwrap().is_empty()
            || self.speaker.is_speaking()
            || matches!(*self.update.lock().unwrap(), UpdateState::Checking);
        if busy || self.was_busy {
            cx.notify();
        }
        self.was_busy = busy;
    }

    /// Speichert, falls sich etwas geändert hat, und zeichnet neu.
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.settings != self.saved {
            match self.settings.save() {
                Ok(()) => self.saved = self.settings.clone(),
                Err(e) => self.message = Some(e),
            }
        }
        cx.notify();
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

    fn speak(&mut self, text: &str, cx: &mut Context<Self>) {
        self.message = self.speaker.speak(text, &self.settings).err();
        cx.notify();
    }

    fn select_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.capture = None;
        self.message = None;
        cx.notify();
    }

    /// Schalter, der ein Feld der Einstellungen setzt und sofort speichert.
    fn switch(&self, id: &'static str, checked: bool, set: fn(&mut Settings, bool), cx: &mut Context<Self>) -> Switch {
        Switch::new(id).checked(checked).on_click(cx.listener(move |this, on: &bool, _, cx| {
            set(&mut this.settings, *on);
            this.save(cx);
        }))
    }

    /// Textfelder für die Ausspracheeinträge neu anlegen (nach Hinzufügen/Entfernen).
    fn rebuild_pronunciation_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pronunciation_rows.clear();
        self.pronunciation_subscriptions.clear();
        for (index, entry) in self.settings.pronunciations.iter().enumerate() {
            let word = cx.new(|cx| InputState::new(window, cx).placeholder("Wort").default_value(entry.word.clone()));
            let spoken = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("So sprechen")
                    .default_value(entry.spoken.clone())
            });
            for (input, is_word) in [(&word, true), (&spoken, false)] {
                self.pronunciation_subscriptions.push(cx.subscribe_in(
                    input,
                    window,
                    move |this, state, event: &InputEvent, _, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let value = state.read(cx).value().to_string();
                        if let Some(entry) = this.settings.pronunciations.get_mut(index) {
                            if is_word {
                                entry.word = value;
                            } else {
                                entry.spoken = value;
                            }
                            this.save(cx);
                        }
                    },
                ));
            }
            self.pronunciation_rows.push(PronunciationRow { word, spoken });
        }
    }

    /// Tastendruck während „Tasten drücken …“.
    fn on_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(target) = self.capture else {
            return;
        };
        match capture_hotkey(&event.keystroke) {
            None => return,
            Some(Ok(hotkey)) => {
                match target {
                    HotkeyTarget::Speak => self.settings.hotkey_speak = hotkey,
                    HotkeyTarget::Ocr => self.settings.hotkey_ocr = hotkey,
                    HotkeyTarget::Snip => self.settings.hotkey_snip = hotkey,
                }
                self.message = None;
                self.capture = None;
            }
            Some(Err(Some(e))) => self.message = Some(e),
            Some(Err(None)) => self.capture = None,
        }
        cx.stop_propagation();
        self.save(cx);
    }
}

impl Render for SettingsApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page: AnyElement = match self.tab {
            Tab::Voice => self.voice_tab(cx).into_any_element(),
            Tab::Models => self.models_tab(cx).into_any_element(),
            Tab::Pronunciation => self.pronunciation_tab(window, cx).into_any_element(),
            Tab::Hotkeys => self.hotkeys_tab(window, cx).into_any_element(),
            Tab::Screenshot => self.screenshot_tab(cx).into_any_element(),
            Tab::Convert => self.convert_tab(cx).into_any_element(),
            Tab::About => self.about_tab(cx).into_any_element(),
        };
        h_flex()
            .id("einstellungen")
            .size_full()
            .items_start()
            .font_family(ui_kit::FONT)
            .text_size(px(BODY_SIZE))
            .text_color(TEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.on_key(event, cx)))
            .child(self.navigation(cx))
            .child(
                div()
                    .id("seite")
                    .flex_1()
                    .h_full()
                    .bg(ui_kit::CONTENT_BG)
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(620.0))
                            .pl(px(28.0))
                            .pr(px(22.0))
                            .pt(px(22.0))
                            .pb(px(12.0))
                            .when_some(self.message.clone(), |el, message| el.child(error_banner(&message).mb(px(14.0))))
                            .child(page),
                    ),
            )
    }
}

impl SettingsApp {
    fn navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(210.0))
            .flex_none()
            .h_full()
            .bg(SIDEBAR_BG)
            .px(px(10.0))
            .child(
                h_flex()
                    .mt(px(18.0))
                    .mb(px(18.0))
                    .pl(px(4.0))
                    .gap(px(8.0))
                    .child(img(self.crab.clone()).size(px(30.0)))
                    .child(
                        v_flex()
                            .child(strong("UtilityCrab", 15.0, TEXT))
                            .child(text("Einstellungen", SMALL_SIZE, TEXT_WEAK)),
                    ),
            )
            .child(v_flex().gap(px(2.0)).children(Tab::ALL.into_iter().map(|tab| {
                let (icon, color) = tab.icon();
                nav_item(
                    tab.id(),
                    self.tab == tab,
                    icon,
                    color,
                    tab.label(),
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.select_tab(tab, cx)),
                )
            })))
            .child(div().flex_1())
            .child(
                div()
                    .px(px(6.0))
                    .pb(px(14.0))
                    .child(text("Änderungen werden sofort gespeichert.", 11.5, TEXT_FAINT)),
            )
    }

    fn voice_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let voices = self.voices_of(&self.settings.provider).to_vec();
        let providers: Vec<(&'static str, &'static str, bool)> = tts::providers()
            .iter()
            .map(|p| (p.id(), p.name(), !self.voices_of(p.id()).is_empty()))
            .collect();
        let view = cx.entity().downgrade();

        let current_provider = providers
            .iter()
            .find(|(id, ..)| *id == self.settings.provider)
            .map(|(_, name, _)| *name)
            .unwrap_or("–");
        let provider_menu = button("provider", current_provider).dropdown_caret(true).dropdown_menu({
            let view = view.clone();
            let selected = self.settings.provider.clone();
            move |mut menu, _, _| {
                for (id, name, installed) in &providers {
                    let label = if *installed {
                        name.to_string()
                    } else {
                        format!("{name} (nicht installiert)")
                    };
                    let view = view.clone();
                    let id = *id;
                    menu = menu.item(PopupMenuItem::new(label).checked(selected == id).on_click(move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.settings.provider = id.to_string();
                            this.settings.voice = String::new();
                            this.save(cx);
                        });
                    }));
                }
                menu
            }
        });

        let voice_control: AnyElement = if voices.is_empty() {
            h_flex()
                .gap(px(8.0))
                .child(text("Noch keine installiert", BODY_SIZE, TEXT_WEAK))
                .child(
                    button("modelle-laden", "Modelle laden …")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.select_tab(Tab::Models, cx))),
                )
                .into_any_element()
        } else {
            let current = voices
                .iter()
                .find(|v| v.id == self.settings.voice)
                .unwrap_or(&voices[0])
                .clone();
            button("stimme", current.name.clone())
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for voice in &voices {
                        let view = view.clone();
                        let id = voice.id.clone();
                        menu = menu.item(PopupMenuItem::new(voice.name.clone()).checked(voice.id == current.id).on_click(
                            move |_, _, cx| {
                                let id = id.clone();
                                let _ = view.update(cx, |this, cx| {
                                    this.settings.voice = id;
                                    this.save(cx);
                                });
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        };

        let speaking = self.speaker.is_speaking();
        let play = if speaking {
            button("probe", "Stopp")
        } else {
            primary_button("probe", "Probe hören")
        }
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if this.speaker.is_speaking() {
                this.speaker.stop();
                cx.notify();
            } else {
                let text = this.sample_text.read(cx).value().to_string();
                this.speak(&text, cx);
            }
        }));

        v_flex()
            .child(page_header("Stimme", "Mit welcher Stimme markierter Text vorgelesen wird."))
            .child(
                section()
                    .row("Engine", None, provider_menu)
                    .row("Stimme", None, voice_control)
                    .row(
                        "Tempo",
                        None,
                        h_flex()
                            .gap(px(10.0))
                            .child(div().w(px(180.0)).child(Slider::new(&self.speed)))
                            .child(strong(format!("{:.2}×", self.settings.speed), BODY_SIZE, TEXT).w(px(44.0))),
                    )
                    .row(
                        "Sprache automatisch erkennen",
                        Some("Wechselt pro Satz zu einer installierten Stimme der erkannten Sprache, z. B. für einen englischen Satz."),
                        self.switch("auto-sprache", self.settings.auto_language, |s, v| s.auto_language = v, cx),
                    ),
            )
            .child(
                section()
                    .title("Probehören")
                    .child(custom(v_flex().gap(px(8.0)).child(Textarea::new(&self.sample_text)).child(h_flex().child(play)))),
            )
    }

    fn models_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let downloads = self.downloads.lock().unwrap();
        let mut page = v_flex().child(page_header(
            "Modelle",
            "Stimmen-Modelle herunterladen oder löschen. Alles läuft lokal, kein Text verlässt den PC.",
        ));
        for provider in tts::providers() {
            let Some((packages, voices)) = self
                .catalog
                .iter()
                .find(|(id, ..)| *id == provider.id())
                .map(|(_, p, v)| (p, v))
            else {
                continue;
            };
            let mut group = section().title(provider.name()).footnote(provider.description());
            if packages.is_empty() {
                group = group.row(
                    "Keine Installation nötig",
                    Some(&format!("{} Stimmen verfügbar", voices.len())),
                    text("Bereit", BODY_SIZE, GREEN),
                );
            }
            let provider_id = provider.id();
            for package in packages {
                let key = format!("{provider_id}/{}", package.id);
                let size = format!("{} MB", package.size_mb);
                let package_id = package.id.clone();
                let control: AnyElement = match downloads.get(&key) {
                    Some(DownloadState::Running(progress)) => h_flex()
                        .gap(px(8.0))
                        .child(div().w(px(150.0)).child(Progress::new(SharedString::from(format!("dl-{key}"))).value(progress * 100.0)))
                        .child(text(format!("{:.0} %", progress * 100.0), SMALL_SIZE, TEXT_WEAK))
                        .into_any_element(),
                    _ if package.installed => {
                        if self.confirm_delete.as_deref() == Some(key.as_str()) {
                            h_flex()
                                .gap(px(8.0))
                                .child(button(SharedString::from(format!("abbrechen-{key}")), "Abbrechen").on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirm_delete = None;
                                        cx.notify();
                                    }),
                                ))
                                .child(danger_button(SharedString::from(format!("loeschen-{key}")), "Löschen").on_click(
                                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.confirm_delete = None;
                                        this.speaker.stop();
                                        if let Some(provider) = tts::provider(provider_id) {
                                            if let Err(e) = provider.uninstall(&package_id) {
                                                this.message = Some(e);
                                            }
                                        }
                                        this.refresh_catalog();
                                        cx.notify();
                                    }),
                                ))
                                .into_any_element()
                        } else {
                            let confirm_key = key.clone();
                            h_flex()
                                .gap(px(8.0))
                                .child(text("Installiert", BODY_SIZE, GREEN))
                                .child(button(SharedString::from(format!("frage-{key}")), "Löschen").on_click(cx.listener(
                                    move |this, _: &ClickEvent, _, cx| {
                                        this.confirm_delete = Some(confirm_key.clone());
                                        cx.notify();
                                    },
                                )))
                                .into_any_element()
                        }
                    }
                    _ => primary_button(SharedString::from(format!("laden-{key}")), "Laden")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.start_download(provider_id, package_id.clone());
                            cx.notify();
                        }))
                        .into_any_element(),
                };
                group = group.row(package.name.clone(), Some(&size), control);
            }
            page = page.child(group);
        }
        page
    }

    fn start_download(&mut self, provider_id: &'static str, package: String) {
        let key = format!("{provider_id}/{package}");
        let downloads = self.downloads.clone();
        downloads.lock().unwrap().insert(key.clone(), DownloadState::Running(0.0));
        // Das Fenster fragt den Stand alle 200 ms ab.
        thread::spawn(move || {
            let Some(provider) = tts::provider(provider_id) else {
                return;
            };
            let result = provider.install(&package, &mut |progress| {
                downloads
                    .lock()
                    .unwrap()
                    .insert(key.clone(), DownloadState::Running(progress));
            });
            let state = match result {
                Ok(()) => DownloadState::Done,
                Err(e) => DownloadState::Failed(e),
            };
            downloads.lock().unwrap().insert(key, state);
        });
    }

    fn pronunciation_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pronunciation_rows.len() != self.settings.pronunciations.len() {
            self.rebuild_pronunciation_rows(window, cx);
        }
        let mut group =
            section().footnote("Beispiel: „privat“ als „priwaht“. Groß- und Kleinschreibung spielt keine Rolle.");
        for (i, row) in self.pronunciation_rows.iter().enumerate() {
            group = group.child(custom(
                h_flex()
                    .gap(px(8.0))
                    .child(div().flex_1().child(Input::new(&row.word)))
                    .child(text("→", 13.0, TEXT_FAINT))
                    .child(div().flex_1().child(Input::new(&row.spoken)))
                    .child(button(("anhoeren", i), "Anhören").on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if let Some(entry) = this.settings.pronunciations.get(i) {
                            let word = entry.word.clone();
                            this.speak(&word, cx);
                        }
                    })))
                    .child(button(("entfernen", i), "Entfernen").on_click(cx.listener(
                        move |this, _: &ClickEvent, window, cx| {
                            if i < this.settings.pronunciations.len() {
                                this.settings.pronunciations.remove(i);
                                this.rebuild_pronunciation_rows(window, cx);
                                this.save(cx);
                            }
                        },
                    ))),
            ));
        }
        if self.settings.pronunciations.is_empty() {
            group = group.row("Noch keine Einträge", None, div());
        }
        group = group.child(custom(
            div()
                .id("hinzufuegen")
                .cursor_pointer()
                .child(text("+ Eintrag hinzufügen", BODY_SIZE, ACCENT))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.settings.pronunciations.push(Pronunciation::default());
                    this.rebuild_pronunciation_rows(window, cx);
                    this.save(cx);
                })),
        ));

        v_flex()
            .child(page_header(
                "Aussprache",
                "Wörter, die falsch ausgesprochen werden, so aufschreiben, wie sie klingen sollen. Gilt für alle Stimmen.",
            ))
            .child(group)
    }

    fn hotkeys_tab(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Während der Aufnahme bekommt die Seite die Tastendrücke.
        if self.capture.is_some() && !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let rows = [
            (HotkeyTarget::Speak, "Markierten Text vorlesen", "Nochmal drücken stoppt das Vorlesen.", &self.settings.hotkey_speak),
            (HotkeyTarget::Ocr, "Text aus Bildausschnitt kopieren", "Bereich auswählen, der Text landet in der Zwischenablage.", &self.settings.hotkey_ocr),
            (HotkeyTarget::Snip, "Screenshot aufnehmen", "Bereich, Fenster oder Bildschirm wählen, danach bearbeiten.", &self.settings.hotkey_snip),
        ];
        let mut group = section().footnote(
            "Kombinationen mit Strg und Alt eignen sich gut. Ist ein Kürzel schon von einem anderen Programm belegt, steht das im Fehlerprotokoll.",
        );
        for (i, (target, label, description, value)) in rows.into_iter().enumerate() {
            let control: AnyElement = if self.capture == Some(target) {
                text("Tasten drücken … (Esc bricht ab)", BODY_SIZE, ACCENT).into_any_element()
            } else {
                h_flex()
                    .gap(px(8.0))
                    .child(keycaps(&hotkey_label(value)))
                    .child(button(("aendern", i), "Ändern").on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.capture = Some(target);
                        this.message = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    })))
                    .into_any_element()
            };
            group = group.row(label, Some(description), control);
        }
        v_flex()
            .child(page_header("Tastenkürzel", "Globale Tastenkürzel, die in jedem Programm funktionieren."))
            .child(group)
    }

    fn screenshot_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let spawn = |args: &[&str]| {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).args(args).spawn();
            }
        };
        let default_folder = crate::screenshot::default_folder();
        let custom_folder = !self.settings.snip_folder.trim().is_empty();
        let current = if custom_folder {
            std::path::PathBuf::from(self.settings.snip_folder.trim())
        } else {
            default_folder.clone()
        };

        let browse = {
            let current = current.clone();
            let default_folder = default_folder.clone();
            button("durchsuchen", "Durchsuchen …").on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if let Some(folder) = crate::screenshot::pick_folder(&current) {
                    // Wieder den Standard gewählt: leer lassen, dann folgt er z. B. einem neuen Benutzerprofil.
                    let value = if folder == default_folder { String::new() } else { folder.display().to_string() };
                    this.snip_folder.update(cx, |s, cx| s.set_value(value.clone(), window, cx));
                    this.settings.snip_folder = value;
                    this.save(cx);
                }
            }))
        };
        let open = {
            let current = current.clone();
            button("oeffnen", "Öffnen").tooltip("Ordner im Explorer anzeigen").on_click(move |_, _, _| {
                let _ = std::fs::create_dir_all(&current);
                let _ = std::process::Command::new("explorer").arg(&current).spawn();
            })
        };
        let reset = custom_folder.then(|| {
            button("standard", "Standard")
                .tooltip("Zurück zu Bilder\\Screenshots")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.snip_folder.update(cx, |s, cx| s.set_value("", window, cx));
                    this.settings.snip_folder.clear();
                    this.save(cx);
                }))
        });

        v_flex()
            .child(page_header(
                "Screenshot",
                "Bildschirmausschnitte aufnehmen, beschriften, unkenntlich machen und teilen.",
            ))
            .child(
                section()
                    .title("Nach der Aufnahme")
                    .row(
                        "Editor öffnen",
                        Some("Zum Beschriften, Zuschneiden und Unkenntlichmachen. Sonst erscheint nur ein Hinweis mit „Bearbeiten“."),
                        self.switch("editor", self.settings.snip_open_editor, |s, v| s.snip_open_editor = v, cx),
                    )
                    .row(
                        "In die Zwischenablage kopieren",
                        Some("Auch Änderungen im Editor landen automatisch dort – einfach mit Strg+V einfügen."),
                        self.switch("kopieren", self.settings.snip_copy, |s, v| s.snip_copy = v, cx),
                    )
                    .row(
                        "Automatisch speichern",
                        Some("Jeden Screenshot als PNG im Ordner unten ablegen."),
                        self.switch("autosave", self.settings.snip_autosave, |s, v| s.snip_autosave = v, cx),
                    )
                    .child(custom(
                        v_flex()
                            .gap(px(6.0))
                            .child(
                                v_flex().child(text("Ordner für Screenshots", BODY_SIZE, TEXT)).child(text(
                                    "Hier landen automatisch gespeicherte Screenshots; „Speichern unter“ im Editor schlägt ihn ebenfalls vor.",
                                    SMALL_SIZE,
                                    TEXT_WEAK,
                                )),
                            )
                            .child(
                                h_flex()
                                    .gap(px(8.0))
                                    .child(div().flex_1().min_w(px(120.0)).child(Input::new(&self.snip_folder)))
                                    .child(browse)
                                    .child(open)
                                    .children(reset),
                            ),
                    )),
            )
            .child(
                section().title("Explorer").row(
                    "„Mit UtilityCrab bearbeiten“ im Kontextmenü",
                    Some("Rechtsklick auf ein Bild öffnet es im Editor. Unter Windows 11 unter „Weitere Optionen anzeigen“."),
                    self.switch("edit-kontextmenue", self.settings.edit_context_menu, |s, v| s.edit_context_menu = v, cx),
                ),
            )
            .child(
                section()
                    .footnote(
                        "Im Editor: Stift, Textmarker, Pfeile, Formen, Text, Schrittnummern, Verpixeln/Weichzeichnen/Schwärzen, \
                         Spotlight, Zuschneiden, Text erkennen, sensible Daten automatisch unkenntlich machen und Anheften.",
                    )
                    .row(
                        "Screenshot aufnehmen",
                        Some("Auch über das Tastenkürzel oder das Tray-Menü."),
                        h_flex()
                            .gap(px(8.0))
                            .child(button("in-5", "In 5 Sekunden").on_click(move |_, _, _| {
                                spawn(&[crate::screenshot::CAPTURE_ARG, crate::screenshot::Prefs::load().mode.id(), "5"]);
                            }))
                            .child(button("jetzt", "Jetzt").on_click(move |_, _, _| {
                                spawn(&[crate::screenshot::CAPTURE_ARG, crate::screenshot::Prefs::load().mode.id(), "0"]);
                            })),
                    )
                    .row(
                        "Vorhandenes Bild bearbeiten",
                        Some("Öffnet den Editor; Bilder lassen sich auch hineinziehen."),
                        button("editor-oeffnen", "Editor öffnen").on_click(move |_, _, _| spawn(&[crate::screenshot::EDIT_ARG])),
                    ),
            )
    }

    fn convert_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let notify = self.settings.notify;
        v_flex()
            .child(page_header("Konvertieren", "Bilder in andere Formate umwandeln und Dateien verkleinern."))
            .child(
                section().title("Explorer").row(
                    "Im Kontextmenü anzeigen",
                    Some("Rechtsklick auf ein Bild → „Konvertieren“. Unter Windows 11 unter „Weitere Optionen anzeigen“."),
                    self.switch("kontextmenue", self.settings.context_menu, |s, v| s.context_menu = v, cx),
                ),
            )
            .child(
                section()
                    .title("Wenn eine Konvertierung fertig ist")
                    .child(choice(
                        "popup",
                        "Popup unten in der Mitte",
                        Some("Mit Fortschritt in %, verschwindet nach ein paar Sekunden."),
                        notify == Notify::Popup,
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.settings.notify = Notify::Popup;
                            this.save(cx);
                        }),
                    ))
                    .child(choice(
                        "windows",
                        "Windows-Benachrichtigung",
                        Some("Unten rechts, bleibt im Benachrichtigungscenter."),
                        notify == Notify::Windows,
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.settings.notify = Notify::Windows;
                            this.save(cx);
                        }),
                    )),
            )
            .child(
                section()
                    .footnote(
                        "Lesen: PNG, JPG, WebP, BMP, GIF, TIFF, ICO, HEIC und AVIF (dafür die „HEIF-Bilderweiterungen“ \
                         bzw. „AV1 Video Extension“ aus dem Microsoft Store). Schreiben: PNG, JPG, WebP, BMP, GIF, TIFF, ICO.",
                    )
                    .row(
                        "Konverter",
                        Some("Mehrere Bilder mit allen Optionen umwandeln."),
                        button("konverter", "Öffnen").on_click(|_, _, _| {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = std::process::Command::new(exe)
                                    .args([crate::convert::CLI_ARG, "fenster"])
                                    .spawn();
                            }
                        }),
                    ),
            )
    }

    fn about_tab(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.update.lock().unwrap().clone();
        let (label, description): (&str, Option<String>) = match &state {
            UpdateState::Idle => ("Nach Updates suchen", None),
            UpdateState::Checking => ("Suche läuft …", None),
            UpdateState::Current => ("UtilityCrab ist aktuell", None),
            UpdateState::Available(r) => ("Update verfügbar", Some(format!("Version {}", r.version))),
            UpdateState::Failed(e) => ("Suche fehlgeschlagen", Some(e.clone())),
        };
        let control: AnyElement = match &state {
            UpdateState::Checking => Spinner::new().into_any_element(),
            UpdateState::Available(release) => {
                let install = release.clone();
                let page_url = release.page_url.clone();
                h_flex()
                    .gap(px(8.0))
                    .when(!page_url.is_empty(), |el| {
                        el.child(button("aenderungen", "Änderungen").on_click(move |_, _, _| {
                            let _ = std::process::Command::new("explorer").arg(&page_url).spawn();
                        }))
                    })
                    .child(primary_button("aktualisieren", "Jetzt aktualisieren").on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            if let Ok(exe) = std::env::current_exe() {
                                let spawned = std::process::Command::new(exe)
                                    .args([update::INSTALL_ARG, &install.setup_url, &install.version])
                                    .spawn();
                                if let Err(e) = spawned {
                                    this.message = Some(format!("Update konnte nicht gestartet werden: {e}"));
                                    cx.notify();
                                }
                            }
                        },
                    )))
                    .into_any_element()
            }
            _ => button("suchen", "Jetzt suchen")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    *this.update.lock().unwrap() = UpdateState::Checking;
                    let state = this.update.clone();
                    thread::spawn(move || {
                        let result = match update::check() {
                            Ok(Some(release)) => UpdateState::Available(release),
                            Ok(None) => UpdateState::Current,
                            Err(e) => UpdateState::Failed(e),
                        };
                        *state.lock().unwrap() = result;
                    });
                    cx.notify();
                }))
                .into_any_element(),
        };

        v_flex()
            .child(page_header("Info & Updates", "Version und Aktualisierungen."))
            .child(
                section()
                    .row("Version", None, text(update::display_version(), BODY_SIZE, TEXT_WEAK))
                    .row(
                        "Automatisch nach Updates suchen",
                        Some("Einmal täglich. Ein Update wird erst nach deiner Bestätigung installiert."),
                        self.switch("auto-update", self.settings.auto_update, |s, v| s.auto_update = v, cx),
                    )
                    .row(label, description.as_deref(), control),
            )
            .when(update::release_version().is_none(), |el| {
                el.child(div().px(px(4.0)).child(text(
                    "Dies ist ein lokaler Build: Er sucht nicht automatisch nach Updates. \
                     Installierte Versionen aus GitHub-Releases tun das.",
                    SMALL_SIZE,
                    TEXT_FAINT,
                )))
            })
    }
}

/// Liest eine gedrückte Tastenkombination. `None` = noch keine vollständige Kombination
/// (z. B. nur Strg gedrückt), `Err(None)` = abgebrochen.
fn capture_hotkey(keystroke: &Keystroke) -> Option<Result<String, Option<String>>> {
    let key = keystroke.key.as_str();
    // Strg, Alt usw. allein sind noch keine Kombination – auf die eigentliche Taste warten.
    if matches!(key, "control" | "alt" | "shift" | "platform" | "function" | "fn" | "") {
        return None;
    }
    if key == "escape" {
        return Some(Err(None));
    }
    let m = keystroke.modifiers;
    if !(m.control || m.alt) {
        return Some(Err(Some("Bitte mit Strg und/oder Alt kombinieren.".into())));
    }
    let name = key_name(key);
    let mut parts = Vec::new();
    if m.control {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(&name);
    let hotkey = parts.join("+");
    if hotkey.parse::<HotKey>().is_err() {
        return Some(Err(Some(format!("Die Taste „{name}“ wird nicht unterstützt."))));
    }
    Some(Ok(hotkey))
}

/// GPUI-Tastenname → Name, den `global_hotkey` versteht (und der lesbar ist).
fn key_name(key: &str) -> String {
    match key {
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "backspace" => "Backspace".into(),
        _ if key.chars().count() == 1 => key.to_uppercase(),
        _ => {
            let mut chars = key.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::key_name;
    use global_hotkey::hotkey::HotKey;

    #[test]
    fn maps_gpui_key_names() {
        assert_eq!(key_name("a"), "A");
        assert_eq!(key_name("f5"), "F5");
        assert_eq!(key_name("space"), "Space");
        assert_eq!(key_name("up"), "ArrowUp");
        for name in ["A", "F5", "Space", "ArrowUp", "PageDown", "Home", "Delete", "1"] {
            assert!(format!("Ctrl+Alt+{name}").parse::<HotKey>().is_ok(), "{name}");
        }
    }
}
