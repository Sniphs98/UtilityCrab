//! Bildeditor für Screenshots und vorhandene Bilder.
//!
//! Oben die Werkzeugleiste (Aktionen aus [`ACTIONS`], Werkzeuge aus [`tools::all`]),
//! darunter die Optionen des aktiven Werkzeugs, in der Mitte das Bild (zoom- und
//! verschiebbar), unten die Statuszeile. Bilder kommen aus der Aufnahme, dem Öffnen-Dialog,
//! per Drag & Drop, aus der Zwischenablage (Strg+V) oder dem Explorer-Kontextmenü.
//!
//! Koordinaten: [`View`] rechnet zwischen Bildpixeln und Fensterkoordinaten (logische Pixel
//! von GPUI). Die Bildfläche merkt sich beim Layout ihre Lage im Fenster.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Selectable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::annotations::{Redact, RedactMode, Style};
use super::canvas::{Canvas, IRect};
use super::document::{Document, History};
use super::icons::{self, Glyph};
use super::overlay::CaptureMode;
use super::redact::{self, Found, PATTERNS};
use super::tools::{self, Input, Opt, Tool, ToolCtx};
use super::view::View;
use super::{export, Prefs, CAPTURE_ARG, PIN_ARG};
use crate::geom::{pos2, vec2, Color32, Pos2, Rect};
use crate::settings::Settings;
use crate::ui_kit::{
    self, button, h_flex, primary_button, strong, text, v_flex, vertical_divider, Button, ButtonVariants, Disableable,
    Painter, Sizable,
    BODY_SIZE, SMALL_SIZE, TEXT, TEXT_FAINT, TEXT_WEAK,
};

pub enum Source {
    Empty,
    Clipboard,
    File(PathBuf),
}

pub fn run(source: Source) {
    ui_kit::run_app(move |cx| {
        let options = ui_kit::window_options("UtilityCrab – Bild bearbeiten", (1180.0, 780.0), (760.0, 480.0), cx);
        ui_kit::open_window(options, cx, move |window, cx| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(window, cx);
                match source {
                    Source::Empty => {}
                    Source::Clipboard => editor.paste(),
                    Source::File(path) => editor.open(&path),
                }
                editor
            });
            // Ungespeicherte Änderungen: erst nachfragen.
            let weak = editor.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                weak.update(cx, |editor, cx| editor.should_close(cx)).unwrap_or(true)
            });
            editor
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Aktionen
//
// Eine neue Aktion = ein Eintrag in ACTIONS. Knopf, Tooltip, Tastenkürzel und
// Aktivierung (z. B. nur mit Bild) ergeben sich daraus.

#[derive(Clone, Copy, PartialEq)]
pub enum Group {
    /// Ganz links: Neu, Öffnen.
    File,
    History,
    /// Zusatzfunktionen hinter den Werkzeugen.
    Extra,
    /// Rechts: Kopieren, Speichern.
    Output,
}

/// Tastenkürzel mit Strg: (mit Umschalt?, GPUI-Tastenname).
#[derive(Clone, Copy, PartialEq)]
pub struct Shortcut {
    pub shift: bool,
    pub key: &'static str,
}

const fn ctrl(key: &'static str) -> Option<Shortcut> {
    Some(Shortcut { shift: false, key })
}

const fn ctrl_shift(key: &'static str) -> Option<Shortcut> {
    Some(Shortcut { shift: true, key })
}

type Run = fn(&mut Editor, &mut Window, &mut Context<Editor>);
/// Baut das kleine Menü neben dem Knopf (Optionen der Aktion).
type Menu = fn(&Editor, PopupMenu, WeakEntity<Editor>) -> PopupMenu;

pub struct ActionDef {
    /// Dauerhafte Kennung.
    pub id: &'static str,
    pub label: &'static str,
    pub glyph: Glyph,
    pub group: Group,
    pub shortcut: Option<Shortcut>,
    pub enabled: fn(&Editor) -> bool,
    pub run: Run,
    pub menu: Option<Menu>,
}

fn always(_: &Editor) -> bool {
    true
}

fn has_image(editor: &Editor) -> bool {
    editor.history.is_some()
}

fn idle_with_image(editor: &Editor) -> bool {
    editor.history.is_some() && editor.busy.is_none()
}

pub const ACTIONS: &[ActionDef] = &[
    ActionDef {
        id: "neu",
        label: "Neuer Screenshot (Strg+N)",
        glyph: Glyph::New,
        group: Group::File,
        shortcut: ctrl("n"),
        enabled: always,
        run: |e, window, _| e.new_capture(window),
        menu: Some(Editor::capture_menu),
    },
    ActionDef {
        id: "oeffnen",
        label: "Bild öffnen … (Strg+O)",
        glyph: Glyph::Open,
        group: Group::File,
        shortcut: ctrl("o"),
        enabled: always,
        run: |e, _, _| e.open_dialog(),
        menu: None,
    },
    ActionDef {
        id: "rueckgaengig",
        label: "Rückgängig (Strg+Z)",
        glyph: Glyph::Undo,
        group: Group::History,
        shortcut: ctrl("z"),
        enabled: |e| e.history.as_ref().is_some_and(History::can_undo),
        run: |e, _, _| e.with_history(History::undo),
        menu: None,
    },
    ActionDef {
        id: "wiederholen",
        label: "Wiederholen (Strg+Y)",
        glyph: Glyph::Redo,
        group: Group::History,
        shortcut: ctrl("y"),
        enabled: |e| e.history.as_ref().is_some_and(History::can_redo),
        run: |e, _, _| e.with_history(History::redo),
        menu: None,
    },
    ActionDef {
        id: "text",
        label: "Text erkennen und kopieren (Strg+Umschalt+T)",
        glyph: Glyph::OcrText,
        group: Group::Extra,
        shortcut: ctrl_shift("t"),
        enabled: idle_with_image,
        run: |e, _, _| e.recognize_text(),
        menu: None,
    },
    ActionDef {
        id: "auto-unkenntlich",
        label: "Sensible Daten automatisch unkenntlich machen (Strg+Umschalt+U)",
        glyph: Glyph::AutoRedact,
        group: Group::Extra,
        shortcut: ctrl_shift("u"),
        enabled: idle_with_image,
        run: |e, _, _| e.auto_redact(),
        menu: Some(Editor::redact_menu),
    },
    ActionDef {
        id: "anheften",
        label: "Am Bildschirm anheften (immer im Vordergrund)",
        glyph: Glyph::Pin,
        group: Group::Extra,
        shortcut: None,
        enabled: has_image,
        run: |e, _, _| e.pin(),
        menu: None,
    },
    ActionDef {
        id: "kopieren",
        label: "In die Zwischenablage kopieren (Strg+C)",
        glyph: Glyph::Copy,
        group: Group::Output,
        shortcut: ctrl("c"),
        enabled: has_image,
        run: |e, _, _| e.copy(true),
        menu: None,
    },
    ActionDef {
        id: "speichern",
        label: "Speichern unter … (Strg+S)",
        glyph: Glyph::Save,
        group: Group::Output,
        shortcut: ctrl("s"),
        enabled: has_image,
        run: |e, _, _| e.save_dialog(),
        menu: None,
    },
];

