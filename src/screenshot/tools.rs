//! Werkzeuge des Editors.
//!
//! # Ein neues Werkzeug hinzufügen
//!
//! 1. Eine Anmerkung schreiben (`annotations.rs`), falls es noch keine passende gibt.
//! 2. Für „ziehen und fertig“-Werkzeuge reicht ein [`DragTool`] mit einer `make`-Funktion,
//!    sonst `Tool` selbst implementieren (siehe [`StepTool`] als kleines Beispiel).
//! 3. In [`all`] eintragen – Werkzeugleiste, Tastenkürzel, Optionen (Farbe, Stärke …)
//!    und gemerkte Einstellungen kommen dann automatisch.
//! 4. Optional ein Symbol in `icons.rs` zeichnen.

use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, Key, Pos2, Rect, Stroke, Ui};

use super::annotations::{
    drag_rect, snap_angle, Annotation, Freehand, Line, Redact, Shape, ShapeKind, Spotlight, Step, Style,
    Text,
};
use super::canvas::IRect;
use super::document::History;
use super::icons::Glyph;
use super::view::View;

/// Was ein Werkzeug gerade bearbeiten darf.
pub struct ToolCtx<'a> {
    pub history: &'a mut History,
    /// Stil dieses Werkzeugs (wird pro Werkzeug gemerkt).
    pub style: &'a mut Style,
    pub view: View,
    /// Umschalt gedrückt: Quadrat/Kreis bzw. 45°-Winkel.
    pub shift: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Input {
    Press(Pos2),
    Drag(Pos2),
    Release(Pos2),
}

