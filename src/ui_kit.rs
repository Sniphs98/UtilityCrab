//! Gemeinsamer Baukasten für alle Fenster (Einstellungen, Konverter, Editor, Popup), gebaut
//! mit GPUI und GPUI Kit: Systemschrift, Farben und Bausteine im Stil der
//! macOS-Systemeinstellungen – Seiten mit Titel, gruppierte Listen aus Zeilen (Beschriftung
//! links, Bedienelement rechts), Schalter statt Checkboxen, Tastenkappen.
//!
//! Dazu ein kleiner [`Painter`] für selbst gezeichnete Symbole und Hilfslinien.

use std::sync::Arc;

pub use gpui_kit::component::button::{Button, ButtonVariants};
pub use gpui_kit::component::{h_flex, v_flex, Disableable, Sizable, StyledExt};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::geom::{pos2, vec2, Color32, Pos2, Rect};

// ---------------------------------------------------------------------------------------------
// Farben

pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x1C);
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x1F);
pub const CONTENT_BG: Color32 = Color32::from_rgb(0x22, 0x23, 0x27);
pub const GROUP_BG: Color32 = Color32::from_rgb(0x2B, 0x2D, 0x31);
pub const DIVIDER: Color32 = Color32::from_rgb(0x36, 0x38, 0x3E);
pub const CONTROL: Color32 = Color32::from_rgb(0x3A, 0x3C, 0x42);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x45, 0x48, 0x4F);
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x50, 0x53, 0x5A);
pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF3);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0xA6, 0xAB, 0xB3);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x80, 0x85, 0x8E);
pub const GREEN: Color32 = Color32::from_rgb(0x4C, 0xC0, 0x6A);
pub const RED: Color32 = Color32::from_rgb(0xEE, 0x5D, 0x4E);
pub const DANGER: Color32 = Color32::from_rgb(0xB8, 0x3A, 0x2E);

/// Farbe mit geringer Deckkraft, für getönte Hintergründe.
pub fn tint(color: Color32, alpha: f32) -> Color32 {
    color.with_alpha(alpha)
}

impl From<Color32> for Rgba {
    fn from(c: Color32) -> Self {
        Rgba {
            r: c.r as f32 / 255.0,
            g: c.g as f32 / 255.0,
            b: c.b as f32 / 255.0,
            a: c.a as f32 / 255.0,
        }
    }
}

impl From<Color32> for Hsla {
    fn from(c: Color32) -> Self {
        Rgba::from(c).into()
    }
}

impl From<Color32> for Fill {
    fn from(c: Color32) -> Self {
        Hsla::from(c).into()
    }
}

impl From<Color32> for Background {
    fn from(c: Color32) -> Self {
        Hsla::from(c).into()
    }
}

// ---------------------------------------------------------------------------------------------
// Schrift

pub const FONT: &str = "Segoe UI";
pub const TITLE_SIZE: f32 = 20.0;
pub const BODY_SIZE: f32 = 13.5;
pub const SMALL_SIZE: f32 = 12.0;

// ---------------------------------------------------------------------------------------------
// Start

static APP_RAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ob dieser Prozess GPUI schon einmal gestartet hat (geht nur einmal pro Prozess).
pub fn has_run() -> bool {
    APP_RAN.load(std::sync::atomic::Ordering::SeqCst)
}

/// GPUI starten, GPUI Kit und das gemeinsame Aussehen einrichten und `setup` ausführen
/// (Fenster öffnen). Kehrt zurück, wenn das letzte Fenster geschlossen ist.
pub fn run_app(setup: impl FnOnce(&mut App) + 'static) {
    APP_RAN.store(true, std::sync::atomic::Ordering::SeqCst);
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        apply_theme(cx);
        setup(cx);
    });
}

