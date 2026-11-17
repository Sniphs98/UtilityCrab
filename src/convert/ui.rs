//! Fenster mit allen Einstellungen: Format, Qualität, Größe, Zieldateigröße.
//! Dateien kommen aus dem Kontextmenü (auch nachträglich) oder per Drag & Drop.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rayon::prelude::*;

use super::batch::Queue;
use super::{convert_file, copy_to_clipboard, describe, human_size, Format, Options, Outcome, Resize, INPUT_EXTENSIONS};
use crate::ui_kit::{
    self, button, h_flex, page_header, primary_button, section, strong, text, v_flex, Disableable, Sizable, ACCENT,
    BODY_SIZE, CONTENT_BG, GREEN, RED, SMALL_SIZE, TEXT, TEXT_WEAK,
};

/// Öffnet das Fenster und blockiert, bis es geschlossen ist.
pub fn run(files: Vec<PathBuf>, queue: Option<Queue>) {
    ui_kit::run_app(move |cx| {
        let options = ui_kit::window_options("UtilityCrab – Bilder konvertieren", (700.0, 640.0), (560.0, 480.0), cx);
        ui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| {
                let mut app = ConverterApp::new(queue, window, cx);
                app.add_files(files);
                app
            })
        });
    });
}

#[derive(Clone)]
enum Status {
    Ready,
    Working,
    Done(Outcome),
    Failed(String),
}

struct Item {
    path: PathBuf,
    size: u64,
    status: Arc<Mutex<Status>>,
    /// Fortschritt in Promille.
    progress: Arc<AtomicU32>,
}

struct ConverterApp {
    items: Vec<Item>,
    options: Options,
    saved: Options,
    queue: Option<Queue>,
    running: Arc<Mutex<bool>>,
    was_running: bool,
    /// Zuletzt eingestellte Werte, damit sie beim Umschalten der Größenart erhalten bleiben.
    long_edge: u32,
    percent: u32,
    max_kb: u32,
    message: Option<String>,
    /// Hinweis nach dem Durchlauf, z. B. dass die Dateien in der Zwischenablage liegen.
    notice: Arc<Mutex<Option<String>>>,
    quality: Entity<SliderState>,
    long_edge_input: Entity<InputState>,
    percent_input: Entity<InputState>,
    max_kb_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

fn options_path() -> PathBuf {
    crate::data_dir().join("konverter.json")
}

/// Zahlenfeld: nur Ziffern, Wert wird beim Tippen übernommen und beim Verlassen begrenzt.
fn number_input(value: u32, window: &mut Window, cx: &mut Context<ConverterApp>) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value.to_string())
            .validate(|text, _| text.chars().all(|c| c.is_ascii_digit()) && text.len() <= 6)
    })
}