/// Einstellungen, die in der Optionsleiste für ein Werkzeug erscheinen.
#[derive(Debug, Clone, Copy)]
pub enum Opt {
    Color,
    Width { label: &'static str, min: f32, max: f32 },
    Filled(&'static str),
    Redact,
    /// Weiche Kante beim Weichzeichnen (Untermenü zum Einstellen/Abschalten).
    SoftEdge,
}

pub trait Tool {
    /// Eindeutig und dauerhaft (Schlüssel für gemerkte Einstellungen).
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Kurze Bedienhilfe für die Statuszeile.
    fn hint(&self) -> &'static str;
    fn shortcut(&self) -> Option<Key>;
    fn glyph(&self) -> Glyph;
    fn default_style(&self) -> Style {
        Style::default()
    }
    fn options(&self) -> &'static [Opt] {
        &[]
    }
    fn cursor(&self) -> CursorIcon {
        CursorIcon::Crosshair
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx);

    /// Anmerkung, die gerade entsteht (wird live gerendert).
    fn preview(&self) -> Option<&dyn Annotation> {
        None
    }

    /// Eigene Oberfläche über dem Bild (Textfeld, Zuschneiderahmen, Auswahl …).
    fn overlay(&mut self, _ui: &mut Ui, _cx: &mut ToolCtx) {}

    /// Tastendruck, solange das Werkzeug aktiv ist. `true` = verbraucht.
    fn key(&mut self, _key: Key, _cx: &mut ToolCtx) -> bool {
        false
    }

    /// Wird beim Wechsel zu einem anderen Werkzeug aufgerufen (z. B. Text übernehmen).
    fn deactivate(&mut self, _cx: &mut ToolCtx) {}

    /// Tippt der Benutzer gerade (dann keine Einbuchstaben-Kürzel)?
    fn typing(&self) -> bool {
        false
    }
}

/// Alle Werkzeuge in der Reihenfolge der Werkzeugleiste. `None` = Trennstrich.
pub fn all() -> Vec<Option<Box<dyn Tool>>> {
    vec![
        Some(Box::new(SelectTool::default())),
        None,
        Some(Box::new(FreehandTool::pen())),
        Some(Box::new(FreehandTool::highlighter())),
        Some(Box::new(EraserTool::default())),
        None,
        Some(Box::new(DragTool::new(
            "pfeil",
            "Pfeil",
            "Ziehen für einen Pfeil · Umschalt rastet 45° ein",
            Key::A,
            Glyph::Arrow,
            STROKE_OPTS,
            Style::default(),
            |a, b, s, shift| {
                let to = if shift { snap_angle(a, b) } else { b };
                (a.distance(to) > 3.0).then(|| {
                    Box::new(Line {
                        from: a,
                        to,
                        color: s.color(),
                        width: s.width,
                        arrow: true,
                    }) as Box<dyn Annotation>
                })
            },
        ))),
        Some(Box::new(DragTool::new(
            "linie",
            "Linie",
            "Ziehen für eine Linie · Umschalt rastet 45° ein",
            Key::L,
            Glyph::Line,
            STROKE_OPTS,
            Style::default(),
            |a, b, s, shift| {
                let to = if shift { snap_angle(a, b) } else { b };
                (a.distance(to) > 2.0).then(|| {
                    Box::new(Line {
                        from: a,
                        to,
                        color: s.color(),
                        width: s.width,
                        arrow: false,
                    }) as Box<dyn Annotation>
                })
            },
        ))),
        Some(Box::new(DragTool::new(
            "rechteck",
            "Rechteck",
            "Ziehen für ein Rechteck · Umschalt für ein Quadrat",
            Key::R,
            Glyph::Rect,
            SHAPE_OPTS,
            Style::default(),
            |a, b, s, shift| shape(ShapeKind::Rect, a, b, s, shift),
        ))),
        Some(Box::new(DragTool::new(
            "ellipse",
            "Ellipse",
            "Ziehen für eine Ellipse · Umschalt für einen Kreis",
            Key::E,
            Glyph::Ellipse,
            SHAPE_OPTS,
            Style::default(),
            |a, b, s, shift| shape(ShapeKind::Ellipse, a, b, s, shift),
        ))),
        Some(Box::new(TextTool::default())),
        Some(Box::new(StepTool)),
        None,
        Some(Box::new(DragTool::new(
            "unkenntlich",
            "Unkenntlich machen",
            "Bereich aufziehen, der verpixelt, weichgezeichnet oder geschwärzt wird",
            Key::U,
            Glyph::Redact,
            &[Opt::Redact, Opt::Width { label: "Stärke", min: 4.0, max: 40.0 }, Opt::SoftEdge],
            Style {
                width: 12.0,
                ..Style::default()
            },
            |a, b, s, shift| {
                let rect = drag_rect(a, b, shift);
                (rect.width() >= 2.0 && rect.height() >= 2.0).then(|| {
                    Box::new(Redact {
                        rect,
                        mode: s.redact,
                        strength: s.width,
                        soft: s.soft(),
                    }) as Box<dyn Annotation>
                })
            },
        ))),
        Some(Box::new(DragTool::new(
            "spotlight",
            "Spotlight",
            "Bereich aufziehen, der hervorgehoben wird – der Rest wird abgedunkelt",
            Key::S,
            Glyph::Spotlight,
            &[
                Opt::Width { label: "Abdunkeln", min: 10.0, max: 90.0 },
                Opt::Filled("Ellipse"),
            ],
            Style {
                width: 55.0,
                ..Style::default()
            },
            |a, b, s, shift| {
                let rect = drag_rect(a, b, shift);
                (rect.width() >= 4.0 && rect.height() >= 4.0).then(|| {
                    Box::new(Spotlight {
                        rect,
                        ellipse: s.filled,
                        dim: s.width / 100.0,
                    }) as Box<dyn Annotation>
                })
            },
        ))),
        Some(Box::new(CropTool::default())),
    ]
}

const STROKE_OPTS: &[Opt] = &[Opt::Color, Opt::Width { label: "Stärke", min: 1.0, max: 30.0 }];
const SHAPE_OPTS: &[Opt] = &[
    Opt::Color,
    Opt::Width { label: "Stärke", min: 1.0, max: 30.0 },
    Opt::Filled("Gefüllt"),
];

fn shape(kind: ShapeKind, a: Pos2, b: Pos2, s: &Style, shift: bool) -> Option<Box<dyn Annotation>> {
    let rect = drag_rect(a, b, shift);
    (rect.width() >= 2.0 || rect.height() >= 2.0).then(|| {
        Box::new(Shape {
            kind,
            rect,
            color: s.color(),
            width: s.width,
            filled: s.filled,
        }) as Box<dyn Annotation>
    })
}

// ---------------------------------------------------------------------------------------------
// Ziehen → eine Anmerkung (Pfeil, Linie, Formen, Unkenntlich, Spotlight)

type Make = fn(Pos2, Pos2, &Style, bool) -> Option<Box<dyn Annotation>>;

pub struct DragTool {
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    key: Key,
    glyph: Glyph,
    options: &'static [Opt],
    default: Style,
    make: Make,
    start: Option<Pos2>,
    current: Option<Box<dyn Annotation>>,
}

impl DragTool {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        key: Key,
        glyph: Glyph,
        options: &'static [Opt],
        default: Style,
        make: Make,
    ) -> Self {
        Self {
            id,
            label,
            hint,
            key,
            glyph,
            options,
            default,
            make,
            start: None,
            current: None,
        }
    }
}