/// Dunkles Thema mit der orangen Akzentfarbe und Segoe UI.
pub fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    // Über `update`, damit auch die Tokens (daraus nehmen z. B. Knöpfe und Regler ihre
    // Hintergründe) die neuen Farben bekommen. `global_mut` ändert nur `colors`.
    Theme::update(cx, |theme| {
        theme.font_family = FONT.into();
        theme.font_size = px(BODY_SIZE);
        theme.radius = px(6.0);
        theme.radius_lg = px(10.0);
        theme.shadow = true;

        let c = &mut theme.colors;
        c.background = CONTENT_BG.into();
        c.foreground = TEXT.into();
        c.border = DIVIDER.into();
        c.input = DIVIDER.into();
        c.muted = GROUP_BG.into();
        c.muted_foreground = TEXT_WEAK.into();
        c.popover = GROUP_BG.into();
        c.popover_foreground = TEXT.into();
        c.accent = CONTROL.into();
        c.accent_foreground = TEXT.into();
        c.ring = ACCENT.into();
        c.caret = ACCENT.into();
        c.selection = tint(ACCENT, 0.45).into();
        c.link = ACCENT.into();
        c.link_hover = Color32::from_rgb(0xFF, 0x8A, 0x45).into();
        c.link_active = ACCENT.into();

        c.primary = ACCENT.into();
        c.primary_hover = Color32::from_rgb(0xF0, 0x7A, 0x30).into();
        c.primary_active = Color32::from_rgb(0xD0, 0x5C, 0x14).into();
        c.primary_foreground = Color32::WHITE.into();
        c.button_primary = c.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;

        c.secondary = CONTROL.into();
        c.secondary_hover = CONTROL_HOVER.into();
        c.secondary_active = CONTROL_ACTIVE.into();
        c.secondary_foreground = TEXT.into();
        c.button = CONTROL.into();
        c.button_hover = CONTROL_HOVER.into();
        c.button_active = CONTROL_ACTIVE.into();
        c.button_foreground = TEXT.into();
        c.button_secondary = c.secondary;
        c.button_secondary_hover = c.secondary_hover;
        c.button_secondary_active = c.secondary_active;
        c.button_secondary_foreground = c.secondary_foreground;

        c.danger = DANGER.into();
        c.danger_hover = Color32::from_rgb(0xC8, 0x48, 0x3B).into();
        c.danger_active = Color32::from_rgb(0xA0, 0x30, 0x25).into();
        c.danger_foreground = Color32::WHITE.into();
        c.button_danger = c.danger;
        c.button_danger_hover = c.danger_hover;
        c.button_danger_active = c.danger_active;
        c.button_danger_foreground = c.danger_foreground;

        c.success = GREEN.into();
        c.progress_bar = ACCENT.into();
        c.slider_bar = ACCENT.into();
        c.slider_thumb = Color32::WHITE.into();
        c.list_hover = CONTROL.into();
        c.list_active = tint(ACCENT, 0.25).into();
        c.list_active_border = ACCENT.into();
        c.sidebar = SIDEBAR_BG.into();
        c.scrollbar = Color32::TRANSPARENT.into();
        c.scrollbar_thumb = Color32::from_rgb(0x4A, 0x4D, 0x55).into();
        c.scrollbar_thumb_hover = Color32::from_rgb(0x5A, 0x5E, 0x66).into();
    });
}

/// Fensteroptionen für ein normales Fenster mit Titelleiste.
pub fn window_options(title: &str, size: (f32, f32), min_size: (f32, f32), cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, gpui_kit::size(px(size.0), px(size.1)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(title.to_string())),
            ..Default::default()
        }),
        window_min_size: Some(gpui_kit::size(px(min_size.0), px(min_size.1))),
        focus: true,
        show: true,
        ..Default::default()
    }
}

/// Öffnet ein Fenster mit dem Wurzelelement von GPUI Kit (Dialoge, Tooltips, Menüs).
pub fn open_window<V: Render>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Option<(AnyWindowHandle, Entity<V>)> {
    match gpui_kit::open_window(options, cx, build) {
        Ok(opened) => Some(opened),
        Err(e) => {
            crate::report(Err(format!("Fenster konnte nicht geöffnet werden: {e}")));
            None
        }
    }
}