impl ConverterApp {
    fn new(queue: Option<Queue>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let options: Options = fs::read_to_string(options_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let long_edge = match options.resize {
            Resize::LongEdge(v) => v,
            _ => 1920,
        };
        let percent = match options.resize {
            Resize::Percent(v) => v,
            _ => 50,
        };
        let max_kb = options.max_kb.unwrap_or(500);

        let quality = cx.new(|_| {
            SliderState::new()
                .min(1.0)
                .max(100.0)
                .step(1.0)
                .default_value(options.quality as f32)
        });
        let long_edge_input = number_input(long_edge, window, cx);
        let percent_input = number_input(percent, window, cx);
        let max_kb_input = number_input(max_kb, window, cx);

        let mut subscriptions = vec![cx.subscribe_in(&quality, window, |this, _, event: &SliderEvent, _, cx| {
            if let SliderEvent::Change(value) = event {
                this.options.quality = value.end().round().clamp(1.0, 100.0) as u8;
                this.changed(cx);
            }
        })];
        for (input, field) in [
            (&long_edge_input, NumberField::LongEdge),
            (&percent_input, NumberField::Percent),
            (&max_kb_input, NumberField::MaxKb),
        ] {
            subscriptions.push(cx.subscribe_in(input, window, move |this, state, event: &InputEvent, window, cx| {
                let parsed: Option<u32> = state.read(cx).value().parse().ok();
                match event {
                    InputEvent::Change => {
                        if let Some(v) = parsed {
                            this.set_number(field, v, false);
                            this.changed(cx);
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        let v = this.set_number(field, parsed.unwrap_or(0), true);
                        state.update(cx, |s, cx| s.set_value(v.to_string(), window, cx));
                        this.changed(cx);
                    }
                    _ => {}
                }
            }));
        }

        // Dateien, die nachträglich per Kontextmenü dazukommen, und Fortschritt der Arbeit.
        let poll = cx.spawn_in(window, async move |this, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                ticks += 1;
                let alive = this.update_in(cx, |this, window, cx| {
                    if ticks % 5 == 0 {
                        if let Some(queue) = &this.queue {
                            let files = queue.drain();
                            if !files.is_empty() {
                                this.add_files(files);
                                window.activate_window();
                                cx.notify();
                            }
                        }
                    }
                    // Solange es läuft und einmal danach (Ergebnis, Hinweis) neu zeichnen.
                    let running = this.is_running();
                    if running || this.was_running {
                        cx.notify();
                    }
                    this.was_running = running;
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        Self {
            items: Vec::new(),
            saved: options.clone(),
            max_kb,
            options,
            queue,
            running: Arc::new(Mutex::new(false)),
            was_running: false,
            long_edge,
            percent,
            message: None,
            notice: Arc::default(),
            quality,
            long_edge_input,
            percent_input,
            max_kb_input,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    /// Setzt ein Zahlenfeld; `clamp` begrenzt auf den erlaubten Bereich. Gibt den Wert zurück.
    fn set_number(&mut self, field: NumberField, value: u32, clamp: bool) -> u32 {
        let (min, max) = field.range();
        let value = if clamp { value.clamp(min, max) } else { value };
        if !clamp && !(min..=max).contains(&value) {
            return value;
        }
        match field {
            NumberField::LongEdge => {
                self.long_edge = value;
                if matches!(self.options.resize, Resize::LongEdge(_)) {
                    self.options.resize = Resize::LongEdge(value);
                }
            }
            NumberField::Percent => {
                self.percent = value;
                if matches!(self.options.resize, Resize::Percent(_)) {
                    self.options.resize = Resize::Percent(value);
                }
            }
            NumberField::MaxKb => {
                self.max_kb = value;
                if self.options.max_kb.is_some() {
                    self.options.max_kb = Some(value);
                }
            }
        }
        value
    }

    /// Einstellungen speichern und neu zeichnen.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.options != self.saved {
            if let Ok(json) = serde_json::to_string_pretty(&self.options) {
                let _ = fs::create_dir_all(crate::data_dir());
                let _ = fs::write(options_path(), json);
            }
            self.saved = self.options.clone();
        }
        cx.notify();
    }

    /// Nach einer Vorlage die Bedienelemente auf die neuen Werte setzen.
    fn apply_preset(&mut self, options: Options, window: &mut Window, cx: &mut Context<Self>) {
        if let Resize::LongEdge(v) = options.resize {
            self.long_edge = v;
        }
        if let Resize::Percent(v) = options.resize {
            self.percent = v;
        }
        if let Some(v) = options.max_kb {
            self.max_kb = v;
        }
        self.options = options;
        let quality = self.options.quality as f32;
        self.quality.update(cx, |s, cx| s.set_value(quality, window, cx));
        for (input, value) in [
            (self.long_edge_input.clone(), self.long_edge),
            (self.percent_input.clone(), self.percent),
            (self.max_kb_input.clone(), self.max_kb),
        ] {
            input.update(cx, |s, cx| s.set_value(value.to_string(), window, cx));
        }
        self.changed(cx);
    }

    fn add_files(&mut self, files: Vec<PathBuf>) {
        for path in files {
            if path.is_dir() {
                // Ordner: die Bilder darin (nicht rekursiv) übernehmen.
                if let Ok(dir) = fs::read_dir(&path) {
                    let mut inner: Vec<PathBuf> = dir.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
                    inner.sort();
                    self.add_files(inner.into_iter().filter(|p| is_image(p)).collect());
                }
                continue;
            }
            if !is_image(&path) {
                self.message = Some(format!("{} ist kein unterstütztes Bild.", file_name(&path)));
                continue;
            }
            if self.items.iter().any(|i| i.path == path) {
                continue;
            }
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.items.push(Item {
                path,
                size,
                status: Arc::new(Mutex::new(Status::Ready)),
                progress: Arc::default(),
            });
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let jobs: Vec<(PathBuf, Arc<Mutex<Status>>, Arc<AtomicU32>)> = self
            .items
            .iter()
            .map(|i| {
                *i.status.lock().unwrap() = Status::Ready;
                i.progress.store(0, Ordering::Relaxed);
                (i.path.clone(), i.status.clone(), i.progress.clone())
            })
            .collect();
        let options = self.options.clone();
        let running = self.running.clone();
        let notice = self.notice.clone();
        *running.lock().unwrap() = true;
        *notice.lock().unwrap() = None;
        self.message = None;
        cx.notify();
        // Das Fenster fragt Fortschritt und Status alle 100 ms ab, solange es läuft.
        thread::spawn(move || {
            jobs.par_iter().for_each(|(path, status, progress)| {
                *status.lock().unwrap() = Status::Working;
                let on_progress = |p: f32| progress.store((p * 1000.0) as u32, Ordering::Relaxed);
                let result = match convert_file(path, &options, &on_progress) {
                    Ok(outcome) => Status::Done(outcome),
                    Err(e) => {
                        crate::log_error(&e);
                        Status::Failed(e)
                    }
                };
                *status.lock().unwrap() = result;
                progress.store(1000, Ordering::Relaxed);
            });

            let written: Vec<PathBuf> = jobs
                .iter()
                .filter_map(|(_, status, _)| match &*status.lock().unwrap() {
                    Status::Done(Outcome::Written { path, .. }) => Some(path.clone()),
                    _ => None,
                })
                .collect();
            if !written.is_empty() {
                *notice.lock().unwrap() = Some(match copy_to_clipboard(&written) {
                    Ok(()) if written.len() == 1 => "Die neue Datei liegt in der Zwischenablage (Strg+V zum Einfügen).".into(),
                    Ok(()) => format!("Die {} neuen Dateien liegen in der Zwischenablage (Strg+V zum Einfügen).", written.len()),
                    Err(e) => e,
                });
            }
            *running.lock().unwrap() = false;
        });
    }

    fn written_files(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter_map(|i| match &*i.status.lock().unwrap() {
                Status::Done(Outcome::Written { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
enum NumberField {
    LongEdge,
    Percent,
    MaxKb,
}

impl NumberField {
    fn range(self) -> (u32, u32) {
        match self {
            NumberField::LongEdge => (16, 20000),
            NumberField::Percent => (1, 400),
            NumberField::MaxKb => (10, 100_000),
        }
    }
}

impl Render for ConverterApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.is_running();
        div()
            .id("konverter")
            .size_full()
            .bg(CONTENT_BG)
            .font_family(ui_kit::FONT)
            .text_size(px(BODY_SIZE))
            .text_color(TEXT)
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                if !this.is_running() {
                    this.add_files(paths.paths().to_vec());
                    cx.notify();
                }
            }))
            .child(
                v_flex()
                    .id("seite")
                    .size_full()
                    .overflow_y_scroll()
                    .px(px(24.0))
                    .py(px(16.0))
                    .child(page_header(
                        "Bilder konvertieren & verkleinern",
                        "Die Originale bleiben unverändert, neue Dateien landen daneben.",
                    ))
                    .child(self.file_list(running, cx))
                    .child(self.settings(running, cx))
                    .child(self.actions(running, cx))
                    .when_some(self.message.clone(), |el, message| el.child(text(message, BODY_SIZE, RED).mt(px(8.0)))),
            )
    }
}

impl ConverterApp {
    fn file_list(&self, running: bool, cx: &mut Context<Self>) -> AnyElement {
        let frame = v_flex()
            .id("dateien")
            .w_full()
            .mb(px(18.0))
            .p(px(14.0))
            .rounded(px(10.0))
            .bg(ui_kit::GROUP_BG)
            .border_2()
            .border_color(gpui_kit::transparent_black())
            .drag_over::<ExternalPaths>(|style, _, _, _| style.border_color(ACCENT));

        if self.items.is_empty() {
            return frame
                .py(px(36.0))
                .items_center()
                .gap(px(4.0))
                .child(text("Bilder oder Ordner hierher ziehen", 17.0, TEXT))
                .child(text("PNG, JPG, WebP, BMP, GIF, TIFF, ICO, HEIC, AVIF", SMALL_SIZE, TEXT_WEAK))
                .into_any_element();
        }

        let rows = self.items.iter().enumerate().map(|(index, item)| {
            let status: AnyElement = match &*item.status.lock().unwrap() {
                Status::Ready => text(human_size(item.size), BODY_SIZE, TEXT_WEAK).into_any_element(),
                Status::Working => {
                    let value = item.progress.load(Ordering::Relaxed) as f32 / 10.0;
                    h_flex()
                        .gap(px(8.0))
                        .child(div().w(px(160.0)).child(Progress::new(("fortschritt", index)).value(value)))
                        .child(text(format!("{value:.0} %"), SMALL_SIZE, TEXT_WEAK))
                        .into_any_element()
                }
                Status::Done(outcome @ Outcome::Written { .. }) => text(describe(outcome), BODY_SIZE, GREEN).into_any_element(),
                Status::Done(outcome) => text(describe(outcome), BODY_SIZE, TEXT_WEAK).into_any_element(),
                Status::Failed(e) => {
                    let short = e.split_once(": ").map(|(_, m)| m).unwrap_or(e).to_string();
                    text(short, BODY_SIZE, RED).into_any_element()
                }
            };
            let full_path = item.path.display().to_string();
            h_flex()
                .id(("datei", index))
                .w_full()
                .min_h(px(30.0))
                .px(px(6.0))
                .gap(px(12.0))
                .rounded(px(6.0))
                .when(index % 2 == 1, |el| el.bg(ui_kit::tint(TEXT, 0.03)))
                .child(
                    div()
                        .id(("name", index))
                        .w(px(220.0))
                        .flex_none()
                        .truncate()
                        .child(file_name(&item.path))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(full_path.clone()).build(window, cx)
                        }),
                )
                .child(div().flex_1().min_w_0().child(status))
                .child(
                    button(("entfernen", index), "Entfernen")
                        .small()
                        .disabled(running)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if index < this.items.len() {
                                this.items.remove(index);
                            }
                            cx.notify();
                        })),
                )
        });

        frame
            .child(v_flex().id("liste").max_h(px(220.0)).overflow_y_scroll().children(rows))
            .child(
                h_flex()
                    .mt(px(8.0))
                    .gap(px(10.0))
                    .child(text(
                        format!("{} Dateien – weitere hierher ziehen", self.items.len()),
                        SMALL_SIZE,
                        TEXT_WEAK,
                    ))
                    .child(
                        button("leeren", "Liste leeren")
                            .small()
                            .disabled(running)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.items.clear();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    fn settings(&self, running: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let format = self.options.format;
        // Mit „Wie Original“ kann das Ziel JPG/WebP sein, Qualität also einstellbar lassen.
        let lossy = format.is_none_or(Format::lossy);

        let preset = |id: &'static str, label: &'static str, hint: &'static str, options: fn() -> Options| {
            button(id, label)
                .small()
                .tooltip(hint)
                .disabled(running)
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.apply_preset(options(), window, cx)))
        };
        let presets = h_flex()
            .flex_wrap()
            .gap(px(6.0))
            .child(preset("email", "Für E-Mail", "JPG, max. 1600 px, Qualität 75", || Options {
                format: Some(Format::Jpg),
                quality: 75,
                resize: Resize::LongEdge(1600),
                max_kb: None,
            }))
            .child(preset("web", "Für Webseiten", "WebP, max. 1920 px, Qualität 80", || Options {
                format: Some(Format::Webp),
                quality: 80,
                resize: Resize::LongEdge(1920),
                max_kb: None,
            }))
            .child(preset("1mb", "Unter 1 MB", "JPG, höchstens 1 MB pro Bild", || Options {
                format: Some(Format::Jpg),
                quality: 90,
                resize: Resize::None,
                max_kb: Some(1000),
            }))
            .child(preset("verkleinern", "Nur verkleinern", "Format behalten, sparsamer speichern", Options::shrink));

        let format_menu = button("format", format.map(Format::label).unwrap_or("Wie Original"))
            .dropdown_caret(true)
            .disabled(running)
            .dropdown_menu({
                let view = cx.entity().downgrade();
                move |mut menu, _, _| {
                    let choices = std::iter::once((None, "Wie Original")).chain(Format::ALL.iter().map(|f| (Some(*f), f.label())));
                    for (value, label) in choices {
                        let view = view.clone();
                        menu = menu.item(PopupMenuItem::new(label).checked(value == format).on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.options.format = value;
                                this.changed(cx);
                            });
                        }));
                    }
                    menu
                }
            });

        let resize_kind = match self.options.resize {
            Resize::None => 0,
            Resize::LongEdge(_) => 1,
            Resize::Percent(_) => 2,
        };
        const RESIZE_LABELS: [&str; 3] = ["Nicht ändern", "Längste Seite höchstens", "Prozent"];
        let resize_menu = button("groesse", RESIZE_LABELS[resize_kind])
            .dropdown_caret(true)
            .disabled(running)
            .dropdown_menu({
                let view = cx.entity().downgrade();
                move |mut menu, _, _| {
                    for (i, label) in RESIZE_LABELS.iter().enumerate() {
                        let view = view.clone();
                        menu = menu.item(PopupMenuItem::new(*label).checked(i == resize_kind).on_click(move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.options.resize = match i {
                                    1 => Resize::LongEdge(this.long_edge),
                                    2 => Resize::Percent(this.percent),
                                    _ => Resize::None,
                                };
                                this.changed(cx);
                            });
                        }));
                    }
                    menu
                }
            });
        let resize_value = match resize_kind {
            1 => Some((self.long_edge_input.clone(), "px")),
            2 => Some((self.percent_input.clone(), "%")),
            _ => None,
        };

        let limit = self.options.max_kb.is_some();

        v_flex()
            .w_full()
            .child(h_flex().gap(px(8.0)).mb(px(10.0)).child(text("Vorlagen:", BODY_SIZE, TEXT_WEAK)).child(presets))
            .child(
                section()
                    .row(
                        "Zielformat",
                        Some("„Wie Original“: BMP/TIFF werden zu PNG, HEIC/AVIF zu JPG"),
                        format_menu,
                    )
                    .row(
                        "Qualität",
                        Some("Nur für JPG und WebP. 75–85 ist meist kaum vom Original zu unterscheiden. WebP mit 100 = verlustfrei."),
                        h_flex()
                            .gap(px(10.0))
                            .child(div().w(px(180.0)).child(Slider::new(&self.quality).disabled(!lossy || running)))
                            .child(strong(self.options.quality.to_string(), BODY_SIZE, TEXT).w(px(28.0))),
                    )
                    .row(
                        "Bildgröße",
                        None,
                        h_flex().gap(px(8.0)).child(resize_menu).when_some(resize_value, |el, (input, unit)| {
                            el.child(div().w(px(90.0)).child(Input::new(&input).small().disabled(running)))
                                .child(text(unit, BODY_SIZE, TEXT_WEAK))
                        }),
                    )
                    .row(
                        "Dateigröße begrenzen",
                        Some("Senkt Qualität und notfalls Auflösung, bis die Datei klein genug ist. Nur JPG und WebP."),
                        h_flex()
                            .gap(px(8.0))
                            .when(limit, |el| {
                                el.child(
                                    div()
                                        .w(px(90.0))
                                        .child(Input::new(&self.max_kb_input).small().disabled(running || !lossy)),
                                )
                                .child(text("KB", BODY_SIZE, TEXT_WEAK))
                            })
                            .child(
                                Switch::new("limit")
                                    .checked(limit)
                                    .disabled(running || !lossy)
                                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                                        this.options.max_kb = on.then_some(this.max_kb);
                                        this.changed(cx);
                                    })),
                            ),
                    )
                    .footnote("Metadaten wie Kamera und GPS-Position werden dabei entfernt."),
            )
    }

    fn actions(&self, running: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let written = self.written_files();
        let progress = running.then(|| {
            let total: u32 = self.items.iter().map(|i| i.progress.load(Ordering::Relaxed)).sum();
            total as f32 / (10 * self.items.len().max(1)) as f32
        });
        let summary = (!running && !written.is_empty()).then(|| {
            self.items.iter().fold((0u64, 0u64), |(b, a), item| match &*item.status.lock().unwrap() {
                Status::Done(Outcome::Written { before, after, .. }) => (b + before, a + after),
                _ => (b, a),
            })
        });
        let notice = if running { None } else { self.notice.lock().unwrap().clone() };

        v_flex()
            .w_full()
            .gap(px(8.0))
            .when_some(progress, |el, value| {
                el.child(
                    h_flex()
                        .gap(px(10.0))
                        .child(div().flex_1().child(Progress::new("gesamt").value(value)))
                        .child(text(format!("{value:.0} %"), SMALL_SIZE, TEXT_WEAK)),
                )
            })
            .child(
                h_flex()
                    .gap(px(10.0))
                    .child(
                        primary_button("start", if running { "Läuft …" } else { "Konvertieren" })
                            .min_w(px(160.0))
                            .disabled(running || self.items.is_empty())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.start(cx))),
                    )
                    .when_some(summary, |el, (before, after)| {
                        let first = written[0].clone();
                        el.child(button("explorer", "Im Explorer zeigen").on_click(move |_, _, _| {
                            // raw_arg, weil Explorer das Anführungszeichen erst nach dem Komma erwartet.
                            let _ = std::process::Command::new("explorer")
                                .raw_arg(format!("/select,\"{}\"", first.display()))
                                .spawn();
                        }))
                        .child(text(
                            format!("Gesamt: von {} auf {}", human_size(before), human_size(after)),
                            BODY_SIZE,
                            TEXT_WEAK,
                        ))
                    }),
            )
            .when_some(notice, |el, notice| el.child(text(notice, BODY_SIZE, GREEN)))
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INPUT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