impl Tool for DragTool {
    fn id(&self) -> &'static str {
        self.id
    }
    fn label(&self) -> &'static str {
        self.label
    }
    fn hint(&self) -> &'static str {
        self.hint
    }
    fn shortcut(&self) -> Option<Key> {
        Some(self.key)
    }
    fn glyph(&self) -> Glyph {
        self.glyph
    }
    fn default_style(&self) -> Style {
        self.default
    }
    fn options(&self) -> &'static [Opt] {
        self.options
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        match input {
            Input::Press(p) => {
                self.start = Some(p);
                self.current = None;
            }
            Input::Drag(p) => {
                if let Some(start) = self.start {
                    self.current = (self.make)(start, p, cx.style, cx.shift);
                }
            }
            Input::Release(p) => {
                if let Some(start) = self.start.take() {
                    self.current = None;
                    if let Some(item) = (self.make)(start, p, cx.style, cx.shift) {
                        cx.history.push(item);
                    }
                }
            }
        }
    }

    fn preview(&self) -> Option<&dyn Annotation> {
        self.current.as_deref()
    }
}

// ---------------------------------------------------------------------------------------------
// Stift und Textmarker

pub struct FreehandTool {
    highlighter: bool,
    current: Option<Freehand>,
}

impl FreehandTool {
    pub fn pen() -> Self {
        Self {
            highlighter: false,
            current: None,
        }
    }

    pub fn highlighter() -> Self {
        Self {
            highlighter: true,
            current: None,
        }
    }
}

impl Tool for FreehandTool {
    fn id(&self) -> &'static str {
        if self.highlighter {
            "textmarker"
        } else {
            "stift"
        }
    }
    fn label(&self) -> &'static str {
        if self.highlighter {
            "Textmarker"
        } else {
            "Stift"
        }
    }
    fn hint(&self) -> &'static str {
        if self.highlighter {
            "Über Text malen, um ihn hervorzuheben"
        } else {
            "Frei zeichnen"
        }
    }
    fn shortcut(&self) -> Option<Key> {
        Some(if self.highlighter { Key::M } else { Key::P })
    }
    fn glyph(&self) -> Glyph {
        if self.highlighter {
            Glyph::Highlighter
        } else {
            Glyph::Pen
        }
    }
    fn default_style(&self) -> Style {
        if self.highlighter {
            Style {
                color: [0xFF, 0xE0, 0x3D, 0xFF],
                width: 20.0,
                ..Style::default()
            }
        } else {
            Style::default()
        }
    }
    fn options(&self) -> &'static [Opt] {
        if self.highlighter {
            &[Opt::Color, Opt::Width { label: "Stärke", min: 6.0, max: 60.0 }]
        } else {
            STROKE_OPTS
        }
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        match input {
            Input::Press(p) => {
                self.current = Some(Freehand {
                    points: vec![p],
                    color: cx.style.color(),
                    width: cx.style.width,
                    highlighter: self.highlighter,
                });
            }
            Input::Drag(p) => {
                if let Some(stroke) = &mut self.current {
                    // Mindestabstand hält die Punktliste klein.
                    if stroke.points.last().is_none_or(|last| last.distance(p) >= cx.view.image_len(1.5)) {
                        stroke.points.push(p);
                    }
                }
            }
            Input::Release(_) => {
                if let Some(stroke) = self.current.take() {
                    cx.history.push(Box::new(stroke));
                }
            }
        }
    }

    fn preview(&self) -> Option<&dyn Annotation> {
        self.current.as_ref().map(|s| s as &dyn Annotation)
    }
}

