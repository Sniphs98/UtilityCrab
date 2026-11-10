//! Bildeditor für Screenshots und vorhandene Bilder.
//!
//! Oben die Werkzeugleiste (Aktionen aus [`ACTIONS`], Werkzeuge aus [`tools::all`]),
//! darunter die Optionen des aktiven Werkzeugs, in der Mitte das Bild (zoom- und
//! verschiebbar), unten die Statuszeile. Bilder kommen aus der Aufnahme, dem Öffnen-Dialog,
//! per Drag & Drop, aus der Zwischenablage (Strg+V) oder dem Explorer-Kontextmenü.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Color32, CornerRadius, Key, KeyboardShortcut, Layout, Modifiers, Pos2, Rect,
    RichText, Sense, Stroke, StrokeKind, Ui,
};

use super::annotations::{Redact, RedactMode, Style};
use super::canvas::{Canvas, IRect};
use super::document::{Document, History};
use super::icons::{self, Glyph};
use super::overlay::CaptureMode;
use super::redact::{self, Found, PATTERNS};
use super::tools::{self, Input, Opt, Tool, ToolCtx};
use super::view::View;
use super::{export, Prefs, CAPTURE_ARG, PIN_ARG};
use crate::settings::Settings;
use crate::ui_kit::{self, regular, semibold, BODY_SIZE, SMALL_SIZE, TEXT, TEXT_FAINT, TEXT_WEAK};

pub enum Source {
    Empty,
    Clipboard,
    File(PathBuf),
}

pub fn run(source: Source) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Bild bearbeiten")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([760.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(crate::settings_ui::window_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab-Editor",
        options,
        Box::new(move |cc| {
            ui_kit::apply(&cc.egui_ctx);
            let mut editor = Editor::new();
            match source {
                Source::Empty => {}
                Source::Clipboard => editor.paste(),
                Source::File(path) => editor.open(&path),
            }
            Ok(Box::new(editor))
        }),
    );
    if let Err(e) = result {
        crate::report(Err(format!("Bildeditor: {e}")));
    }
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

pub struct ActionDef {
    /// Dauerhafte Kennung.
    pub id: &'static str,
    pub label: &'static str,
    pub glyph: Glyph,
    pub group: Group,
    pub shortcut: Option<KeyboardShortcut>,
    pub enabled: fn(&Editor) -> bool,
    pub run: fn(&mut Editor, &egui::Context),
    /// Kleines Menü neben dem Knopf (Optionen der Aktion).
    pub menu: Option<fn(&mut Editor, &mut Ui)>,
}