/// Farben der Palette (wie Stifte im Snipping Tool) – beliebige weitere über den Farbwähler.
const PALETTE: [[u8; 3]; 10] = [
    [0x15, 0x15, 0x18],
    [0xFF, 0xFF, 0xFF],
    [0xE5, 0x39, 0x35],
    [0xFB, 0x8C, 0x00],
    [0xFF, 0xE0, 0x3D],
    [0x43, 0xA0, 0x47],
    [0x00, 0xAC, 0xC1],
    [0x1E, 0x88, 0xE5],
    [0x8E, 0x24, 0xAA],
    [0xD8, 0x1B, 0x60],
];

const CANVAS_BG: Color32 = Color32::from_rgb(0x17, 0x18, 0x1B);

// ---------------------------------------------------------------------------------------------

/// Ergebnisse von Arbeiten im Hintergrund (Texterkennung, Kopieren).
enum JobResult {
    Text(Result<String, String>),
    Redact(Result<Vec<Found>, String>),
    Copied { result: Result<(), String>, revision: u64, manual: bool },
}

struct Status {
    text: String,
    error: bool,
    since: Instant,
}

/// Schieberegler der Optionsleiste, gebunden an ein Werkzeug.
struct ToolSliders {
    tool: String,
    width: Entity<SliderState>,
    softness: Entity<SliderState>,
    _subscriptions: [Subscription; 2],
}

