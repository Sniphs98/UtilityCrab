//! Gemeinsamer Baukasten für alle Fenster (Einstellungen, Konverter, Popup):
//! Systemschrift, Farben und Bausteine im Stil der macOS-Systemeinstellungen –
//! Seiten mit Titel, gruppierte Listen aus Zeilen (Beschriftung links, Bedienelement rechts),
//! Schalter statt Checkboxen, Tastenkappen.

use std::sync::Arc;

use eframe::egui::{
    self, vec2, Align, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Painter, Rect, Response, RichText,
    Sense, Shape, Stroke, Ui,
};

// ---------------------------------------------------------------------------------------------
// Farben

pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x1C);
pub const SIDEBAR_BG: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x1F);
pub const CONTENT_BG: Color32 = Color32::from_rgb(0x22, 0x23, 0x27);
pub const GROUP_BG: Color32 = Color32::from_rgb(0x2B, 0x2D, 0x31);
pub const FIELD_BG: Color32 = Color32::from_rgb(0x1C, 0x1D, 0x20);
pub const DIVIDER: Color32 = Color32::from_rgb(0x36, 0x38, 0x3E);
pub const TEXT: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF3);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0xA6, 0xAB, 0xB3);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x80, 0x85, 0x8E);
pub const GREEN: Color32 = Color32::from_rgb(0x4C, 0xC0, 0x6A);
pub const RED: Color32 = Color32::from_rgb(0xEE, 0x5D, 0x4E);

/// Farbe mit geringer Deckkraft, für getönte Hintergründe.
pub fn tint(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8)
}

// ---------------------------------------------------------------------------------------------
// Schrift

/// Name der Schriftfamilie für Hervorgehobenes (Segoe UI Semibold).
pub const SEMIBOLD: &str = "semibold";

pub const TITLE_SIZE: f32 = 20.0;
pub const BODY_SIZE: f32 = 13.5;
pub const SMALL_SIZE: f32 = 12.0;

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Lädt die Windows-Systemschrift (Segoe UI, dazu Semibold). Fehlt sie, bleibt egui's Schrift.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    let dir = std::path::PathBuf::from(windir).join("Fonts");
    let mut semibold = Vec::new();
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe-semibold", "seguisb.ttf")] {
        let Ok(bytes) = std::fs::read(dir.join(file)) else {
            continue;
        };
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
        if name == "segoe" {
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, name.into());
        } else {
            semibold.push(name.to_string());
        }
    }
    // Fehlt Semibold, wird es normal; die übrigen Schriften dienen als Ersatz für Sonderzeichen.
    semibold.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}

/// Schrift, Farben und Abstände für ein Fenster.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = CONTENT_BG;
        v.window_fill = GROUP_BG;
        v.window_stroke = Stroke::new(1.0, DIVIDER);
        v.extreme_bg_color = FIELD_BG;
        v.faint_bg_color = Color32::from_rgb(0x2F, 0x31, 0x36);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = tint(ACCENT, 0.55);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.slider_trailing_fill = true;
        v.popup_shadow = egui::Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(90),
        };
        for (widget, fill) in [
            (&mut v.widgets.inactive, Color32::from_rgb(0x3A, 0x3C, 0x42)),
            (&mut v.widgets.hovered, Color32::from_rgb(0x45, 0x48, 0x4F)),
            (&mut v.widgets.active, Color32::from_rgb(0x50, 0x53, 0x5A)),
            (&mut v.widgets.open, Color32::from_rgb(0x3A, 0x3C, 0x42)),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::NONE;
            widget.corner_radius = CornerRadius::same(6);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
        v.widgets.inactive.fg_stroke.color = TEXT;

        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 4.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.menu_margin = Margin::symmetric(6, 6);
        style.text_styles.insert(egui::TextStyle::Body, regular(BODY_SIZE));
        style.text_styles.insert(egui::TextStyle::Button, regular(BODY_SIZE));
        style.text_styles.insert(egui::TextStyle::Small, regular(SMALL_SIZE));
        style.text_styles.insert(egui::TextStyle::Heading, semibold(TITLE_SIZE));
        style.interaction.tooltip_delay = 0.3;
    });
}

// ---------------------------------------------------------------------------------------------
// Seiten, Gruppen, Zeilen

/// Seitentitel mit kurzer Erklärung darunter.
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(TITLE_SIZE)).color(TEXT));
    ui.add_space(2.0);
    ui.label(RichText::new(subtitle).font(regular(BODY_SIZE)).color(TEXT_WEAK));
    ui.add_space(18.0);
}