// ---------------------------------------------------------------------------------------------
// Radierer: entfernt ganze Anmerkungen

#[derive(Default)]
pub struct EraserTool {
    erased_in_gesture: bool,
}

impl EraserTool {
    fn erase(&mut self, p: Pos2, cx: &mut ToolCtx) {
        let tolerance = cx.view.image_len(6.0);
        if let Some(index) = cx.history.doc.hit(p, tolerance) {
            if !self.erased_in_gesture {
                cx.history.checkpoint();
                self.erased_in_gesture = true;
            }
            cx.history.doc_mut().items.remove(index);
        }
    }
}

impl Tool for EraserTool {
    fn id(&self) -> &'static str {
        "radierer"
    }
    fn label(&self) -> &'static str {
        "Radierer"
    }
    fn hint(&self) -> &'static str {
        "Über Anmerkungen fahren, um sie zu entfernen"
    }
    fn shortcut(&self) -> Option<Key> {
        Some(Key::X)
    }
    fn glyph(&self) -> Glyph {
        Glyph::Eraser
    }
    fn cursor(&self) -> CursorIcon {
        CursorIcon::Cell
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        match input {
            Input::Press(p) => {
                self.erased_in_gesture = false;
                self.erase(p, cx);
            }
            Input::Drag(p) => self.erase(p, cx),
            Input::Release(_) => self.erased_in_gesture = false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Auswählen und verschieben

#[derive(Default)]
pub struct SelectTool {
    selected: Option<usize>,
    last: Option<Pos2>,
    moved: bool,
}

impl Tool for SelectTool {
    fn id(&self) -> &'static str {
        "auswahl"
    }
    fn label(&self) -> &'static str {
        "Auswählen"
    }
    fn hint(&self) -> &'static str {
        "Anmerkung anklicken und verschieben · Entf löscht sie"
    }
    fn shortcut(&self) -> Option<Key> {
        Some(Key::V)
    }
    fn glyph(&self) -> Glyph {
        Glyph::Select
    }
    fn cursor(&self) -> CursorIcon {
        CursorIcon::Default
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        match input {
            Input::Press(p) => {
                self.selected = cx.history.doc.hit(p, cx.view.image_len(6.0));
                self.last = Some(p);
                self.moved = false;
            }
            Input::Drag(p) => {
                let (Some(index), Some(last)) = (self.selected, self.last) else {
                    return;
                };
                if index >= cx.history.doc.items.len() {
                    self.selected = None;
                    return;
                }
                if !self.moved {
                    cx.history.checkpoint();
                    self.moved = true;
                }
                cx.history.doc_mut().items[index].translate(p - last);
                self.last = Some(p);
            }
            Input::Release(_) => self.last = None,
        }
    }

    fn overlay(&mut self, ui: &mut Ui, cx: &mut ToolCtx) {
        let Some(item) = self.selected.and_then(|i| cx.history.doc.items.get(i)) else {
            self.selected = None;
            return;
        };
        let bounds = item.bounds().intersect(cx.history.doc.image_rect());
        let rect = cx.view.rect_to_screen(bounds).expand(3.0);
        dashed_rect(ui.painter(), rect, Color32::WHITE, ui_accent());
    }

    fn key(&mut self, key: Key, cx: &mut ToolCtx) -> bool {
        match (key, self.selected) {
            (Key::Delete | Key::Backspace, Some(index)) if index < cx.history.doc.items.len() => {
                cx.history.edit(|doc| {
                    doc.items.remove(index);
                });
                self.selected = None;
                true
            }
            (Key::Escape, Some(_)) => {
                self.selected = None;
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Schrittmarker ①②③

pub struct StepTool;

impl Tool for StepTool {
    fn id(&self) -> &'static str {
        "schritte"
    }
    fn label(&self) -> &'static str {
        "Schrittnummern"
    }
    fn hint(&self) -> &'static str {
        "Klicken setzt fortlaufende Nummern 1, 2, 3 … für Anleitungen"
    }
    fn shortcut(&self) -> Option<Key> {
        Some(Key::N)
    }
    fn glyph(&self) -> Glyph {
        Glyph::Step
    }
    fn default_style(&self) -> Style {
        Style {
            width: 16.0,
            ..Style::default()
        }
    }
    fn options(&self) -> &'static [Opt] {
        &[Opt::Color, Opt::Width { label: "Größe", min: 8.0, max: 60.0 }]
    }
    fn cursor(&self) -> CursorIcon {
        CursorIcon::PointingHand
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        if let Input::Press(p) = input {
            let number = cx.history.doc.next_step_number();
            cx.history.push(Box::new(Step {
                center: p,
                number,
                radius: cx.style.width,
                color: cx.style.color(),
            }));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Text

#[derive(Default)]
pub struct TextTool {
    editing: Option<(Pos2, String)>,
    focus: bool,
}

impl TextTool {
    fn commit(&mut self, cx: &mut ToolCtx) {
        if let Some((pos, text)) = self.editing.take() {
            let text = text.trim_end().to_string();
            if !text.trim().is_empty() {
                cx.history.push(Box::new(Text {
                    pos,
                    text,
                    size: cx.style.width,
                    color: cx.style.color(),
                    boxed: cx.style.filled,
                }));
            }
        }
    }
}

impl Tool for TextTool {
    fn id(&self) -> &'static str {
        "text"
    }
    fn label(&self) -> &'static str {
        "Text"
    }
    fn hint(&self) -> &'static str {
        "Klicken und tippen · Enter übernimmt, Umschalt+Enter für eine neue Zeile"
    }
    fn shortcut(&self) -> Option<Key> {
        Some(Key::T)
    }
    fn glyph(&self) -> Glyph {
        Glyph::Text
    }
    fn default_style(&self) -> Style {
        Style {
            width: 28.0,
            ..Style::default()
        }
    }
    fn options(&self) -> &'static [Opt] {
        &[
            Opt::Color,
            Opt::Width { label: "Größe", min: 10.0, max: 120.0 },
            Opt::Filled("Hinterlegt"),
        ]
    }
    fn cursor(&self) -> CursorIcon {
        CursorIcon::Text
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        if let Input::Press(p) = input {
            if self.editing.is_some() {
                // Klick daneben übernimmt den Text.
                self.commit(cx);
            } else {
                self.editing = Some((p, String::new()));
                self.focus = true;
            }
        }
    }

    fn preview(&self) -> Option<&dyn Annotation> {
        None
    }

    fn overlay(&mut self, ui: &mut Ui, cx: &mut ToolCtx) {
        let Some((pos, text)) = &mut self.editing else {
            return;
        };
        let screen = cx.view.to_screen(*pos);
        let size = (cx.style.width * cx.view.scale).max(8.0);
        let color = cx.style.color();
        let mut commit = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::unique("text-werkzeug"))
            .fixed_pos(screen - vec2(4.0, 4.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .stroke(Stroke::new(1.0, ui_accent()))
                    .fill(Color32::from_black_alpha(40))
                    .inner_margin(egui::Margin::same(3))
                    .show(ui, |ui| {
                        // Breite wächst mit dem Text (grob geschätzt), das Feld bleibt so kompakt.
                        let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
                        let width = (size * 0.62 * (longest + 2.0)).max(size * 4.0);
                        let edit = egui::TextEdit::multiline(text)
                            .font(egui::FontId::new(size, egui::FontFamily::Name(crate::ui_kit::SEMIBOLD.into())))
                            .text_color(color)
                            .frame(egui::Frame::NONE)
                            .desired_rows(1)
                            .desired_width(width)
                            .hint_text("Text …")
                            .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)));
                        let response = ui.add(edit);
                        if self.focus {
                            response.request_focus();
                            self.focus = false;
                        }
                        if response.has_focus() {
                            commit = ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
                            cancel = ui.input(|i| i.key_pressed(Key::Escape));
                        }
                    });
            });
        if cancel {
            self.editing = None;
        } else if commit {
            self.commit(cx);
        }
    }

    fn deactivate(&mut self, cx: &mut ToolCtx) {
        self.commit(cx);
    }

    fn typing(&self) -> bool {
        self.editing.is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// Zuschneiden

#[derive(Default)]
pub struct CropTool {
    rect: Option<Rect>,
    drag: Option<CropDrag>,
}

#[derive(Clone, Copy)]
enum CropDrag {
    New(Pos2),
    Move(Pos2),
    /// Kanten: links, oben, rechts, unten.
    Edges([bool; 4]),
}

impl CropTool {
    fn apply(&mut self, cx: &mut ToolCtx) {
        if let Some(rect) = self.rect.take() {
            let area = IRect::round(rect);
            if area.w >= 2 && area.h >= 2 {
                cx.history.edit(|doc| doc.crop(area));
            }
        }
    }

    fn edges_at(&self, p: Pos2, tolerance: f32) -> Option<[bool; 4]> {
        let r = self.rect?;
        if !r.expand(tolerance).contains(p) {
            return None;
        }
        let edges = [
            (p.x - r.min.x).abs() <= tolerance,
            (p.y - r.min.y).abs() <= tolerance,
            (p.x - r.max.x).abs() <= tolerance,
            (p.y - r.max.y).abs() <= tolerance,
        ];
        edges.iter().any(|e| *e).then_some(edges)
    }
}

impl Tool for CropTool {
    fn id(&self) -> &'static str {
        "zuschneiden"
    }
    fn label(&self) -> &'static str {
        "Zuschneiden"
    }
    fn hint(&self) -> &'static str {
        "Bereich aufziehen, Kanten anpassen · Enter schneidet zu, Esc bricht ab"
    }
    fn shortcut(&self) -> Option<Key> {
        Some(Key::C)
    }
    fn glyph(&self) -> Glyph {
        Glyph::Crop
    }

    fn pointer(&mut self, input: Input, cx: &mut ToolCtx) {
        let bounds = cx.history.doc.image_rect();
        let clamp = |p: Pos2| pos2(p.x.clamp(0.0, bounds.max.x), p.y.clamp(0.0, bounds.max.y));
        match input {
            Input::Press(p) => {
                let tolerance = cx.view.image_len(8.0);
                self.drag = Some(if let Some(edges) = self.edges_at(p, tolerance) {
                    CropDrag::Edges(edges)
                } else if self.rect.is_some_and(|r| r.contains(p)) {
                    CropDrag::Move(p)
                } else {
                    CropDrag::New(clamp(p))
                });
            }
            Input::Drag(p) => match self.drag {
                Some(CropDrag::New(start)) => self.rect = Some(drag_rect(start, clamp(p), cx.shift)),
                Some(CropDrag::Move(last)) => {
                    if let Some(r) = self.rect {
                        let mut delta = p - last;
                        delta.x = delta.x.clamp(-r.min.x, bounds.max.x - r.max.x);
                        delta.y = delta.y.clamp(-r.min.y, bounds.max.y - r.max.y);
                        self.rect = Some(r.translate(delta));
                        self.drag = Some(CropDrag::Move(last + delta));
                    }
                }
                Some(CropDrag::Edges([l, t, r, b])) => {
                    if let Some(mut rect) = self.rect {
                        let p = clamp(p);
                        if l {
                            rect.min.x = p.x.min(rect.max.x - 2.0);
                        }
                        if t {
                            rect.min.y = p.y.min(rect.max.y - 2.0);
                        }
                        if r {
                            rect.max.x = p.x.max(rect.min.x + 2.0);
                        }
                        if b {
                            rect.max.y = p.y.max(rect.min.y + 2.0);
                        }
                        self.rect = Some(rect);
                    }
                }
                None => {}
            },
            Input::Release(_) => {
                self.drag = None;
                if self.rect.is_some_and(|r| r.width() < 2.0 || r.height() < 2.0) {
                    self.rect = None;
                }
            }
        }
    }

    fn overlay(&mut self, ui: &mut Ui, cx: &mut ToolCtx) {
        let Some(rect) = self.rect else {
            return;
        };
        let image = cx.view.rect_to_screen(cx.history.doc.image_rect());
        let sel = cx.view.rect_to_screen(rect);
        let painter = ui.painter();
        let dim = Color32::from_black_alpha(140);
        for r in [
            Rect::from_min_max(image.min, pos2(image.max.x, sel.min.y)),
            Rect::from_min_max(pos2(image.min.x, sel.max.y), image.max),
            Rect::from_min_max(pos2(image.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)),
            Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(image.max.x, sel.max.y)),
        ] {
            painter.rect_filled(r, 0.0, dim);
        }
        painter.rect_stroke(sel, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
        // Drittel-Linien und Griffe an den Ecken.
        for i in 1..3 {
            let f = i as f32 / 3.0;
            let x = egui::lerp(sel.min.x..=sel.max.x, f);
            let y = egui::lerp(sel.min.y..=sel.max.y, f);
            painter.line_segment([pos2(x, sel.min.y), pos2(x, sel.max.y)], Stroke::new(1.0, Color32::from_white_alpha(70)));
            painter.line_segment([pos2(sel.min.x, y), pos2(sel.max.x, y)], Stroke::new(1.0, Color32::from_white_alpha(70)));
        }
        for corner in [sel.left_top(), sel.right_top(), sel.left_bottom(), sel.right_bottom()] {
            painter.rect_filled(Rect::from_center_size(corner, vec2(9.0, 9.0)), 2.0, Color32::WHITE);
        }
        let label = format!("{} × {}", rect.width().round(), rect.height().round());
        painter.text(
            sel.left_top() + vec2(0.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            crate::ui_kit::regular(crate::ui_kit::SMALL_SIZE),
            Color32::WHITE,
        );

        // Knöpfe unter dem Rahmen.
        let mut apply = false;
        let mut cancel = false;
        let anchor = pos2(sel.right(), (sel.bottom() + 8.0).min(ui.clip_rect().bottom() - 36.0));
        egui::Area::new(egui::Id::unique("zuschneiden-knoepfe"))
            .fixed_pos(anchor)
            .pivot(egui::Align2::RIGHT_TOP)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        cancel = ui.button("Abbrechen").clicked();
                        apply = ui.add(crate::ui_kit::primary_button("Zuschneiden")).clicked();
                    });
                });
            });
        if apply {
            self.apply(cx);
        } else if cancel {
            self.rect = None;
        }
    }

    fn key(&mut self, key: Key, cx: &mut ToolCtx) -> bool {
        match key {
            Key::Enter if self.rect.is_some() => {
                self.apply(cx);
                true
            }
            Key::Escape if self.rect.is_some() => {
                self.rect = None;
                true
            }
            _ => false,
        }
    }

    fn deactivate(&mut self, _cx: &mut ToolCtx) {
        self.rect = None;
    }
}

// ---------------------------------------------------------------------------------------------

fn ui_accent() -> Color32 {
    crate::ui_kit::ACCENT
}

/// Gestricheltes Rechteck (Auswahlrahmen).
pub fn dashed_rect(painter: &egui::Painter, rect: Rect, a: Color32, b: Color32) {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    painter.add(egui::Shape::line(corners.to_vec(), Stroke::new(1.0, a)));
    painter.add(egui::Shape::dashed_line(&corners, Stroke::new(1.0, b), 5.0, 4.0));
}