pub struct Editor {
    history: Option<History>,
    /// Datei, aus der das Bild stammt bzw. wohin zuletzt gespeichert wurde.
    file: Option<PathBuf>,
    /// Stand, der zuletzt gespeichert oder kopiert wurde (für die Nachfrage beim Schließen).
    exported_revision: u64,
    tools: Vec<Option<Box<dyn Tool>>>,
    active: usize,
    prefs: Prefs,
    prefs_dirty: bool,
    settings: Settings,
    view: Option<View>,
    /// Ansicht automatisch einpassen (bis der Benutzer selbst zoomt).
    fit: bool,
    /// Lage der Bildfläche im Fenster, beim Layout gemessen.
    area: Rc<Cell<Rect>>,
    /// Fläche, für die zuletzt eingepasst wurde.
    fitted_area: Rect,
    /// Skalierung des Bildschirms im letzten Bild; ändert sie sich (Fenster auf anderen
    /// Bildschirm gezogen), wird der Zoom so angepasst, dass das Bild in Bildschirmpixeln
    /// gleich groß bleibt.
    last_scale: f32,
    image_size: (u32, u32),
    composite: Option<Canvas>,
    composite_revision: u64,
    image: Option<Arc<RenderImage>>,
    preview: Option<(Arc<RenderImage>, Rect)>,
    /// Bilder, die die GPU wieder freigeben soll (im nächsten Bild).
    stale_images: Vec<Arc<RenderImage>>,
    has_transparency: bool,
    gesture: bool,
    last_pointer: Option<Pos2>,
    /// Verschieben mit mittlerer Maustaste oder Leertaste: letzte Mausposition.
    panning: Option<Pos2>,
    space_down: bool,
    busy: Option<&'static str>,
    jobs: Arc<Mutex<Vec<JobResult>>>,
    /// Automatisch kopieren, sobald sich nichts mehr tut.
    copy_due: Option<(u64, Instant)>,
    status: Option<Status>,
    confirm_close: bool,
    allow_close: bool,
    title: String,
    focus: FocusHandle,
    /// Textfeld für das Text-Werkzeug und die Stelle, für die es gerade offen ist.
    text_input: Entity<TextareaState>,
    text_at: Option<Pos2>,
    color_picker: Entity<ColorPickerState>,
    sliders: Option<ToolSliders>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl Editor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = Prefs::load();
        let tools = tools::all();
        let active = tools
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.id() == prefs.tool))
            .or_else(|| tools.iter().position(Option::is_some))
            .unwrap_or(0);

        let text_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .submit_on_enter(true)
                .auto_grow(1, 12)
                .placeholder("Text …")
        });
        let color_picker = cx.new(|cx| ColorPickerState::new(window, cx));
        let subscriptions = vec![
            // Enter übernimmt den Text (Umschalt+Enter = neue Zeile).
            cx.subscribe_in(&text_input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.finish_text(true, window, cx);
                }
            }),
            cx.subscribe_in(&color_picker, window, |this, _, event: &ColorPickerEvent, _, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    let rgba = Rgba::from(*color);
                    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    this.edit_style(cx, |style| style.color = [to_u8(rgba.r), to_u8(rgba.g), to_u8(rgba.b), 255]);
                }
            }),
        ];

        // Hintergrundarbeiten abholen, automatisch kopieren, Meldungen ausblenden.
        let poll = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if this.update_in(cx, |this, _, cx| this.tick(cx)).is_err() {
                break;
            }
        });

        Self {
            history: None,
            file: None,
            exported_revision: 0,
            tools,
            active,
            prefs,
            prefs_dirty: false,
            settings: Settings::load(),
            view: None,
            fit: true,
            area: Rc::new(Cell::new(Rect::NOTHING)),
            fitted_area: Rect::NOTHING,
            last_scale: 0.0,
            image_size: (0, 0),
            composite: None,
            composite_revision: u64::MAX,
            image: None,
            preview: None,
            stale_images: Vec::new(),
            has_transparency: false,
            gesture: false,
            last_pointer: None,
            panning: None,
            space_down: false,
            busy: None,
            jobs: Arc::default(),
            copy_due: None,
            status: None,
            confirm_close: false,
            allow_close: false,
            title: String::new(),
            focus: cx.focus_handle(),
            text_input,
            text_at: None,
            color_picker,
            sliders: None,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    /// Läuft alle 100 ms.
    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.finish_jobs();
        changed |= self.auto_copy();
        // Statusmeldungen nach einer Weile ausblenden.
        if let Some(status) = &self.status {
            let limit = Duration::from_secs(if status.error { 12 } else { 6 });
            if status.since.elapsed() >= limit {
                self.status = None;
                changed = true;
            }
        }
        if self.prefs_dirty && !self.gesture {
            self.prefs.save();
            self.prefs_dirty = false;
        }
        if changed || self.busy.is_some() {
            cx.notify();
        }
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            error,
            since: Instant::now(),
        });
    }

    fn report(&mut self, result: Result<(), String>) {
        if let Err(e) = result {
            crate::log_error(&e);
            self.set_status(e, true);
        }
    }

    fn with_history(&mut self, f: fn(&mut History)) {
        if let Some(history) = &mut self.history {
            f(history);
        }
    }

    fn revision(&self) -> u64 {
        self.history.as_ref().map(History::revision).unwrap_or(0)
    }

    fn dirty(&self) -> bool {
        self.history.is_some() && self.revision() != self.exported_revision
    }

    // --- Bilder laden ------------------------------------------------------------------------

    fn load_image(&mut self, image: image::RgbaImage, file: Option<PathBuf>) {
        let doc = Document::new(image);
        match &mut self.history {
            // Rückgängig machbar, falls man versehentlich ein anderes Bild öffnet.
            Some(history) => history.replace(doc),
            None => self.history = Some(History::new(doc)),
        }
        // Aufnahmen liegen nur im Zwischenordner: nicht als „Datei“ anbieten.
        self.file = file.filter(|f| !super::is_temp(f));
        self.exported_revision = self.revision();
        self.fit = true;
        self.view = None;
        self.preview = None;
    }

    pub fn open(&mut self, path: &Path) {
        match export::load(path) {
            Ok(image) => {
                self.load_image(image, Some(path.to_path_buf()));
                if self.file.is_some() {
                    self.set_status(format!("Geöffnet: {}", file_name(path)), false);
                }
            }
            Err(e) => self.report(Err(e)),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = export::pick_open() {
            self.open(&path);
        }
    }

    pub fn paste(&mut self) {
        match export::clipboard_image() {
            Ok(image) => {
                self.load_image(image, None);
                self.set_status("Bild aus der Zwischenablage eingefügt", false);
            }
            Err(e) => self.set_status(e, true),
        }
    }


    // --- Aktionen ----------------------------------------------------------------------------

    fn new_capture(&mut self, window: &mut Window) {
        let delay = self.prefs.delay.to_string();
        self.prefs.save();
        let result = super::spawn(&[CAPTURE_ARG, self.prefs.mode.id(), &delay]);
        if result.is_ok() {
            // Die Aufnahme öffnet einen neuen Editor. Ohne ungesicherte Änderungen wird dieser
            // hier geschlossen, sonst nur aus dem Weg geräumt (nicht mit auf dem Bild).
            if self.dirty() {
                window.minimize_window();
            } else {
                self.allow_close = true;
                window.remove_window();
            }
        }
        self.report(result);
    }

    fn capture_menu(&self, mut menu: PopupMenu, editor: WeakEntity<Editor>) -> PopupMenu {
        menu = menu.label("Aufnahmeart");
        for mode in CaptureMode::ALL {
            let editor = editor.clone();
            menu = menu.item(PopupMenuItem::new(mode.label()).checked(self.prefs.mode == mode).on_click(move |_, _, cx| {
                let _ = editor.update(cx, |e, cx| {
                    e.prefs.mode = mode;
                    e.prefs_dirty = true;
                    cx.notify();
                });
            }));
        }
        menu = menu.separator().label("Verzögerung");
        for (secs, label) in [(0, "Keine"), (3, "3 Sekunden"), (5, "5 Sekunden"), (10, "10 Sekunden")] {
            let editor = editor.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(self.prefs.delay == secs).on_click(move |_, _, cx| {
                let _ = editor.update(cx, |e, cx| {
                    e.prefs.delay = secs;
                    e.prefs_dirty = true;
                    cx.notify();
                });
            }));
        }
        menu
    }

    fn composite_image(&mut self) -> Option<image::RgbaImage> {
        self.ensure_composite();
        self.composite.as_ref().map(Canvas::to_image)
    }

    fn copy(&mut self, manual: bool) {
        let Some(image) = self.composite_image() else {
            return;
        };
        let revision = self.revision();
        let jobs = self.jobs.clone();
        thread::spawn(move || {
            let result = export::copy(&image);
            jobs.lock().unwrap().push(JobResult::Copied { result, revision, manual });
        });
    }

    fn save_dialog(&mut self) {
        let Some(image) = self.composite_image() else {
            return;
        };
        let suggestion = match &self.file {
            Some(file) => file.clone(),
            // Neue Screenshots: im eingestellten Screenshot-Ordner vorschlagen.
            None => {
                let dir = export::screenshot_folder(&Settings::load());
                let _ = std::fs::create_dir_all(&dir);
                dir.join(export::default_name())
            }
        };
        let Some(path) = export::pick_save(&suggestion) else {
            return;
        };
        match export::save(&image, &path) {
            Ok(()) => {
                self.exported_revision = self.revision();
                self.set_status(format!("Gespeichert: {}", path.display()), false);
                self.file = Some(path);
            }
            Err(e) => self.report(Err(e)),
        }
    }

    fn pin(&mut self) {
        let Some(image) = self.composite_image() else {
            return;
        };
        let result = super::write_temp(&image)
            .and_then(|path| super::spawn(&[PIN_ARG, &path.to_string_lossy()]));
        self.report(result);
    }


    fn recognize_text(&mut self) {
        let Some(image) = self.composite_image() else {
            return;
        };
        self.busy = Some("Text wird erkannt …");
        let jobs = self.jobs.clone();
        thread::spawn(move || {
            let result = crate::ocr::recognize(image.width(), image.height(), image.as_raw())
                .map_err(|e| format!("Texterkennung: {e}"))
                .and_then(|text| {
                    if text.trim().is_empty() {
                        return Err("Kein Text im Bild gefunden".into());
                    }
                    arboard::Clipboard::new()
                        .and_then(|mut c| c.set_text(text.clone()))
                        .map_err(|e| format!("Zwischenablage: {e}"))?;
                    Ok(text)
                });
            jobs.lock().unwrap().push(JobResult::Text(result));
        });
    }

    fn auto_redact(&mut self) {
        if self.prefs.redact_patterns.is_empty() {
            self.set_status("Im Menü neben dem Knopf mindestens eine Art von Daten auswählen.", true);
            return;
        }
        // Auf dem Ausgangsbild suchen: bereits Verpixeltes soll nicht stören.
        let Some(image) = self.history.as_ref().map(|h| h.doc.base.clone()) else {
            return;
        };
        self.busy = Some("Sensible Daten werden gesucht …");
        let enabled = self.prefs.redact_patterns.clone();
        let jobs = self.jobs.clone();
        thread::spawn(move || {
            let result = crate::ocr::recognize_words(image.width(), image.height(), image.as_raw())
                .map(|lines| redact::find(&lines, &enabled))
                .map_err(|e| format!("Texterkennung: {e}"));
            jobs.lock().unwrap().push(JobResult::Redact(result));
        });
    }

    fn redact_menu(&self, mut menu: PopupMenu, editor: WeakEntity<Editor>) -> PopupMenu {
        menu = menu.label("Unkenntlich machen");
        for pattern in PATTERNS {
            let on = self.prefs.redact_patterns.iter().any(|p| p == pattern.id);
            let editor = editor.clone();
            menu = menu.item(PopupMenuItem::new(pattern.label).checked(on).on_click(move |_, _, cx| {
                let _ = editor.update(cx, |e, cx| {
                    e.prefs.redact_patterns.retain(|p| p != pattern.id);
                    if !on {
                        e.prefs.redact_patterns.push(pattern.id.to_string());
                    }
                    e.prefs_dirty = true;
                    cx.notify();
                });
            }));
        }
        menu.separator().label("Art und Stärke wie beim Werkzeug „Unkenntlich machen“.")
    }

    fn finish_jobs(&mut self) -> bool {
        let results: Vec<JobResult> = std::mem::take(&mut *self.jobs.lock().unwrap());
        let changed = !results.is_empty();
        for result in results {
            match result {
                JobResult::Text(result) => {
                    self.busy = None;
                    match result {
                        Ok(text) => {
                            let chars = text.chars().filter(|c| !c.is_whitespace()).count();
                            let first = text.lines().next().unwrap_or("").chars().take(60).collect::<String>();
                            self.set_status(format!("Text kopiert ({chars} Zeichen): „{first} …“"), false);
                        }
                        Err(e) => self.set_status(e, true),
                    }
                }
                JobResult::Redact(result) => {
                    self.busy = None;
                    match result {
                        Ok(found) if found.is_empty() => {
                            self.set_status("Keine sensiblen Daten gefunden.", false)
                        }
                        Ok(found) => self.apply_redactions(found),
                        Err(e) => self.set_status(e, true),
                    }
                }
                JobResult::Copied { result, revision, manual } => match result {
                    Ok(()) => {
                        self.exported_revision = self.exported_revision.max(revision);
                        if manual {
                            self.set_status("In die Zwischenablage kopiert", false);
                        }
                    }
                    Err(e) => self.report(Err(e)),
                },
            }
        }
        changed
    }

    fn apply_redactions(&mut self, found: Vec<Found>) {
        let style = self.style_of("unkenntlich");
        let Some(history) = &mut self.history else {
            return;
        };
        let bounds = history.doc.image_rect();
        let mut counts: Vec<(&str, usize)> = Vec::new();
        history.edit(|doc| {
            for f in &found {
                doc.items.push(Box::new(Redact {
                    rect: f.rect.intersect(bounds),
                    mode: style.redact,
                    strength: style.width,
                    soft: style.soft(),
                }));
            }
        });
        for f in &found {
            match counts.iter_mut().find(|(p, _)| *p == f.pattern) {
                Some((_, n)) => *n += 1,
                None => counts.push((f.pattern, 1)),
            }
        }
        let summary: Vec<String> = counts.iter().map(|(p, n)| format!("{n}× {p}")).collect();
        self.set_status(
            format!("{} Stellen unkenntlich gemacht: {} · Strg+Z macht es rückgängig", found.len(), summary.join(", ")),
            false,
        );
    }


    fn style_of(&self, tool_id: &str) -> Style {
        self.prefs.styles.get(tool_id).copied().unwrap_or_else(|| {
            self.tools
                .iter()
                .flatten()
                .find(|t| t.id() == tool_id)
                .map(|t| t.default_style())
                .unwrap_or_default()
        })
    }

    // --- Werkzeuge ---------------------------------------------------------------------------

    /// Ruft `f` mit dem aktiven Werkzeug und seinem Kontext auf.
    fn with_tool<R>(&mut self, shift: bool, f: impl FnOnce(&mut dyn Tool, &mut ToolCtx) -> R) -> Option<R> {
        let view = self.view?;
        let history = self.history.as_mut()?;
        let tool = self.tools.get_mut(self.active)?.as_mut()?;
        let style = self
            .prefs
            .styles
            .entry(tool.id().to_string())
            .or_insert_with(|| tool.default_style());
        let mut cx = ToolCtx {
            history,
            style,
            view,
            shift,
        };
        Some(f(tool.as_mut(), &mut cx))
    }

    fn select_tool(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active {
            return;
        }
        self.finish_text(true, window, cx);
        self.with_tool(false, |tool, cx| tool.deactivate(cx));
        self.active = index;
        self.gesture = false;
        if let Some(Some(tool)) = self.tools.get(index) {
            self.prefs.tool = tool.id().to_string();
            self.prefs_dirty = true;
        }
        cx.notify();
    }

    fn active_tool(&self) -> Option<&dyn Tool> {
        self.tools.get(self.active)?.as_deref()
    }

    /// Stil des aktiven Werkzeugs (gemerkt oder Vorgabe).
    fn current_style(&self) -> Style {
        self.active_tool().map(|t| self.style_of(t.id())).unwrap_or_default()
    }

    /// Stil des aktiven Werkzeugs ändern und merken.
    fn edit_style(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Style)) {
        let Some(tool) = self.tools.get(self.active).and_then(|t| t.as_ref()) else {
            return;
        };
        let style = self
            .prefs
            .styles
            .entry(tool.id().to_string())
            .or_insert_with(|| tool.default_style());
        let before = *style;
        change(style);
        if *style != before {
            self.prefs_dirty = true;
            cx.notify();
        }
    }

    /// Textfeld des Text-Werkzeugs schließen: übernehmen oder verwerfen.
    fn finish_text(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_at.take().is_none() {
            return;
        }
        let text = commit.then(|| self.text_input.read(cx).value().to_string());
        self.with_tool(false, |tool, tcx| tool.text_done(text, tcx));
        self.text_input.update(cx, |s, cx| s.set_value("", window, cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Schieberegler für Stärke und Weichheit passend zum aktiven Werkzeug.
    fn ensure_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.active_tool() else {
            return;
        };
        let id = tool.id().to_string();
        if self.sliders.as_ref().is_some_and(|s| s.tool == id) {
            return;
        }
        let (min, max) = tool
            .options()
            .iter()
            .find_map(|o| match o {
                Opt::Width { min, max, .. } => Some((*min, *max)),
                _ => None,
            })
            .unwrap_or((1.0, 30.0));
        let style = self.style_of(&id);
        let width = cx.new(|_| {
            SliderState::new()
                .min(min)
                .max(max)
                .step(1.0)
                .default_value(style.width.clamp(min, max))
        });
        let softness = cx.new(|_| {
            SliderState::new()
                .min(2.0)
                .max(80.0)
                .step(1.0)
                .default_value(style.softness.clamp(2.0, 80.0))
        });
        let subscriptions = [
            cx.subscribe_in(&width, window, |this, _, event: &SliderEvent, _, cx| {
                if let SliderEvent::Change(v) = event {
                    let v = v.end().round();
                    this.edit_style(cx, |s| s.width = v);
                }
            }),
            cx.subscribe_in(&softness, window, |this, _, event: &SliderEvent, _, cx| {
                if let SliderEvent::Change(v) = event {
                    let v = v.end().round();
                    this.edit_style(cx, |s| s.softness = v);
                }
            }),
        ];
        // Farbwähler zeigt die Farbe des Werkzeugs.
        let [r, g, b, _] = style.color;
        let color: Hsla = Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
        .into();
        self.color_picker.update(cx, |s, cx| s.set_value(color, window, cx));
        self.sliders = Some(ToolSliders {
            tool: id,
            width,
            softness,
            _subscriptions: subscriptions,
        });
    }

    // --- Rendern -----------------------------------------------------------------------------

    fn ensure_composite(&mut self) {
        let Some(history) = &self.history else {
            self.composite = None;
            return;
        };
        if self.composite_revision != history.revision() || self.composite.is_none() {
            self.composite = Some(history.doc.compose());
            self.composite_revision = history.revision();
            if let Some(old) = self.image.take() {
                self.stale_images.push(old);
            }
        }
    }

    fn update_images(&mut self) {
        self.ensure_composite();
        let Some(composite) = &self.composite else {
            if let Some(old) = self.image.take() {
                self.stale_images.push(old);
            }
            return;
        };
        let size = (composite.pixmap.width(), composite.pixmap.height());
        if size != self.image_size {
            self.image_size = size;
            self.fit = true;
            self.view = None;
        }
        if self.image.is_none() {
            self.has_transparency = composite.pixmap.pixels().iter().any(|p| p.alpha() < 255);
            self.image = Some(canvas_image(composite));
        }

        // Live-Vorschau: nur der Bereich der entstehenden Anmerkung wird neu gerendert.
        if let Some((old, _)) = self.preview.take() {
            self.stale_images.push(old);
        }
        let Some(item) = self.tools.get(self.active).and_then(|t| t.as_ref()).and_then(|t| t.preview()) else {
            return;
        };
        // Ein neues Spotlight ändert die gemeinsame Abdunklung aller Spotlights – dafür
        // das ganze Bild mit ihm zusammen neu zusammensetzen.
        if item.as_spotlight().is_some() {
            if let Some(history) = &self.history {
                let mut doc = history.doc.clone();
                doc.items.push(item.box_clone());
                let canvas = doc.compose();
                let area = canvas.area().to_rect();
                self.preview = Some((canvas_image(&canvas), area));
            }
            return;
        }
        let bounds = item.bounds().intersect(Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32)));
        if !bounds.is_positive() {
            return;
        }
        let region = IRect::around(bounds);
        if let Some(mut canvas) = composite.crop(region) {
            item.render(&mut canvas);
            let area = canvas.area().to_rect();
            self.preview = Some((canvas_image(&canvas), area));
        }
    }

    /// Ansicht einpassen bzw. an eine neue Bildschirmskalierung anpassen.
    fn update_view(&mut self, window: &Window) {
        let area = self.area.get();
        let scale_factor = window.scale_factor();
        if self.last_scale > 0.0 && (scale_factor - self.last_scale).abs() > 0.001 {
            if let Some(view) = &mut self.view {
                let ratio = self.last_scale / scale_factor;
                let anchor = area.min;
                view.scale *= ratio;
                view.offset = anchor + (view.offset - anchor) * ratio;
            }
        }
        self.last_scale = scale_factor;
        if !area.is_positive() || self.history.is_none() {
            return;
        }
        let (w, h) = self.image_size;
        let size = vec2(w as f32, h as f32);
        if self.view.is_none() || (self.fit && area != self.fitted_area) {
            let scale = (1.0 / scale_factor)
                .min((area.width() - 48.0) / size.x)
                .min((area.height() - 48.0) / size.y)
                .max(0.02);
            self.view = Some(View {
                offset: area.center() - size * scale / 2.0,
                scale,
            });
            self.fitted_area = area;
        }
    }

    fn zoom_to(&mut self, scale: f32, anchor: Option<Pos2>) {
        let Some(view) = &mut self.view else {
            return;
        };
        let scale = scale.clamp(0.02, 32.0);
        let anchor = anchor.unwrap_or_else(|| self.area.get().center());
        let image_point = view.to_image(anchor);
        view.scale = scale;
        view.offset = anchor - image_point.to_vec2() * scale;
        self.fit = false;
    }

    fn fit_view(&mut self) {
        self.fit = true;
        self.view = None;
    }

    /// Zeichnet Bild, Vorschau und Werkzeug-Hilfslinien (Zeichenphase der Bildfläche).
    fn paint_canvas(&self, window: &mut Window) {
        let area = self.area.get();
        let Some(view) = self.view else {
            return;
        };
        let (w, h) = self.image_size;
        let image_rect = view.rect_to_screen(Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32)));
        window.with_content_mask(Some(ContentMask { bounds: ui_kit::to_bounds(area) }), |window| {
            let mut p = Painter::new(window);
            // Schatten und Karo-Muster für durchsichtige Bereiche.
            p.rect_filled(image_rect.translate(vec2(0.0, 3.0)).expand(2.0), 3.0, Color32::from_black_alpha(60));
            if self.has_transparency {
                checkerboard(&mut p, image_rect.intersect(area));
            }
            if let Some(image) = &self.image {
                p.image(area, image_rect, image);
            }
            if let Some((image, region)) = &self.preview {
                p.image(area, view.rect_to_screen(*region), image);
            }
            if let (Some(tool), Some(history)) = (self.active_tool(), &self.history) {
                tool.paint_overlay(&mut p, view, &history.doc);
            }
        });
    }

    // --- Eingabe -----------------------------------------------------------------------------

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let p = ui_kit::from_point(event.position);
        window.focus(&self.focus, cx);
        if event.button == MouseButton::Middle || (event.button == MouseButton::Left && self.space_down) {
            self.panning = Some(p);
            return;
        }
        if event.button != MouseButton::Left || self.history.is_none() {
            return;
        }
        // Klick neben das Textfeld übernimmt den Text (und setzt kein neues).
        if self.text_at.is_some() {
            self.finish_text(true, window, cx);
            return;
        }
        let Some(view) = self.view else {
            return;
        };
        self.gesture = true;
        self.last_pointer = Some(p);
        let shift = event.modifiers.shift;
        self.with_tool(shift, |tool, tcx| tool.pointer(Input::Press(view.to_image(p)), tcx));
        self.after_tool_input(window, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let p = ui_kit::from_point(event.position);
        if let Some(last) = self.panning {
            if let Some(view) = &mut self.view {
                view.offset += p - last;
            }
            self.panning = Some(p);
            self.fit = false;
            cx.notify();
            return;
        }
        if !self.gesture || Some(p) == self.last_pointer {
            return;
        }
        let Some(view) = self.view else {
            return;
        };
        self.last_pointer = Some(p);
        let shift = event.modifiers.shift;
        self.with_tool(shift, |tool, tcx| tool.pointer(Input::Drag(view.to_image(p)), tcx));
        self.after_tool_input(window, cx);
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.panning.take().is_some() {
            return;
        }
        if !self.gesture || event.button != MouseButton::Left {
            return;
        }
        self.gesture = false;
        let Some(view) = self.view else {
            return;
        };
        let p = ui_kit::from_point(event.position);
        let shift = event.modifiers.shift;
        self.with_tool(shift, |tool, tcx| tool.pointer(Input::Release(view.to_image(p)), tcx));
        self.after_tool_input(window, cx);
    }

    /// Nach einer Eingabe: ggf. das Textfeld des Text-Werkzeugs öffnen.
    fn after_tool_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self.active_tool().and_then(|t| t.text_entry());
        if entry.is_some() && self.text_at.is_none() {
            self.text_at = entry;
            self.text_input.update(cx, |s, cx| {
                s.set_value("", window, cx);
                s.focus(window, cx);
            });
        }
        cx.notify();
    }

    fn scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = ui_kit::from_point(event.delta.pixel_delta(px(20.0))).to_vec2();
        if event.modifiers.control {
            // Zoomen mit Strg+Mausrad.
            if let Some(view) = self.view {
                let factor = (1.0 + delta.y * 0.0025).clamp(0.5, 2.0);
                self.zoom_to(view.scale * factor, Some(ui_kit::from_point(event.position)));
            }
        } else if let Some(view) = &mut self.view {
            view.offset += delta;
            self.fit = false;
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let m = event.keystroke.modifiers;
        if window.focused_input(cx).is_some() {
            return;
        }
        if key == "space" && !m.modified() {
            self.space_down = true;
            return;
        }

        // Zuerst darf das Werkzeug Tasten nutzen (Entf, Enter, Esc).
        if !m.modified() && matches!(key, "delete" | "backspace" | "enter" | "escape") {
            let used = self.with_tool(false, |tool, tcx| tool.key(key, tcx)).unwrap_or(false);
            if used {
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        if self.active_tool().is_some_and(|t| t.typing()) {
            return;
        }

        if m.control && !m.alt {
            if m.shift && key == "z" {
                self.with_history(History::redo);
            } else if let Some(action) = ACTIONS
                .iter()
                .find(|a| a.shortcut.is_some_and(|s| s.shift == m.shift && s.key == key))
            {
                if (action.enabled)(self) {
                    (action.run)(self, window, cx);
                }
            } else if !m.shift && key == "v" {
                self.paste();
            } else if key == "0" {
                self.fit_view();
            } else if key == "1" {
                self.zoom_to(1.0 / window.scale_factor(), None);
            } else if matches!(key, "+" | "=") {
                if let Some(view) = self.view {
                    self.zoom_to(view.scale * 1.25, None);
                }
            } else if key == "-" {
                if let Some(view) = self.view {
                    self.zoom_to(view.scale * 0.8, None);
                }
            } else {
                return;
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // Werkzeuge mit einzelnen Buchstaben.
        if !m.modified() && self.history.is_some() {
            let index = self
                .tools
                .iter()
                .position(|t| t.as_ref().and_then(|t| t.shortcut()) == Some(key));
            if let Some(index) = index {
                self.select_tool(index, window, cx);
            }
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.space_down = false;
            cx.notify();
        }
    }

    fn run_action(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(action) = ACTIONS.iter().find(|a| a.id == id) {
            if (action.enabled)(self) {
                (action.run)(self, window, cx);
            }
        }
        cx.notify();
    }

    // --- Schließen ---------------------------------------------------------------------------

    /// Darf das Fenster zu? Bei ungesicherten Änderungen erst nachfragen.
    fn should_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.allow_close || !self.dirty() {
            self.prefs.save();
            return true;
        }
        self.confirm_close = true;
        cx.notify();
        false
    }

    fn close_choice(&mut self, choice: u8, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_close = false;
        match choice {
            0 => {
                self.save_dialog();
                if !self.dirty() {
                    self.allow_close = true;
                    self.prefs.save();
                    window.remove_window();
                }
            }
            1 => {
                self.allow_close = true;
                self.prefs.save();
                window.remove_window();
            }
            _ => {}
        }
        cx.notify();
    }

    fn auto_copy(&mut self) -> bool {
        // Wie im Snipping Tool: Änderungen landen von selbst in der Zwischenablage.
        if !self.settings.snip_copy || self.history.is_none() || self.gesture {
            self.copy_due = None;
            return false;
        }
        let revision = self.revision();
        if revision == self.exported_revision || revision == 0 {
            return false;
        }
        match self.copy_due {
            Some((r, due)) if r == revision => {
                if Instant::now() >= due {
                    self.copy_due = None;
                    self.copy(false);
                }
            }
            _ => self.copy_due = Some((revision, Instant::now() + Duration::from_millis(600))),
        }
        false
    }

    fn update_title(&mut self, window: &mut Window) {
        let name = match (&self.file, &self.history) {
            (Some(file), _) => file_name(file),
            (None, Some(_)) => "Screenshot".into(),
            (None, None) => "Bild bearbeiten".into(),
        };
        let title = format!("{}{name} – UtilityCrab", if self.dirty() { "• " } else { "" });
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.stale_images.drain(..) {
            ui_kit::drop_image(image, window);
        }
        self.update_images();
        self.update_view(window);
        self.ensure_sliders(window, cx);
        self.update_title(window);

        let body: AnyElement = if self.history.is_some() {
            self.canvas_area(cx).into_any_element()
        } else {
            self.empty_state(cx).into_any_element()
        };

        v_flex()
            .id("editor")
            .size_full()
            .bg(CANVAS_BG)
            .font_family(ui_kit::FONT)
            .text_size(px(BODY_SIZE))
            .text_color(TEXT)
            .track_focus(&self.focus)
            // Esc im Textfeld verwirft den Text, bevor das Feld die Taste bekommt.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.text_at.is_some() && event.keystroke.key == "escape" {
                    this.finish_text(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| this.key_down(event, window, cx)))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| this.key_up(event, cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().iter().find(|p| p.is_file()) {
                    this.open(&path.clone());
                    cx.notify();
                }
            }))
            .child(self.toolbar(cx))
            .child(self.options_bar(cx))
            .child(body)
            .child(self.status_bar(cx))
            .when(self.confirm_close, |el| el.child(self.close_dialog(cx)))
    }
}