/// Win32-Handle eines GPUI-Fensters (für Platzierung, Stil und DWM-Attribute).
pub fn hwnd(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Text

/// Text in Größe und Farbe.
pub fn text(content: impl Into<SharedString>, size: f32, color: Color32) -> Div {
    div().text_size(px(size)).text_color(color).child(content.into())
}

/// Hervorgehobener Text (Semibold).
pub fn strong(content: impl Into<SharedString>, size: f32, color: Color32) -> Div {
    text(content, size, color).font_weight(FontWeight::SEMIBOLD)
}

// ---------------------------------------------------------------------------------------------
// Seiten, Gruppen, Zeilen

/// Seitentitel mit kurzer Erklärung darunter.
pub fn page_header(title: &str, subtitle: &str) -> Div {
    v_flex()
        .gap(px(2.0))
        .mb(px(18.0))
        .child(strong(title.to_string(), TITLE_SIZE, TEXT))
        .child(text(subtitle.to_string(), BODY_SIZE, TEXT_WEAK))
}

/// Gruppe von Zeilen mit optionaler Überschrift und Fußnote.
#[derive(IntoElement, Default)]
pub struct Section {
    title: Option<SharedString>,
    footnote: Option<SharedString>,
    rows: Vec<AnyElement>,
}

pub fn section() -> Section {
    Section::default()
}

impl Section {
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn footnote(mut self, footnote: impl Into<SharedString>) -> Self {
        self.footnote = Some(footnote.into());
        self
    }

    /// Beliebige Zeile (siehe [`row`], [`choice`], [`custom`]).
    pub fn child(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }
    /// Wie [`Section::child`], aber nur, wenn es die Zeile gibt.
    pub fn child_if_some(self, row: Option<impl IntoElement>) -> Self {
        match row {
            Some(row) => self.child(row),
            None => self,
        }
    }

    /// Zeile: Beschriftung (und Erklärung) links, Bedienelement rechtsbündig.
    pub fn row(self, label: impl Into<SharedString>, description: Option<&str>, control: impl IntoElement) -> Self {
        self.child(row(label, description, control))
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let count = self.rows.len();
        let mut group = v_flex().w_full().bg(GROUP_BG).rounded(px(10.0));
        for (i, row) in self.rows.into_iter().enumerate() {
            group = group.child(row);
            if i + 1 < count {
                group = group.child(div().h(px(1.0)).ml(px(14.0)).bg(DIVIDER));
            }
        }
        v_flex()
            .w_full()
            .mb(px(18.0))
            .when_some(self.title, |el, title| {
                el.child(div().pl(px(4.0)).pb(px(5.0)).child(strong(title, SMALL_SIZE, TEXT_WEAK)))
            })
            .child(group)
            .when_some(self.footnote, |el, note| {
                el.child(div().px(px(4.0)).pt(px(5.0)).child(text(note, SMALL_SIZE, TEXT_FAINT)))
            })
    }
}

fn row_frame() -> Div {
    h_flex().w_full().min_h(px(44.0)).px(px(14.0)).py(px(8.0)).gap(px(12.0))
}

fn label_block(label: SharedString, description: Option<&str>) -> Div {
    v_flex()
        .flex_1()
        .min_w(px(120.0))
        .gap(px(1.0))
        .child(text(label, BODY_SIZE, TEXT))
        .when_some(description.map(str::to_string), |el, d| el.child(text(d, SMALL_SIZE, TEXT_WEAK)))
}

/// Zeile: Beschriftung (und Erklärung) links, Bedienelement rechtsbündig.
pub fn row(label: impl Into<SharedString>, description: Option<&str>, control: impl IntoElement) -> AnyElement {
    row_frame()
        .items_center()
        .child(label_block(label.into(), description))
        .child(h_flex().flex_none().gap(px(8.0)).items_center().justify_end().child(control))
        .into_any_element()
}

/// Auswahlzeile mit Häkchen rechts (wie eine Liste in den Systemeinstellungen).
pub fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    description: Option<&str>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    row_frame()
        .id(id)
        .items_center()
        .cursor_pointer()
        .child(label_block(label.into(), description))
        .child(
            div().flex_none().size(px(16.0)).when(selected, |el| {
                el.child(
                    canvas(|_, _, _| (), move |bounds, _, window, _| {
                        let rect = to_rect(bounds);
                        checkmark(&mut Painter::new(window), rect, ACCENT);
                    })
                    .size_full(),
                )
            }),
        )
        .on_click(on_click)
        .into_any_element()
}

/// Beliebiger Inhalt über die ganze Breite (z. B. ein Textfeld).
pub fn custom(content: impl IntoElement) -> AnyElement {
    v_flex().w_full().px(px(14.0)).py(px(10.0)).gap(px(8.0)).child(content).into_any_element()
}

// ---------------------------------------------------------------------------------------------
// Bedienelemente