/// Gruppe von Zeilen mit optionaler Überschrift und Fußnote.
pub fn section(ui: &mut Ui, title: Option<&str>, footnote: Option<&str>, add: impl FnOnce(&mut Group)) {
    if let Some(title) = title {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(title).font(semibold(SMALL_SIZE)).color(TEXT_WEAK));
        });
        ui.add_space(2.0);
    }
    egui::Frame::new()
        .fill(GROUP_BG)
        .corner_radius(CornerRadius::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut group = Group { ui, rows: 0 };
            add(&mut group);
        });
    if let Some(footnote) = footnote {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(footnote).font(regular(SMALL_SIZE)).color(TEXT_FAINT)).wrap());
        });
    }
    ui.add_space(18.0);
}

pub struct Group<'a> {
    pub ui: &'a mut Ui,
    rows: usize,
}

impl Group<'_> {
    fn divider(&mut self) {
        if self.rows > 0 {
            let (rect, _) = self.ui.allocate_exact_size(vec2(self.ui.available_width(), 1.0), Sense::hover());
            self.ui.painter().line_segment(
                [rect.left_center() + vec2(14.0, 0.0), rect.right_center()],
                Stroke::new(1.0, DIVIDER),
            );
        }
        self.rows += 1;
    }

    /// Zeile: Beschriftung (und Erklärung) links, Bedienelement rechtsbündig.
    pub fn row(&mut self, label: &str, description: Option<&str>, control: impl FnOnce(&mut Ui)) {
        self.divider();
        row_frame(self.ui, |ui| {
            ui.horizontal(|ui| {
                let total = ui.available_width();
                let left = (total * 0.56).min(total - 140.0).max(120.0);
                // Mindesthöhe wie ein Bedienelement, darin senkrecht zentriert: Einzeilige
                // Beschriftungen stehen so auf einer Linie mit dem Element rechts.
                let height = ui.spacing().interact_size.y + 2.0;
                ui.allocate_ui_with_layout(vec2(left, height), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(left);
                    ui.set_min_height(height);
                    if description.is_none() {
                        ui.add_space(((height - ui.text_style_height(&egui::TextStyle::Body)) / 2.0).max(0.0));
                    }
                    label_block(ui, label, description);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), control);
            });
        });
    }

    /// Auswahlzeile mit Häkchen rechts (wie eine Liste in den Systemeinstellungen).
    pub fn choice(&mut self, label: &str, description: Option<&str>, selected: bool) -> bool {
        self.divider();
        let response = row_frame(self.ui, |ui| {
            ui.horizontal(|ui| {
                let left = ui.available_width() - 28.0;
                let height = ui.spacing().interact_size.y + 2.0;
                ui.allocate_ui_with_layout(vec2(left, height), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(left);
                    ui.set_min_height(height);
                    if description.is_none() {
                        ui.add_space(((height - ui.text_style_height(&egui::TextStyle::Body)) / 2.0).max(0.0));
                    }
                    label_block(ui, label, description);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    if selected {
                        checkmark(ui.painter(), rect, ACCENT);
                    }
                });
            });
        });
        response
            .interact(Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    }

    /// Beliebiger Inhalt über die ganze Breite (z. B. ein Textfeld).
    pub fn custom(&mut self, add: impl FnOnce(&mut Ui)) {
        self.divider();
        row_frame(self.ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            add(ui);
        });
    }
}

fn row_frame(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> Response {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(26.0);
            add(ui);
        })
        .response
}

fn label_block(ui: &mut Ui, label: &str, description: Option<&str>) {
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.add(egui::Label::new(RichText::new(label).font(regular(BODY_SIZE)).color(TEXT)).wrap());
    if let Some(description) = description {
        ui.add(egui::Label::new(RichText::new(description).font(regular(SMALL_SIZE)).color(TEXT_WEAK)).wrap());
    }
}

// ---------------------------------------------------------------------------------------------
// Bedienelemente

/// Schalter (An/Aus) mit kurzer Animation.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let off = Color32::from_rgb(0x4A, 0x4D, 0x55);
    let track = lerp_color(off, ACCENT, t);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    painter.circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Hauptaktion (Akzentfarbe).
pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).font(regular(BODY_SIZE)).color(Color32::WHITE)).fill(ACCENT)
}

/// Aktion, die Daten löscht.
pub fn danger_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).font(regular(BODY_SIZE)).color(Color32::WHITE)).fill(Color32::from_rgb(0xB8, 0x3A, 0x2E))
}

/// Tastenkürzel als einzelne Tastenkappen, z. B. [Strg] [Alt] [S].
pub fn keycaps(ui: &mut Ui, label: &str) {
    // In einem rechtsbündigen Bereich wird von rechts nach links gelegt – dann umdrehen,
    // damit es „Strg Alt S“ bleibt.
    let mut keys: Vec<&str> = label.split('+').collect();
    if ui.layout().prefer_right_to_left() {
        keys.reverse();
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for key in keys {
            let galley = ui.painter().layout_no_wrap(key.to_string(), semibold(SMALL_SIZE), TEXT);
            let size = vec2((galley.size().x + 12.0).max(24.0), 22.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect.translate(vec2(0.0, 1.0)), CornerRadius::same(5), Color32::from_rgb(0x18, 0x19, 0x1C));
            painter.rect_filled(rect, CornerRadius::same(5), Color32::from_rgb(0x3D, 0x40, 0x46));
            painter.galley(rect.center() - galley.size() / 2.0, galley, TEXT);
        }
    });
}