impl Editor {
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.history.is_some();
        let tools: Vec<AnyElement> = self.tools.iter().enumerate().map(|(index, tool)| -> AnyElement {
            let Some(tool) = tool else {
                return vertical_divider().into_any_element();
            };
            let tooltip = match tool.shortcut() {
                Some(key) => format!("{} ({})", tool.label(), key.to_uppercase()),
                None => tool.label().to_string(),
            };
            icons::button(("werkzeug", index), tool.glyph(), index == self.active && enabled, enabled, tooltip)
                .when(enabled, |el| {
                    el.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.select_tool(index, window, cx)))
                })
                .into_any_element()
        }).collect();
        h_flex()
            .w_full()
            .flex_none()
            .bg(ui_kit::SIDEBAR_BG)
            .px(px(10.0))
            .py(px(6.0))
            .gap(px(2.0))
            .children(self.action_group(Group::File, cx))
            .child(vertical_divider())
            .children(tools)
            .child(vertical_divider())
            .children(self.action_group(Group::History, cx))
            .child(vertical_divider())
            .children(self.action_group(Group::Extra, cx))
            .child(div().flex_1())
            .children(self.action_group(Group::Output, cx))
    }

    fn action_group(&self, group: Group, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        for action in ACTIONS.iter().filter(|a| a.group == group) {
            let enabled = (action.enabled)(self);
            let id = action.id;
            out.push(
                icons::button(id, action.glyph, false, enabled, action.label)
                    .when(enabled, |el| {
                        el.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.run_action(id, window, cx)))
                    })
                    .into_any_element(),
            );
            if let Some(menu) = action.menu {
                let editor = cx.entity().downgrade();
                out.push(
                    Button::new(SharedString::from(format!("{id}-menue")))
                        .ghost()
                        .xsmall()
                        .dropdown_caret(true)
                        .dropdown_menu(move |popup, _, cx| match editor.upgrade() {
                            Some(entity) => menu(entity.read(cx), popup.min_w(px(200.0)), editor.clone()),
                            None => popup,
                        })
                        .into_any_element(),
                );
            }
        }
        out
    }

    fn options_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = h_flex()
            .w_full()
            .flex_none()
            .min_h(px(38.0))
            .bg(ui_kit::CONTENT_BG)
            .px(px(14.0))
            .py(px(4.0))
            .gap(px(8.0));
        let Some(tool) = self.active_tool() else {
            return bar;
        };
        let enabled = self.history.is_some();
        let style = self.style_of(tool.id());
        let mut row = h_flex().gap(px(8.0)).when(!enabled, |el| el.opacity(0.45));
        for option in tool.options() {
            match *option {
                Opt::Color => {
                    let swatches = PALETTE.into_iter().enumerate().map(|(i, rgb)| {
                        let color = [rgb[0], rgb[1], rgb[2], 255];
                        let selected = style.color == color;
                        let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                        div()
                            .id(("farbe", i))
                            .size(px(20.0))
                            .cursor_pointer()
                            .child(
                                ui_kit::painted(move |p, rect| {
                                    if selected {
                                        p.circle_stroke(rect.center(), 9.0, 2.0, ui_kit::ACCENT);
                                    }
                                    p.circle_filled(rect.center(), 7.0, c);
                                    p.circle_stroke(rect.center(), 7.0, 1.0, Color32::from_white_alpha(40));
                                })
                                .size_full(),
                            )
                            .when(enabled, |el| {
                                el.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.edit_style(cx, |s| s.color = color)
                                }))
                            })
                    });
                    row = row
                        .child(h_flex().gap(px(4.0)).children(swatches))
                        .child(div().ml(px(4.0)).child(ColorPicker::new(&self.color_picker).small()))
                        .child(div().w(px(10.0)));
                }
                Opt::Width { label, .. } => {
                    if let Some(sliders) = &self.sliders {
                        row = row
                            .child(text(label, BODY_SIZE, TEXT_WEAK))
                            .child(div().w(px(140.0)).child(Slider::new(&sliders.width).disabled(!enabled)))
                            .child(strong(format!("{}", style.width.round()), SMALL_SIZE, TEXT).w(px(24.0)))
                            .child(div().w(px(6.0)));
                    }
                }
                Opt::Filled(label) => {
                    row = row.child(text(label, BODY_SIZE, TEXT_WEAK)).child(
                        Switch::new("gefuellt").checked(style.filled).disabled(!enabled).on_click(cx.listener(
                            |this, on: &bool, _, cx| {
                                let on = *on;
                                this.edit_style(cx, |s| s.filled = on);
                            },
                        )),
                    );
                }
                Opt::Redact => {
                    let modes = RedactMode::ALL.into_iter().enumerate().map(|(i, mode)| {
                        Button::new(("modus", i))
                            .small()
                            .label(mode.label())
                            .selected(style.redact == mode)
                            .disabled(!enabled)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.edit_style(cx, |s| s.redact = mode)
                            }))
                    });
                    row = row.child(h_flex().gap(px(2.0)).children(modes)).child(div().w(px(6.0)));
                }
                Opt::SoftEdge => {
                    if style.redact != RedactMode::Blur {
                        continue;
                    }
                    if let Some(sliders) = &self.sliders {
                        row = row.child(self.soft_edge_popover(&style, sliders.softness.clone(), cx));
                    }
                }
            }
        }
        bar.child(row)
            .child(div().flex_1())
            .child(div().min_w_0().truncate().child(text(tool.hint(), SMALL_SIZE, TEXT_FAINT)))
    }

    fn soft_edge_popover(&self, style: &Style, softness: Entity<SliderState>, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if style.soft_edge {
            format!("Weicher Rand: {} px", style.softness.round())
        } else {
            "Harter Rand".to_string()
        };
        let editor = cx.entity().downgrade();
        Popover::new("weicher-rand")
            .trigger(
                Button::new("weicher-rand-knopf")
                    .small()
                    .label(label)
                    .dropdown_caret(true)
                    .tooltip("Runde Ecken und ein nach außen auslaufender Rand"),
            )
            .content(move |_, _, cx| {
                let on = editor
                    .upgrade()
                    .map(|e| e.read(cx).current_style().soft_edge)
                    .unwrap_or(false);
                let editor = editor.clone();
                v_flex()
                    .w(px(240.0))
                    .gap(px(10.0))
                    .p(px(4.0))
                    .child(
                        h_flex().justify_between().child(text("Weicher Rand", BODY_SIZE, TEXT)).child(
                            Switch::new("weich").checked(on).on_click(move |on: &bool, _, cx| {
                                let on = *on;
                                let _ = editor.update(cx, |this, cx| this.edit_style(cx, |s| s.soft_edge = on));
                            }),
                        ),
                    )
                    .child(text("Weichheit", SMALL_SIZE, TEXT_WEAK))
                    .child(Slider::new(&softness).disabled(!on))
            })
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let left: Option<AnyElement> = if let Some(busy) = self.busy {
            Some(
                h_flex()
                    .gap(px(6.0))
                    .child(Spinner::new().small().color(ui_kit::ACCENT.into()))
                    .child(text(busy, SMALL_SIZE, TEXT_WEAK))
                    .into_any_element(),
            )
        } else if let Some(status) = &self.status {
            let color = if status.error { ui_kit::RED } else { ui_kit::GREEN };
            Some(
                h_flex()
                    .min_w_0()
                    .gap(px(6.0))
                    .child(div().size(px(7.0)).flex_none().rounded_full().bg(color))
                    .child(div().min_w_0().truncate().child(text(status.text.clone(), SMALL_SIZE, TEXT_WEAK)))
                    .into_any_element(),
            )
        } else {
            self.file
                .as_ref()
                .map(|file| text(file.display().to_string(), SMALL_SIZE, TEXT_FAINT).truncate().into_any_element())
        };
        let right = self.view.map(|view| {
            let (w, h) = self.image_size;
            h_flex()
                .flex_none()
                .gap(px(8.0))
                .child(text(format!("{w} × {h} px"), SMALL_SIZE, TEXT_FAINT))
                .child(text(format!("{:.0} %", view.scale * self.last_scale * 100.0), SMALL_SIZE, TEXT_WEAK))
                .child(
                    button("hundert", "100 %")
                        .xsmall()
                        .tooltip("Strg+1")
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.zoom_to(1.0 / window.scale_factor(), None);
                            cx.notify();
                        })),
                )
                .child(button("einpassen", "Einpassen").xsmall().tooltip("Strg+0").on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.fit_view();
                        cx.notify();
                    },
                )))
        });
        h_flex()
            .w_full()
            .flex_none()
            .h(px(30.0))
            .bg(ui_kit::SIDEBAR_BG)
            .px(px(12.0))
            .gap(px(10.0))
            .child(div().flex_1().min_w_0().children(left))
            .children(right)
    }

    fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex_1().w_full().flex().items_center().justify_center().child(
            v_flex()
                .id("leer")
                .w(px(420.0))
                .items_center()
                .p(px(24.0))
                .gap(px(6.0))
                .rounded(px(14.0))
                .bg(ui_kit::GROUP_BG)
                .border_1()
                .border_color(ui_kit::DIVIDER)
                .drag_over::<ExternalPaths>(|style, _, _, _| style.border_2().border_color(ui_kit::ACCENT))
                .child(strong("Screenshot aufnehmen oder Bild öffnen", 17.0, TEXT).mt(px(8.0)))
                .child(text("Oder ein Bild hierher ziehen bzw. mit Strg+V einfügen.", BODY_SIZE, TEXT_WEAK))
                .child(div().h(px(16.0)))
                .child(
                    primary_button("leer-neu", "Neuer Screenshot")
                        .w(px(220.0))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, _| this.new_capture(window))),
                )
                .child(button("leer-oeffnen", "Bild öffnen …").w(px(220.0)).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.open_dialog();
                        cx.notify();
                    },
                )))
                .child(button("leer-einfuegen", "Aus Zwischenablage einfügen").w(px(220.0)).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.paste();
                        cx.notify();
                    },
                ))),
        )
    }

    fn canvas_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let area_cell = self.area.clone();
        let cursor = if self.panning.is_some() {
            CursorStyle::ClosedHand
        } else if self.space_down {
            CursorStyle::OpenHand
        } else {
            self.active_tool().map(|t| t.cursor()).unwrap_or_default()
        };
        let area = self.area.get();

        let surface = canvas(
            move |bounds, window, _| {
                let rect = ui_kit::to_rect(bounds);
                if area_cell.get() != rect {
                    area_cell.set(rect);
                    // Ansicht für die neue Größe einpassen: noch ein Bild zeichnen.
                    window.refresh();
                }
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            },
            move |_, hitbox, window, cx| {
                window.set_cursor_style(cursor, &hitbox);
                let weak = this.downgrade();
                let hb = hitbox.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && hb.is_hovered(window) {
                        let _ = weak.update(cx, |ed, cx| ed.mouse_down(e, window, cx));
                    }
                });
                let weak = this.downgrade();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = weak.update(cx, |ed, cx| ed.mouse_move(e, window, cx));
                    }
                });
                let weak = this.downgrade();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = weak.update(cx, |ed, cx| ed.mouse_up(e, window, cx));
                    }
                });
                let weak = this.downgrade();
                let hb = hitbox.clone();
                window.on_mouse_event(move |e: &ScrollEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && hb.is_hovered(window) {
                        let _ = weak.update(cx, |ed, cx| ed.scroll(e, cx));
                    }
                });
                this.read(cx).paint_canvas(window);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        // Textfeld des Text-Werkzeugs über dem Bild.
        let text_field = match (self.text_at, self.view) {
            (Some(pos), Some(view)) => {
                let style = self.style_of("text");
                let size = (style.width * view.scale).max(8.0);
                let value = self.text_input.read(cx).value();
                let longest = value.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
                let width = (size * 0.62 * (longest + 2.0)).max(size * 4.0);
                let at = view.to_screen(pos) - vec2(4.0, 4.0) - area.min.to_vec2();
                Some(
                    div()
                        .absolute()
                        .left(px(at.x))
                        .top(px(at.y))
                        .occlude()
                        .p(px(3.0))
                        .border_1()
                        .border_color(ui_kit::ACCENT)
                        .bg(Color32::from_black_alpha(40))
                        .child(
                            div()
                                .w(px(width))
                                .text_size(px(size))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(style.color())
                                .child(Textarea::new(&self.text_input).appearance(false)),
                        ),
                )
            }
            _ => None,
        };

        // Knöpfe des Werkzeugs (z. B. Zuschneiden).
        let controls = match (self.active_tool(), self.view, &self.history) {
            (Some(tool), Some(view), Some(history)) => tool.controls(view, &history.doc),
            _ => None,
        };
        let controls = controls.map(|controls| {
            let local = |p: Pos2| p - area.min.to_vec2();
            let anchor = local(controls.anchor);
            let max_top = area.height() - 44.0;
            let buttons = controls.buttons.into_iter().map(|(id, label, primary)| {
                let b = if primary { primary_button(id, label) } else { button(id, label) };
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.with_tool(false, |tool, tcx| tool.command(id, tcx));
                    cx.notify();
                }))
            });
            let label = controls.label.map(|(at, label)| {
                let at = local(at);
                div()
                    .absolute()
                    .left(px(at.x))
                    .top(px((at.y - 18.0).max(2.0)))
                    .child(text(label, SMALL_SIZE, Color32::WHITE))
            });
            div()
                .absolute()
                .size_full()
                .top_0()
                .left_0()
                .children(label)
                .child(
                    h_flex()
                        .absolute()
                        .right(px((area.width() - anchor.x).max(4.0)))
                        .top(px(anchor.y.min(max_top).max(4.0)))
                        .occlude()
                        .gap(px(6.0))
                        .p(px(6.0))
                        .rounded(px(8.0))
                        .bg(ui_kit::GROUP_BG)
                        .border_1()
                        .border_color(ui_kit::DIVIDER)
                        .children(buttons),
                )
        });

        div()
            .id("leinwand")
            .flex_1()
            .w_full()
            .relative()
            .overflow_hidden()
            .child(surface)
            .children(controls)
            .children(text_field)
    }

    fn close_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("schliessen-dialog")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .bg(Color32::from_black_alpha(120))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(360.0))
                    .p(px(20.0))
                    .gap(px(4.0))
                    .rounded(px(12.0))
                    .bg(ui_kit::GROUP_BG)
                    .border_1()
                    .border_color(ui_kit::DIVIDER)
                    .child(strong("Änderungen speichern?", 16.0, TEXT))
                    .child(text("Das bearbeitete Bild wurde noch nicht gespeichert oder kopiert.", BODY_SIZE, TEXT_WEAK))
                    .child(
                        h_flex()
                            .mt(px(14.0))
                            .gap(px(8.0))
                            .child(primary_button("dlg-speichern", "Speichern …").on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| this.close_choice(0, window, cx),
                            )))
                            .child(button("dlg-verwerfen", "Nicht speichern").on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| this.close_choice(1, window, cx),
                            )))
                            .child(button("dlg-abbrechen", "Abbrechen").on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| this.close_choice(2, window, cx),
                            ))),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------------------------

/// Leinwand (vormultipliziertes RGBA) als Bild für GPUI.
fn canvas_image(canvas: &Canvas) -> Arc<RenderImage> {
    ui_kit::render_premultiplied(canvas.pixmap.width(), canvas.pixmap.height(), canvas.pixmap.data())
}

fn checkerboard(p: &mut Painter, rect: Rect) {
    if !rect.is_positive() {
        return;
    }
    p.rect_filled(rect, 0.0, Color32::from_gray(0xCC));
    let tile = 10.0;
    let mut y = rect.min.y;
    let mut row = 0;
    while y < rect.max.y {
        let mut x = rect.min.x + if row % 2 == 0 { 0.0 } else { tile };
        while x < rect.max.x {
            let r = Rect::from_min_max(pos2(x, y), pos2((x + tile).min(rect.max.x), (y + tile).min(rect.max.y)));
            p.rect_filled(r, 0.0, Color32::from_gray(0xF2));
            x += tile * 2.0;
        }
        y += tile;
        row += 1;
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