/// Hauptaktion (Akzentfarbe).
pub fn primary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).primary().label(label)
}

/// Normale Aktion.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label)
}

/// Aktion, die Daten löscht.
pub fn danger_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).danger().label(label)
}

/// Tastenkürzel als einzelne Tastenkappen, z. B. [Strg] [Alt] [S].
pub fn keycaps(label: &str) -> Div {
    h_flex().gap(px(4.0)).children(label.split('+').map(|key| {
        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(22.0))
            .min_w(px(24.0))
            .px(px(6.0))
            .rounded(px(5.0))
            .bg(Color32::from_rgb(0x3D, 0x40, 0x46))
            .border_b_1()
            .border_color(Color32::from_rgb(0x18, 0x19, 0x1C))
            .child(strong(key.to_string(), SMALL_SIZE, TEXT))
    }))
}

/// Rot getönter Hinweis bei Fehlern.
pub fn error_banner(message: &str) -> Div {
    div()
        .w_full()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(tint(RED, 0.14))
        .child(text(message.to_string(), BODY_SIZE, Color32::from_rgb(0xFF, 0xB4, 0xA8)))
}

/// Senkrechter Trennstrich in Werkzeugleisten.
pub fn vertical_divider() -> Div {
    div().w(px(1.0)).h(px(22.0)).mx(px(4.0)).bg(DIVIDER)
}

/// Kleines Element, das mit dem [`Painter`] gezeichnet wird.
pub fn painted(paint: impl Fn(&mut Painter, Rect) + 'static) -> Canvas<()> {
    canvas(|_, _, _| (), move |bounds, _, window, _| paint(&mut Painter::new(window), to_rect(bounds)))
}

pub fn checkmark(painter: &mut Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width() / 16.0;
    painter.line(&[c + vec2(-5.0, 0.5) * s, c + vec2(-1.5, 4.0) * s, c + vec2(5.5, -4.0) * s], 2.0 * s, color);
}

// ---------------------------------------------------------------------------------------------
// Seitenleiste

/// Symbol einer Seite: farbiges abgerundetes Quadrat mit weißem Zeichen.
#[derive(Clone, Copy)]
pub enum NavIcon {
    Speaker,
    Download,
    Text,
    Keyboard,
    Convert,
    Camera,
    Info,
}

/// Eintrag in der Seitenleiste.
pub fn nav_item(
    id: impl Into<ElementId>,
    selected: bool,
    icon: NavIcon,
    color: Color32,
    label: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let glyph = div()
        .relative()
        .flex_none()
        .size(px(20.0))
        .rounded(px(5.0))
        .bg(color)
        .flex()
        .items_center()
        .justify_center()
        .map(|el| match icon {
            // Text lässt sich nicht mit Linien zeichnen – hier die echte Schrift nehmen.
            NavIcon::Text => el.child(strong("Aa", 10.5, Color32::WHITE)),
            _ => el.child(painted(move |p, rect| nav_glyph(p, rect, icon)).absolute().inset_0()),
        });
    let label = if selected {
        strong(label.to_string(), BODY_SIZE, TEXT)
    } else {
        text(label.to_string(), BODY_SIZE, TEXT_WEAK)
    };
    h_flex()
        .id(id)
        .w_full()
        .h(px(32.0))
        .px(px(8.0))
        .gap(px(10.0))
        .rounded(px(7.0))
        .cursor_pointer()
        .when(selected, |el| el.bg(Color32::from_rgb(0x33, 0x35, 0x3B)))
        .when(!selected, |el| el.hover(|s| s.bg(Color32::from_rgb(0x26, 0x28, 0x2C))))
        .child(glyph)
        .child(label)
        .on_click(on_click)
}