const fn key(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

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
        shortcut: key(CTRL, Key::N),
        enabled: always,
        run: |e, ctx| e.new_capture(ctx),
        menu: Some(|e, ui| e.capture_menu(ui)),
    },
    ActionDef {
        id: "oeffnen",
        label: "Bild öffnen … (Strg+O)",
        glyph: Glyph::Open,
        group: Group::File,
        shortcut: key(CTRL, Key::O),
        enabled: always,
        run: |e, _| e.open_dialog(),
        menu: None,
    },
    ActionDef {
        id: "rueckgaengig",
        label: "Rückgängig (Strg+Z)",
        glyph: Glyph::Undo,
        group: Group::History,
        shortcut: key(CTRL, Key::Z),
        enabled: |e| e.history.as_ref().is_some_and(History::can_undo),
        run: |e, _| e.with_history(History::undo),
        menu: None,
    },
    ActionDef {
        id: "wiederholen",
        label: "Wiederholen (Strg+Y)",
        glyph: Glyph::Redo,
        group: Group::History,
        shortcut: key(CTRL, Key::Y),
        enabled: |e| e.history.as_ref().is_some_and(History::can_redo),
        run: |e, _| e.with_history(History::redo),
        menu: None,
    },
    ActionDef {
        id: "text",
        label: "Text erkennen und kopieren (Strg+Umschalt+T)",
        glyph: Glyph::OcrText,
        group: Group::Extra,
        shortcut: key(CTRL_SHIFT, Key::T),
        enabled: idle_with_image,
        run: |e, ctx| e.recognize_text(ctx),
        menu: None,
    },
    ActionDef {
        id: "auto-unkenntlich",
        label: "Sensible Daten automatisch unkenntlich machen (Strg+Umschalt+U)",
        glyph: Glyph::AutoRedact,
        group: Group::Extra,
        shortcut: key(CTRL_SHIFT, Key::U),
        enabled: idle_with_image,
        run: |e, ctx| e.auto_redact(ctx),
        menu: Some(|e, ui| e.redact_menu(ui)),
    },
    ActionDef {
        id: "anheften",
        label: "Am Bildschirm anheften (immer im Vordergrund)",
        glyph: Glyph::Pin,
        group: Group::Extra,
        shortcut: None,
        enabled: has_image,
        run: |e, _| e.pin(),
        menu: None,
    },
    ActionDef {
        id: "kopieren",
        label: "In die Zwischenablage kopieren (Strg+C)",
        glyph: Glyph::Copy,
        group: Group::Output,
        shortcut: key(CTRL, Key::C),
        enabled: has_image,
        run: |e, _| e.copy(true),
        menu: None,
    },
    ActionDef {
        id: "speichern",
        label: "Speichern unter … (Strg+S)",
        glyph: Glyph::Save,
        group: Group::Output,
        shortcut: key(CTRL, Key::S),
        enabled: has_image,
        run: |e, _| e.save_dialog(),
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
    /// Skalierung im letzten Bild; ändert sie sich (Fenster auf anderen Bildschirm gezogen),
    /// wird der Zoom so angepasst, dass das Bild in Bildschirmpixeln gleich groß bleibt.
    last_ppp: f32,
    image_size: (u32, u32),
    composite: Option<Canvas>,
    composite_revision: u64,
    texture: Option<egui::TextureHandle>,
    preview: Option<(egui::TextureHandle, Rect)>,
    has_transparency: bool,
    gesture: bool,
    last_pointer: Option<Pos2>,
    panning: bool,
    busy: Option<&'static str>,
    jobs: Arc<Mutex<Vec<JobResult>>>,
    /// Automatisch kopieren, sobald sich nichts mehr tut.
    copy_due: Option<(u64, Instant)>,
    status: Option<Status>,
    confirm_close: bool,
    allow_close: bool,
    title: String,
}