/// Rot getönter Hinweis bei Fehlern.
pub fn error_banner(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(tint(RED, 0.14))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).font(regular(BODY_SIZE)).color(Color32::from_rgb(0xFF, 0xB4, 0xA8))).wrap());
        });
}

pub fn checkmark(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width() / 16.0;
    let points = vec![c + vec2(-5.0, 0.5) * s, c + vec2(-1.5, 4.0) * s, c + vec2(5.5, -4.0) * s];
    painter.add(Shape::line(points, Stroke::new(2.0 * s, color)));
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
    Info,
}

/// Eintrag in der Seitenleiste. Gibt `true` zurück, wenn er angeklickt wurde.
pub fn nav_item(ui: &mut Ui, selected: bool, icon: NavIcon, color: Color32, label: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(7), Color32::from_rgb(0x33, 0x35, 0x3B));
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(7), Color32::from_rgb(0x26, 0x28, 0x2C));
    }
    let icon_rect = Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(20.0, 20.0));
    painter.rect_filled(icon_rect, CornerRadius::same(5), color);
    nav_glyph(painter, icon_rect, icon);
    let font = if selected { semibold(BODY_SIZE) } else { regular(BODY_SIZE) };
    painter.text(
        rect.left_center() + vec2(36.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        if selected { TEXT } else { TEXT_WEAK },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn nav_glyph(painter: &Painter, rect: Rect, icon: NavIcon) {
    let c = rect.center();
    let white = Color32::WHITE;
    let stroke = Stroke::new(1.5, white);
    match icon {
        NavIcon::Speaker => {
            let body = vec![
                c + vec2(-6.0, -2.5),
                c + vec2(-3.0, -2.5),
                c + vec2(1.0, -6.0),
                c + vec2(1.0, 6.0),
                c + vec2(-3.0, 2.5),
                c + vec2(-6.0, 2.5),
            ];
            painter.add(Shape::convex_polygon(body, white, Stroke::NONE));
            arc(painter, c + vec2(1.5, 0.0), 3.5, stroke);
            arc(painter, c + vec2(1.5, 0.0), 6.0, stroke);
        }
        NavIcon::Download => {
            painter.line_segment([c + vec2(0.0, -6.0), c + vec2(0.0, 2.5)], stroke);
            painter.add(Shape::line(
                vec![c + vec2(-3.5, -1.0), c + vec2(0.0, 2.5), c + vec2(3.5, -1.0)],
                stroke,
            ));
            painter.line_segment([c + vec2(-5.5, 6.0), c + vec2(5.5, 6.0)], stroke);
        }
        NavIcon::Text => {
            painter.text(c + vec2(0.0, 0.5), egui::Align2::CENTER_CENTER, "Aa", semibold(11.0), white);
        }
        NavIcon::Keyboard => {
            let body = Rect::from_center_size(c, vec2(14.0, 9.0));
            painter.rect_stroke(body, CornerRadius::same(2), Stroke::new(1.3, white), egui::StrokeKind::Middle);
            for x in [-4.0, -1.3, 1.3, 4.0] {
                painter.circle_filled(c + vec2(x, -1.5), 0.8, white);
            }
            painter.line_segment([c + vec2(-3.0, 2.0), c + vec2(3.0, 2.0)], Stroke::new(1.2, white));
        }
        NavIcon::Info => {
            painter.circle_filled(c + vec2(0.0, -4.5), 1.4, white);
            painter.line_segment([c + vec2(0.0, -1.5), c + vec2(0.0, 5.5)], Stroke::new(1.8, white));
        }
        NavIcon::Convert => {
            painter.line_segment([c + vec2(-5.0, -3.0), c + vec2(5.0, -3.0)], stroke);
            painter.add(Shape::line(vec![c + vec2(2.5, -5.5), c + vec2(5.0, -3.0), c + vec2(2.5, -0.5)], stroke));
            painter.line_segment([c + vec2(5.0, 3.0), c + vec2(-5.0, 3.0)], stroke);
            painter.add(Shape::line(vec![c + vec2(-2.5, 0.5), c + vec2(-5.0, 3.0), c + vec2(-2.5, 5.5)], stroke));
        }
    }
}

/// Bogen nach rechts (Schallwellen des Lautsprechers).
fn arc(painter: &Painter, c: egui::Pos2, r: f32, stroke: Stroke) {
    let points = (0..=12)
        .map(|i| {
            let a = -0.9 + 1.8 * i as f32 / 12.0;
            c + vec2(r * a.cos(), r * a.sin())
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}