fn nav_glyph(painter: &mut Painter, rect: Rect, icon: NavIcon) {
    let c = rect.center();
    let white = Color32::WHITE;
    let w = 1.5;
    match icon {
        NavIcon::Speaker => {
            painter.polygon(
                &[
                    c + vec2(-6.0, -2.5),
                    c + vec2(-3.0, -2.5),
                    c + vec2(1.0, -6.0),
                    c + vec2(1.0, 6.0),
                    c + vec2(-3.0, 2.5),
                    c + vec2(-6.0, 2.5),
                ],
                white,
            );
            painter.arc(c + vec2(1.5, 0.0), 3.5, -0.9, 0.9, w, white);
            painter.arc(c + vec2(1.5, 0.0), 6.0, -0.9, 0.9, w, white);
        }
        NavIcon::Download => {
            painter.line(&[c + vec2(0.0, -6.0), c + vec2(0.0, 2.5)], w, white);
            painter.line(&[c + vec2(-3.5, -1.0), c + vec2(0.0, 2.5), c + vec2(3.5, -1.0)], w, white);
            painter.line(&[c + vec2(-5.5, 6.0), c + vec2(5.5, 6.0)], w, white);
        }
        NavIcon::Text => {}
        NavIcon::Keyboard => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(14.0, 9.0)), 2.0, 1.3, white);
            for x in [-4.0, -1.3, 1.3, 4.0] {
                painter.circle_filled(c + vec2(x, -1.5), 0.8, white);
            }
            painter.line(&[c + vec2(-3.0, 2.0), c + vec2(3.0, 2.0)], 1.2, white);
        }
        NavIcon::Camera => {
            painter.rect_stroke(Rect::from_center_size(c + vec2(0.0, 1.0), vec2(14.0, 10.0)), 2.0, 1.3, white);
            painter.rect_filled(Rect::from_center_size(c + vec2(-2.0, -4.5), vec2(5.0, 2.0)), 1.0, white);
            painter.circle_stroke(c + vec2(0.0, 1.0), 2.8, 1.3, white);
        }
        NavIcon::Info => {
            painter.circle_filled(c + vec2(0.0, -4.5), 1.4, white);
            painter.line(&[c + vec2(0.0, -1.5), c + vec2(0.0, 5.5)], 1.8, white);
        }
        NavIcon::Convert => {
            painter.line(&[c + vec2(-5.0, -3.0), c + vec2(5.0, -3.0)], w, white);
            painter.line(&[c + vec2(2.5, -5.5), c + vec2(5.0, -3.0), c + vec2(2.5, -0.5)], w, white);
            painter.line(&[c + vec2(5.0, 3.0), c + vec2(-5.0, 3.0)], w, white);
            painter.line(&[c + vec2(-2.5, 0.5), c + vec2(-5.0, 3.0), c + vec2(-2.5, 5.5)], w, white);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Zeichnen

pub fn to_point(p: Pos2) -> Point<Pixels> {
    point(px(p.x), px(p.y))
}

pub fn from_point(p: Point<Pixels>) -> Pos2 {
    pos2(p.x.as_f32(), p.y.as_f32())
}

pub fn to_bounds(r: Rect) -> Bounds<Pixels> {
    Bounds::new(to_point(r.min), gpui_kit::size(px(r.width().max(0.0)), px(r.height().max(0.0))))
}

pub fn to_rect(b: Bounds<Pixels>) -> Rect {
    Rect::from_min_size(from_point(b.origin), vec2(b.size.width.as_f32(), b.size.height.as_f32()))
}

/// Zeichnet Linien, Flächen und Bilder in Fensterkoordinaten (logische Pixel).
/// Nur in der Zeichenphase benutzen (`canvas`).
pub struct Painter<'a> {
    pub window: &'a mut Window,
}

impl<'a> Painter<'a> {
    pub fn new(window: &'a mut Window) -> Self {
        Self { window }
    }