impl Editor {
    fn new() -> Self {
        let prefs = Prefs::load();
        let tools = tools::all();
        let active = tools
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.id() == prefs.tool))
            .or_else(|| tools.iter().position(Option::is_some))
            .unwrap_or(0);
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
            last_ppp: 0.0,
            image_size: (0, 0),
            composite: None,
            composite_revision: u64::MAX,
            texture: None,
            preview: None,
            has_transparency: false,
            gesture: false,
            last_pointer: None,
            panning: false,
            busy: None,
            jobs: Arc::default(),
            copy_due: None,
            status: None,
            confirm_close: false,
            allow_close: false,
            title: String::new(),
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

    fn new_capture(&mut self, ctx: &egui::Context) {
        let delay = self.prefs.delay.to_string();
        self.prefs.save();
        let result = super::spawn(&[CAPTURE_ARG, self.prefs.mode.id(), &delay]);
        if result.is_ok() {
            // Die Aufnahme öffnet einen neuen Editor. Ohne ungesicherte Änderungen wird dieser
            // hier geschlossen, sonst nur aus dem Weg geräumt (nicht mit auf dem Bild).
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            } else {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.report(result);
    }

    fn capture_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Aufnahmeart").font(semibold(SMALL_SIZE)).color(TEXT_WEAK));
        for mode in CaptureMode::ALL {
            if ui.radio(self.prefs.mode == mode, mode.label()).clicked() {
                self.prefs.mode = mode;
                self.prefs_dirty = true;
            }
        }
        ui.separator();
        ui.label(RichText::new("Verzögerung").font(semibold(SMALL_SIZE)).color(TEXT_WEAK));
        for (secs, label) in [(0, "Keine"), (3, "3 Sekunden"), (5, "5 Sekunden"), (10, "10 Sekunden")] {
            if ui.radio(self.prefs.delay == secs, label).clicked() {
                self.prefs.delay = secs;
                self.prefs_dirty = true;
            }
        }
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

    fn recognize_text(&mut self, ctx: &egui::Context) {
        let Some(image) = self.composite_image() else {
            return;
        };
        self.busy = Some("Text wird erkannt …");
        let jobs = self.jobs.clone();
        let ctx = ctx.clone();
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
            ctx.request_repaint();
        });
    }

    fn auto_redact(&mut self, ctx: &egui::Context) {
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
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = crate::ocr::recognize_words(image.width(), image.height(), image.as_raw())
                .map(|lines| redact::find(&lines, &enabled))
                .map_err(|e| format!("Texterkennung: {e}"));
            jobs.lock().unwrap().push(JobResult::Redact(result));
            ctx.request_repaint();
        });
    }

    fn redact_menu(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Unkenntlich machen").font(semibold(SMALL_SIZE)).color(TEXT_WEAK));
        for pattern in PATTERNS {
            let mut on = self.prefs.redact_patterns.iter().any(|p| p == pattern.id);
            if ui.checkbox(&mut on, pattern.label).changed() {
                self.prefs.redact_patterns.retain(|p| p != pattern.id);
                if on {
                    self.prefs.redact_patterns.push(pattern.id.to_string());
                }
                self.prefs_dirty = true;
            }
        }
        ui.separator();
        ui.label(
            RichText::new("Art und Stärke wie beim Werkzeug „Unkenntlich machen“.")
                .font(regular(SMALL_SIZE))
                .color(TEXT_FAINT),
        );
    }

    fn finish_jobs(&mut self) {
        let results: Vec<JobResult> = std::mem::take(&mut *self.jobs.lock().unwrap());
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
    fn with_tool<R>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut ToolCtx) -> R) -> Option<R> {
        let view = self.view?;
        let history = self.history.as_mut()?;
        let tool = self.tools.get_mut(self.active)?.as_mut()?;
        let style = self
            .prefs
            .styles
            .entry(tool.id().to_string())
            .or_insert_with(|| tool.default_style());
        let shift = false;
        let mut cx = ToolCtx {
            history,
            style,
            view,
            shift,
        };
        Some(f(tool.as_mut(), &mut cx))
    }

    fn select_tool(&mut self, index: usize) {
        if index == self.active {
            return;
        }
        self.with_tool(|tool, cx| tool.deactivate(cx));
        self.active = index;
        self.gesture = false;
        if let Some(Some(tool)) = self.tools.get(index) {
            self.prefs.tool = tool.id().to_string();
            self.prefs_dirty = true;
        }
    }

    fn active_tool(&self) -> Option<&dyn Tool> {
        self.tools.get(self.active)?.as_deref()
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
            self.texture = None;
        }
    }

    fn update_textures(&mut self, ctx: &egui::Context) {
        self.ensure_composite();
        let Some(composite) = &self.composite else {
            self.texture = None;
            return;
        };
        let size = (composite.pixmap.width(), composite.pixmap.height());
        if size != self.image_size {
            self.image_size = size;
            self.fit = true;
            self.view = None;
        }
        if self.texture.is_none() {
            self.has_transparency = composite.pixmap.pixels().iter().any(|p| p.alpha() < 255);
            self.texture = Some(canvas_texture(ctx, "bild", composite));
        }

        // Live-Vorschau: nur der Bereich der entstehenden Anmerkung wird neu gerendert.
        self.preview = None;
        let Some(item) = self.tools.get(self.active).and_then(|t| t.as_ref()).and_then(|t| t.preview()) else {
            return;
        };
        let bounds = item.bounds().intersect(Rect::from_min_size(Pos2::ZERO, vec2(size.0 as f32, size.1 as f32)));
        if !bounds.is_positive() {
            return;
        }
        let region = IRect::around(bounds);
        if let Some(mut canvas) = composite.crop(region) {
            item.render(&mut canvas);
            let area = canvas.area().to_rect();
            self.preview = Some((canvas_texture(ctx, "vorschau", &canvas), area));
        }
    }

    // --- Oberfläche --------------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            self.action_group(ui, ctx, Group::File);
            divider(ui);
            for index in 0..self.tools.len() {
                let Some(tool) = &self.tools[index] else {
                    divider(ui);
                    continue;
                };
                let tooltip = match tool.shortcut() {
                    Some(key) => format!("{} ({})", tool.label(), key.name()),
                    None => tool.label().to_string(),
                };
                let enabled = self.history.is_some();
                if icons::button(ui, tool.glyph(), index == self.active && enabled, enabled, &tooltip).clicked() {
                    self.select_tool(index);
                }
            }
            divider(ui);
            self.action_group(ui, ctx, Group::History);
            divider(ui);
            self.action_group(ui, ctx, Group::Extra);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Rechts nach links: Speichern ganz außen.
                let output: Vec<&ActionDef> = ACTIONS.iter().filter(|a| a.group == Group::Output).collect();
                for action in output.iter().rev() {
                    self.action_button(ui, ctx, action);
                }
            });
        });
    }

    fn action_group(&mut self, ui: &mut Ui, ctx: &egui::Context, group: Group) {
        for action in ACTIONS.iter().filter(|a| a.group == group) {
            self.action_button(ui, ctx, action);
        }
    }

    fn action_button(&mut self, ui: &mut Ui, ctx: &egui::Context, action: &ActionDef) {
        let enabled = (action.enabled)(self);
        if icons::button(ui, action.glyph, false, enabled, action.label).clicked() {
            (action.run)(self, ctx);
        }
        if let Some(menu) = action.menu {
            let response = ui.add(
                egui::Button::new(RichText::new("▾").font(regular(SMALL_SIZE)).color(TEXT_WEAK))
                    .frame(false)
                    .min_size(vec2(12.0, 30.0)),
            );
            egui::Popup::menu(&response).show(|ui| {
                ui.set_min_width(200.0);
                menu(self, ui);
            });
        }
    }

    fn options_bar(&mut self, ui: &mut Ui) {
        let Some(tool) = self.tools.get(self.active).and_then(|t| t.as_ref()) else {
            return;
        };
        let options = tool.options();
        let hint = tool.hint();
        let id = tool.id().to_string();
        let default = tool.default_style();
        let enabled = self.history.is_some();
        let style = self.prefs.styles.entry(id).or_insert(default);
        let before = *style;
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            ui.add_enabled_ui(enabled, |ui| {
                for option in options {
                    match *option {
                        Opt::Color => {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for rgb in PALETTE {
                                let color = [rgb[0], rgb[1], rgb[2], 255];
                                if swatch(ui, color, style.color == color).clicked() {
                                    style.color = color;
                                }
                            }
                            let mut custom = style.color();
                            if egui::color_picker::color_edit_button_srgba(ui, &mut custom, egui::color_picker::Alpha::Opaque)
                                .on_hover_text("Eigene Farbe")
                                .changed()
                            {
                                style.color = custom.to_srgba_unmultiplied();
                            }
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.add_space(10.0);
                        }
                        Opt::Width { label, min, max } => {
                            ui.label(RichText::new(label).color(TEXT_WEAK));
                            ui.add(egui::Slider::new(&mut style.width, min..=max).integer().trailing_fill(true));
                            ui.add_space(10.0);
                        }
                        Opt::Filled(label) => {
                            ui.label(RichText::new(label).color(TEXT_WEAK));
                            ui_kit::toggle(ui, &mut style.filled);
                            ui.add_space(10.0);
                        }
                        Opt::Redact => {
                            for mode in RedactMode::ALL {
                                if ui.selectable_label(style.redact == mode, mode.label()).clicked() {
                                    style.redact = mode;
                                }
                            }
                            ui.add_space(10.0);
                        }
                    }
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(hint).font(regular(SMALL_SIZE)).color(TEXT_FAINT)).truncate());
            });
        });
        if *style != before {
            self.prefs_dirty = true;
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(24.0);
            if let Some(busy) = self.busy {
                ui.add(egui::Spinner::new().size(14.0).color(ui_kit::ACCENT));
                ui.label(RichText::new(busy).font(regular(SMALL_SIZE)).color(TEXT_WEAK));
            } else if let Some(status) = &self.status {
                let color = if status.error { ui_kit::RED } else { ui_kit::GREEN };
                ui.label(RichText::new("●").font(regular(SMALL_SIZE)).color(color));
                ui.add(egui::Label::new(RichText::new(&status.text).font(regular(SMALL_SIZE)).color(TEXT_WEAK)).truncate());
            } else if let Some(file) = &self.file {
                ui.label(RichText::new(file.display().to_string()).font(regular(SMALL_SIZE)).color(TEXT_FAINT));
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let Some(view) = self.view else {
                    return;
                };
                if ui.small_button("Einpassen").on_hover_text("Strg+0").clicked() {
                    self.fit = true;
                    self.view = None;
                }
                let ppp = ui.ctx().pixels_per_point();
                if ui.small_button("100 %").on_hover_text("Strg+1").clicked() {
                    self.zoom_to(1.0 / ppp, None, ui.ctx());
                }
                ui.label(
                    RichText::new(format!("{:.0} %", view.scale * ppp * 100.0))
                        .font(regular(SMALL_SIZE))
                        .color(TEXT_WEAK),
                );
                let (w, h) = self.image_size;
                ui.label(RichText::new(format!("{w} × {h} px")).font(regular(SMALL_SIZE)).color(TEXT_FAINT));
            });
        });
    }

    fn zoom_to(&mut self, scale: f32, anchor: Option<Pos2>, ctx: &egui::Context) {
        let Some(view) = &mut self.view else {
            return;
        };
        let scale = scale.clamp(0.02, 32.0);
        let anchor = anchor.unwrap_or_else(|| ctx.content_rect().center());
        let image_point = view.to_image(anchor);
        view.scale = scale;
        view.offset = anchor - image_point.to_vec2() * scale;
        self.fit = false;
    }

    fn empty_state(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let rect = ui.max_rect();
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let card = Rect::from_center_size(rect.center(), vec2(420.0, 250.0));
        ui.painter().rect(
            card,
            CornerRadius::same(14),
            ui_kit::GROUP_BG,
            Stroke::new(if hovering { 2.0 } else { 1.0 }, if hovering { ui_kit::ACCENT } else { ui_kit::DIVIDER }),
            StrokeKind::Inside,
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(card.shrink(24.0)), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Screenshot aufnehmen oder Bild öffnen").font(semibold(17.0)).color(TEXT));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Oder ein Bild hierher ziehen bzw. mit Strg+V einfügen.")
                        .font(regular(BODY_SIZE))
                        .color(TEXT_WEAK),
                );
                ui.add_space(22.0);
                if ui.add(ui_kit::primary_button("Neuer Screenshot").min_size(vec2(220.0, 34.0))).clicked() {
                    self.new_capture(ctx);
                }
                ui.add_space(6.0);
                if ui.add(egui::Button::new("Bild öffnen …").min_size(vec2(220.0, 30.0))).clicked() {
                    self.open_dialog();
                }
                ui.add_space(6.0);
                if ui.add(egui::Button::new("Aus Zwischenablage einfügen").min_size(vec2(220.0, 30.0))).clicked() {
                    self.paste();
                }
            });
        });
    }

    fn canvas(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let area = ui.max_rect();
        let ppp = ctx.pixels_per_point();
        if self.last_ppp > 0.0 && (ppp - self.last_ppp).abs() > 0.001 {
            if let Some(view) = &mut self.view {
                let ratio = self.last_ppp / ppp;
                view.scale *= ratio;
                view.offset = (view.offset.to_vec2() * ratio).to_pos2();
            }
        }
        self.last_ppp = ppp;
        let (w, h) = self.image_size;
        let size = vec2(w as f32, h as f32);
        if self.fit || self.view.is_none() {
            let scale = (1.0 / ppp).min((area.width() - 48.0) / size.x).min((area.height() - 48.0) / size.y).max(0.02);
            self.view = Some(View {
                offset: area.center() - size * scale / 2.0,
                scale,
            });
        }
        let Some(view) = self.view else {
            return;
        };

        let response = ui.interact(area, egui::Id::unique("leinwand"), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let image_rect = view.rect_to_screen(Rect::from_min_size(Pos2::ZERO, size));

        // Schatten und Karo-Muster für durchsichtige Bereiche.
        painter.rect_filled(image_rect.translate(vec2(0.0, 3.0)).expand(2.0), CornerRadius::same(3), Color32::from_black_alpha(60));
        if self.has_transparency {
            checkerboard(&painter, image_rect.intersect(area));
        }
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        if let Some(texture) = &self.texture {
            painter.image(texture.id(), image_rect, uv, Color32::WHITE);
        }
        if let Some((texture, region)) = &self.preview {
            painter.image(texture.id(), view.rect_to_screen(*region), uv, Color32::WHITE);
        }

        // --- Eingabe ---
        let no_focus = ctx.memory(|m| m.focused().is_none());
        let (pressed, down, released, pointer, shift, middle, delta) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
                i.modifiers.shift,
                i.pointer.middle_down(),
                i.pointer.delta(),
            )
        });
        let space = no_focus && ctx.input(|i| i.key_down(Key::Space));

        // Verschieben: mittlere Maustaste oder Leertaste + Ziehen.
        if (middle || (space && down)) && (response.hovered() || self.panning) {
            self.panning = true;
            if let Some(view) = &mut self.view {
                view.offset += delta;
            }
            self.fit = false;
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            self.panning = false;
            let to_image = |p: Pos2| view.to_image(p);
            let mut event = None;
            if pressed && response.hovered() {
                if let Some(p) = pointer {
                    self.gesture = true;
                    self.last_pointer = Some(p);
                    event = Some(Input::Press(to_image(p)));
                }
            } else if self.gesture && released {
                self.gesture = false;
                let p = pointer.or(self.last_pointer).unwrap_or_default();
                event = Some(Input::Release(to_image(p)));
            } else if self.gesture && down {
                if let Some(p) = pointer.filter(|p| Some(*p) != self.last_pointer) {
                    self.last_pointer = Some(p);
                    event = Some(Input::Drag(to_image(p)));
                }
            }
            if let Some(event) = event {
                self.with_tool(|tool, cx| {
                    cx.shift = shift;
                    tool.pointer(event, cx);
                });
            }
            if response.hovered() {
                if space {
                    ctx.set_cursor_icon(egui::CursorIcon::Grab);
                } else if let Some(tool) = self.active_tool() {
                    ctx.set_cursor_icon(tool.cursor());
                }
            }
        }

        // Zoomen mit Strg+Mausrad (bzw. Touchpad-Geste), sonst scrollen.
        if response.hovered() {
            let (zoom, scroll) = ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if zoom != 1.0 {
                self.zoom_to(view.scale * zoom, pointer, ctx);
            } else if scroll != egui::Vec2::ZERO {
                if let Some(view) = &mut self.view {
                    view.offset += scroll;
                }
                self.fit = false;
            }
        }

        // Werkzeug-eigene Oberfläche (Textfeld, Zuschneiderahmen, Auswahl).
        self.with_tool(|tool, cx| tool.overlay(ui, cx));
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let no_focus = ctx.memory(|m| m.focused().is_none());
        let typing = self.active_tool().is_some_and(|t| t.typing());

        // Zuerst darf das Werkzeug Tasten nutzen (Entf, Enter, Esc).
        for key in [Key::Delete, Key::Backspace, Key::Enter, Key::Escape] {
            if no_focus && ctx.input(|i| i.key_pressed(key) && i.modifiers.is_none()) {
                let used = self.with_tool(|tool, cx| tool.key(key, cx)).unwrap_or(false);
                if used {
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
                }
            }
        }
        if !no_focus || typing {
            return;
        }

        // Aktionen; Varianten mit Umschalt zuerst, damit Strg+S nicht Strg+Umschalt+S schluckt.
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(CTRL_SHIFT, Key::Z))) {
            self.with_history(History::redo);
        }
        let mut ordered: Vec<&ActionDef> = ACTIONS.iter().collect();
        ordered.sort_by_key(|a| !a.shortcut.is_some_and(|s| s.modifiers.shift));
        for action in ordered {
            if let Some(shortcut) = action.shortcut {
                if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) && (action.enabled)(self) {
                    (action.run)(self, ctx);
                }
            }
        }
        // Strg+C und Strg+V kommen von egui als eigene Ereignisse, nicht als Tasten.
        let (copy, paste) = ctx.input(|i| {
            (
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_))),
            )
        });
        if copy {
            self.run_action("kopieren", ctx);
        }
        if paste {
            self.paste();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(CTRL, Key::Num0))) {
            self.fit = true;
            self.view = None;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(CTRL, Key::Num1))) {
            self.zoom_to(1.0 / ctx.pixels_per_point(), None, ctx);
        }
        for (shortcut, factor) in [
            (KeyboardShortcut::new(CTRL, Key::Plus), 1.25),
            (KeyboardShortcut::new(CTRL, Key::Equals), 1.25),
            (KeyboardShortcut::new(CTRL, Key::Minus), 0.8),
        ] {
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                if let Some(view) = self.view {
                    self.zoom_to(view.scale * factor, None, ctx);
                }
            }
        }

        // Werkzeuge mit einzelnen Buchstaben.
        if self.history.is_some() {
            let pressed = ctx.input(|i| {
                if !i.modifiers.is_none() {
                    return None;
                }
                self.tools
                    .iter()
                    .position(|t| t.as_ref().and_then(|t| t.shortcut()).is_some_and(|k| i.key_pressed(k)))
            });
            if let Some(index) = pressed {
                self.select_tool(index);
            }
        }
    }

    fn run_action(&mut self, id: &str, ctx: &egui::Context) {
        if let Some(action) = ACTIONS.iter().find(|a| a.id == id) {
            if (action.enabled)(self) {
                (action.run)(self, ctx);
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| p.is_file())
        });
        if let Some(path) = dropped {
            self.open(&path);
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if !self.confirm_close {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("schliessen")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new("Änderungen speichern?").font(semibold(16.0)).color(TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Das bearbeitete Bild wurde noch nicht gespeichert oder kopiert.")
                    .color(TEXT_WEAK),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if ui.add(ui_kit::primary_button("Speichern …")).clicked() {
                    choice = Some(0);
                }
                if ui.button("Nicht speichern").clicked() {
                    choice = Some(1);
                }
                if ui.button("Abbrechen").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                self.confirm_close = false;
                self.save_dialog();
                if !self.dirty() {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Some(1) => {
                self.confirm_close = false;
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(_) => self.confirm_close = false,
            None => {}
        }
    }

    fn auto_copy(&mut self, ctx: &egui::Context) {
        // Wie im Snipping Tool: Änderungen landen von selbst in der Zwischenablage.
        if !self.settings.snip_copy || self.history.is_none() || self.gesture {
            self.copy_due = None;
            return;
        }
        let revision = self.revision();
        if revision == self.exported_revision || revision == 0 {
            return;
        }
        match self.copy_due {
            Some((r, due)) if r == revision => {
                if Instant::now() >= due {
                    self.copy_due = None;
                    self.copy(false);
                } else {
                    ctx.request_repaint_after(due - Instant::now());
                }
            }
            _ => {
                let due = Instant::now() + Duration::from_millis(600);
                self.copy_due = Some((revision, due));
                ctx.request_repaint_after(Duration::from_millis(600));
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = match (&self.file, &self.history) {
            (Some(file), _) => file_name(file),
            (None, Some(_)) => "Screenshot".into(),
            (None, None) => "Bild bearbeiten".into(),
        };
        let title = format!("{}{name} – UtilityCrab", if self.dirty() { "• " } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.finish_jobs();
        self.handle_drops(&ctx);
        self.shortcuts(&ctx);
        self.update_textures(&ctx);

        let bar = egui::Frame::new()
            .fill(ui_kit::SIDEBAR_BG)
            .inner_margin(egui::Margin::symmetric(10, 6));
        egui::Panel::top("werkzeuge").frame(bar).show(ui, |ui| self.toolbar(ui, &ctx));
        let options = egui::Frame::new()
            .fill(ui_kit::CONTENT_BG)
            .inner_margin(egui::Margin::symmetric(14, 4));
        egui::Panel::top("optionen").frame(options).show(ui, |ui| self.options_bar(ui));
        let status = egui::Frame::new()
            .fill(ui_kit::SIDEBAR_BG)
            .inner_margin(egui::Margin::symmetric(12, 3));
        egui::Panel::bottom("status").frame(status).show(ui, |ui| self.status_bar(ui));

        let canvas_bg = egui::Frame::new().fill(Color32::from_rgb(0x17, 0x18, 0x1B));
        egui::CentralPanel::default().frame(canvas_bg).show(ui, |ui| {
            if self.history.is_some() {
                self.canvas(ui, &ctx);
            } else {
                self.empty_state(ui, &ctx);
            }
        });

        self.auto_copy(&ctx);
        self.close_dialog(&ctx);
        self.update_title(&ctx);
        if self.prefs_dirty && !self.gesture {
            self.prefs.save();
            self.prefs_dirty = false;
        }
        // Statusmeldungen nach einer Weile ausblenden.
        if let Some(status) = &self.status {
            let shown = status.since.elapsed();
            let limit = Duration::from_secs(if status.error { 12 } else { 6 });
            if shown >= limit {
                self.status = None;
            } else {
                ctx.request_repaint_after(limit - shown);
            }
        }
        if self.busy.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.prefs.save();
    }
}

// ---------------------------------------------------------------------------------------------

fn canvas_texture(ctx: &egui::Context, name: &str, canvas: &Canvas) -> egui::TextureHandle {
    let (w, h) = (canvas.pixmap.width(), canvas.pixmap.height());
    let max = ctx.input(|i| i.max_texture_side) as u32;
    if w.max(h) <= max {
        let options = egui::TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Linear,
            ..Default::default()
        };
        let image = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], canvas.pixmap.data());
        return ctx.load_texture(name, image, options);
    }
    super::overlay::upload(ctx, name, &canvas.to_image(), max).0
}

fn divider(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, ui_kit::DIVIDER);
    ui.add_space(4.0);
}

fn swatch(ui: &mut Ui, color: [u8; 4], selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    let painter = ui.painter();
    let c = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
    if selected {
        painter.circle_stroke(rect.center(), 10.0, Stroke::new(2.0, ui_kit::ACCENT));
    }
    painter.circle_filled(rect.center(), 7.0, c);
    painter.circle_stroke(rect.center(), 7.0, Stroke::new(1.0, Color32::from_white_alpha(40)));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn checkerboard(painter: &egui::Painter, rect: Rect) {
    if !rect.is_positive() {
        return;
    }
    painter.rect_filled(rect, 0.0, Color32::from_gray(0xCC));
    let tile = 10.0;
    let mut y = rect.min.y;
    let mut row = 0;
    while y < rect.max.y {
        let mut x = rect.min.x + if row % 2 == 0 { 0.0 } else { tile };
        while x < rect.max.x {
            let r = Rect::from_min_max(pos2(x, y), pos2((x + tile).min(rect.max.x), (y + tile).min(rect.max.y)));
            painter.rect_filled(r, 0.0, Color32::from_gray(0xF2));
            x += tile * 2.0;
        }
        y += tile;
        row += 1;
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