    fn stroke_path(&mut self, points: &[Pos2], width: f32, color: Color32, closed: bool, dash: Option<(f32, f32)>) {
        if points.len() < 2 || color.a == 0 {
            return;
        }
        let mut options = StrokeOptions::default().with_line_width(width);
        options.start_cap = lyon_tessellation::LineCap::Round;
        options.end_cap = lyon_tessellation::LineCap::Round;
        options.line_join = lyon_tessellation::LineJoin::Round;
        let mut builder = PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(options));
        if let Some((on, off)) = dash {
            builder = builder.dash_array(&[px(on), px(off)]);
        }
        builder.move_to(to_point(points[0]));
        for p in &points[1..] {
            builder.line_to(to_point(*p));
        }
        if closed {
            builder.close();
        }
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, Hsla::from(color));
        }
    }

    /// Linienzug mit runden Enden.
    pub fn line(&mut self, points: &[Pos2], width: f32, color: Color32) {
        self.stroke_path(points, width, color, false, None);
    }

    pub fn closed_line(&mut self, points: &[Pos2], width: f32, color: Color32) {
        self.stroke_path(points, width, color, true, None);
    }

    pub fn dashed_line(&mut self, points: &[Pos2], width: f32, color: Color32, dash: f32, gap: f32) {
        self.stroke_path(points, width, color, false, Some((dash, gap)));
    }

    /// Gefüllte Fläche.
    pub fn polygon(&mut self, points: &[Pos2], color: Color32) {
        if points.len() < 3 || color.a == 0 {
            return;
        }
        let mut builder = PathBuilder::fill();
        builder.move_to(to_point(points[0]));
        for p in &points[1..] {
            builder.line_to(to_point(*p));
        }
        builder.close();
        if let Ok(path) = builder.build() {
            self.window.paint_path(path, Hsla::from(color));
        }
    }

    /// Kreisbogen von `from` bis `to` (Bogenmaß).
    pub fn arc(&mut self, c: Pos2, r: f32, from: f32, to: f32, width: f32, color: Color32) {
        let steps = 16;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let a = from + (to - from) * i as f32 / steps as f32;
                c + vec2(r * a.cos(), r * a.sin())
            })
            .collect();
        self.line(&points, width, color);
    }

    pub fn rect_filled(&mut self, rect: Rect, radius: f32, color: Color32) {
        if !rect.is_positive() || color.a == 0 {
            return;
        }
        self.window.paint_quad(fill(to_bounds(rect), color).corner_radii(px(radius)));
    }

    /// Rahmen, mittig auf der Kante von `rect`.
    pub fn rect_stroke(&mut self, rect: Rect, radius: f32, width: f32, color: Color32) {
        let outer = rect.expand(width / 2.0);
        if !outer.is_positive() || color.a == 0 {
            return;
        }
        self.window.paint_quad(quad(
            to_bounds(outer),
            px(radius + width / 2.0),
            Color32::TRANSPARENT,
            px(width),
            color,
            BorderStyle::Solid,
        ));
    }

    /// Rahmen außerhalb von `rect`.
    pub fn rect_stroke_outside(&mut self, rect: Rect, width: f32, color: Color32) {
        self.rect_stroke(rect.expand(width / 2.0), 0.0, width, color);
    }

    /// Rahmen innerhalb von `rect`.
    pub fn rect_stroke_inside(&mut self, rect: Rect, width: f32, color: Color32) {
        self.rect_stroke(rect.shrink(width / 2.0), 0.0, width, color);
    }

    pub fn circle_filled(&mut self, c: Pos2, r: f32, color: Color32) {
        self.rect_filled(Rect::from_center_size(c, vec2(r * 2.0, r * 2.0)), r, color);
    }

    pub fn circle_stroke(&mut self, c: Pos2, r: f32, width: f32, color: Color32) {
        self.rect_stroke(Rect::from_center_size(c, vec2(r * 2.0, r * 2.0)), r, width, color);
    }

    /// Bild in `image_rect` zeichnen, sichtbar nur innerhalb von `clip`.
    pub fn image(&mut self, clip: Rect, image_rect: Rect, image: &Arc<RenderImage>) {
        let _ = self.window.paint_image(
            to_bounds(clip),
            to_bounds(image_rect),
            Corners::default(),
            image.clone(),
            0,
            false,
        );
    }
}

/// Bild aus RGBA-Pixeln (nicht vormultipliziert) für GPUI (erwartet BGRA).
pub fn render_image(rgba: &image::RgbaImage) -> Arc<RenderImage> {
    let mut bgra = rgba.clone();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(image::Frame::new(bgra), 1)))
}

/// Wie [`render_image`], aber aus vormultiplizierten RGBA-Pixeln (tiny-skia).
pub fn render_premultiplied(width: u32, height: u32, data: &[u8]) -> Arc<RenderImage> {
    let mut out = Vec::with_capacity(data.len());
    for px in data.chunks_exact(4) {
        let a = px[3];
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        out.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), a]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, out).unwrap_or_else(|| image::RgbaImage::new(1, 1));
    Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(image::Frame::new(buffer), 1)))
}

/// Gibt die Grafikspeicher eines nicht mehr gebrauchten Bildes frei.
pub fn drop_image(image: Arc<RenderImage>, window: &mut Window) {
    let _ = window.drop_image(image);
}
