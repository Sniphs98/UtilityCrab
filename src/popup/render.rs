//! Zeichnet die Karte eines Hinweises.
//!
//! Aufbau (kompakt, wie eine macOS-Mitteilung):
//! ```text
//! (✓) Titel (Semibold)                  [−75 %]  ✕
//!     Untertitel
//!     Inhalt: 1,7 MB → 420 KB / Fortschritt / Text …
//!     [Knopf]                      ⧉ In Zwischenablage
//! ```
//! Das Badge bzw. die Prozentzahl steht in der Titelzeile, Nebensächliches (Zwischenablage)
//! klein und blass in der Fußzeile. Erklärungen zum Badge erscheinen beim Darüberfahren.

use eframe::egui::{self, vec2, Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui};

use super::icons;
use super::model::{Block, Icon, Notice};
use super::theme::*;

/// Was der Benutzer in diesem Bild angeklickt hat.
#[derive(Default)]
pub struct Events {
    pub close: bool,
    pub action: bool,
}

/// Zustand, der nur im Fenster lebt.
#[derive(Default)]
pub struct ViewState {
    pub details_open: bool,
}

/// Was rechts in der Titelzeile steht.
enum Trailing<'a> {
    Badge { text: &'a str, color: Color32, hint: Option<&'a str> },
    Percent(f32),
}

/// Zeichnet die Karte und gibt ihr Rechteck zurück (zum Bemessen des Fensters).
/// `remaining`: verbleibender Anteil des Timers (für die Leiste unten), `time` für Animationen.
pub fn card(ui: &mut Ui, notice: &Notice, state: &mut ViewState, remaining: Option<f32>, time: f64) -> (Rect, Events) {
    let mut events = Events::default();
    let frame = egui::Frame::new().fill(CARD_BG).inner_margin(Margin {
        left: PAD_X as i8,
        right: (PAD_X - 4.0) as i8,
        top: PAD_Y as i8,
        bottom: PAD_Y as i8,
    });

    let trailing = notice.blocks.iter().find_map(|b| match b {
        Block::Stat {
            badge: Some((text, tone)),
            badge_hint,
            ..
        } => Some(Trailing::Badge {
            text,
            color: tone_color(*tone),
            hint: badge_hint.as_deref(),
        }),
        Block::Progress { fraction, .. } => Some(Trailing::Percent(*fraction)),
        _ => None,
    });
    let chips: Vec<(&Icon, &str)> = notice
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Chip { icon, text } => Some((icon, text.as_str())),
            _ => None,
        })
        .collect();
    let body: Vec<&Block> = notice.blocks.iter().filter(|b| !matches!(b, Block::Chip { .. })).collect();

    let response = frame.show(ui, |ui| {
        ui.set_width(CARD_WIDTH - PAD_X - (PAD_X - 4.0));
        ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

        header(ui, notice, trailing, time, &mut events);

        indented(ui, |ui| {
            for block in &body {
                ui.add_space(GAP);
                draw_block(ui, block, state);
            }
            if notice.action.is_some() || !chips.is_empty() {
                ui.add_space(FOOTER_GAP);
                footer(ui, notice, &chips, &mut events);
            }
        });
    });
    let rect = response.response.rect;

    // Timer-Leiste ganz unten: läuft von voll nach leer.
    if let Some(remaining) = remaining {
        let bar = Rect::from_min_size(
            egui::pos2(rect.left(), rect.bottom() - 2.0),
            vec2(rect.width() * remaining.clamp(0.0, 1.0), 2.0),
        );
        ui.painter().rect_filled(bar, CornerRadius::ZERO, tint(kind_color(notice.kind), 0.5));
    }
    (rect, events)
}

fn header(ui: &mut Ui, notice: &Notice, trailing: Option<Trailing>, time: f64, events: &mut Events) {
    let title_height = TITLE_SIZE * 1.35;
    // Titel, Badge und ✕ auf Höhe der Symbolmitte, solange es nur eine Zeile ist.
    let line_offset = ((ICON_SIZE - title_height) / 2.0).max(0.0);

    ui.horizontal_top(|ui| {
        // Abstände explizit (Elementabstand 0), damit der Titel exakt bei `INDENT` beginnt –
        // genau wie der Inhalt darunter.
        ui.spacing_mut().item_spacing.x = 0.0;
        let (icon_rect, _) = ui.allocate_exact_size(vec2(ICON_SIZE, ICON_SIZE), Sense::hover());
        icons::status(ui.painter(), icon_rect, notice.kind, time);
        gap(ui, INDENT - ICON_SIZE);

        // Rechts: ✕ und davor Badge bzw. Prozentzahl, rechtsbündig.
        let close_size = 20.0;
        let trailing_width = match &trailing {
            Some(Trailing::Badge { text, .. }) => pill_size(ui, text).x + 8.0,
            Some(Trailing::Percent(_)) => 44.0,
            None => 0.0,
        };
        let text_width = ui.available_width() - trailing_width - close_size - 12.0;

        ui.vertical(|ui| {
            ui.set_width(text_width);
            ui.add_space(if notice.subtitle.is_some() { 0.0 } else { line_offset });
            ui.add(egui::Label::new(RichText::new(&notice.title).font(semibold(TITLE_SIZE)).color(TEXT)).wrap());
            if let Some(subtitle) = &notice.subtitle {
                ui.add_space(1.0);
                ui.add(egui::Label::new(RichText::new(subtitle).font(regular(SMALL_SIZE)).color(TEXT_WEAK)).truncate());
            }
        });

        let first_line = if notice.subtitle.is_some() { 0.0 } else { line_offset };
        gap(ui, 6.0);
        ui.vertical(|ui| {
            ui.set_width(trailing_width);
            match trailing {
                Some(Trailing::Badge { text, color, hint }) => {
                    ui.add_space(first_line + 1.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        let response = pill(ui, text, color);
                        if let Some(hint) = hint {
                            response.on_hover_cursor(egui::CursorIcon::Help).on_hover_ui(|ui| {
                                ui.set_max_width(250.0);
                                ui.label(RichText::new(hint).font(regular(SMALL_SIZE)).color(TEXT));
                            });
                        }
                    });
                }
                Some(Trailing::Percent(fraction)) => {
                    let shown = ui
                        .ctx()
                        .animate_value_with_time(egui::Id::unique("fortschritt"), fraction.clamp(0.0, 1.0), 0.25);
                    ui.add_space(first_line + 1.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0} %", shown * 100.0))
                                .font(regular(SMALL_SIZE))
                                .color(TEXT_WEAK),
                        );
                    });
                }
                None => {}
            }
        });

        gap(ui, 6.0);
        ui.vertical(|ui| {
            ui.add_space((first_line + title_height / 2.0 - close_size / 2.0).max(0.0));
            if close_button(ui, close_size) {
                events.close = true;
            }
        });
    });
}

fn close_button(ui: &mut Ui, size: f32) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter().circle_filled(rect.center(), size / 2.0, tint(TEXT, 0.09));
    }
    let color = if hovered { TEXT } else { TEXT_FAINT };
    icons::cross(ui.painter(), rect.center(), 3.5, Stroke::new(1.4, color));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Inhalt unter dem Titel, bündig mit dem Titeltext statt mit dem Symbol.
fn indented(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        gap(ui, INDENT);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.set_width(ui.available_width() - 4.0);
            add(ui);
        });
    });
}

/// Fester waagrechter Abstand (unabhängig vom Elementabstand).
fn gap(ui: &mut Ui, width: f32) {
    ui.allocate_exact_size(vec2(width, 0.0), Sense::hover());
}

fn draw_block(ui: &mut Ui, block: &Block, state: &mut ViewState) {
    match block {
        Block::Progress { fraction, label } => {
            let shown = ui
                .ctx()
                .animate_value_with_time(egui::Id::unique("fortschritt"), fraction.clamp(0.0, 1.0), 0.25);
            ui.add_space(2.0);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, CornerRadius::same(2), TRACK);
            if shown > 0.0 {
                let fill = Rect::from_min_size(rect.min, vec2((rect.width() * shown).max(4.0), rect.height()));
                painter.rect_filled(fill, CornerRadius::same(2), ACCENT);
            }
            ui.add_space(5.0);
            ui.add(egui::Label::new(RichText::new(label).font(regular(SMALL_SIZE)).color(TEXT_FAINT)).truncate());
        }
        Block::Stat { before, after, .. } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.label(RichText::new(before).font(regular(BODY_SIZE)).color(TEXT_WEAK));
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 12.0), Sense::hover());
                icons::arrow(ui.painter(), rect.center() + vec2(0.0, 1.0), 11.0, TEXT_FAINT);
                ui.label(RichText::new(after).font(semibold(BODY_SIZE)).color(TEXT));
            });
        }
        Block::Text(text) => {
            ui.add(egui::Label::new(RichText::new(text).font(regular(BODY_SIZE)).color(TEXT)).wrap());
        }
        Block::Quote(text) => {
            ui.add_space(2.0);
            egui::Frame::new()
                .fill(tint(TEXT, 0.05))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(RichText::new(shorten(text, 3, 150)).font(regular(SMALL_SIZE)).color(TEXT))
                            .wrap(),
                    );
                });
        }
        Block::Hint(text) => {
            ui.add(egui::Label::new(RichText::new(text).font(regular(SMALL_SIZE)).color(TEXT_FAINT)).wrap());
        }
        Block::Chip { .. } => {}
        Block::Details { label, items } => {
            let response = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(RichText::new(label).font(regular(SMALL_SIZE)).color(TEXT_WEAK));
                    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    icons::chevron(ui.painter(), rect.center() + vec2(0.0, 1.0), state.details_open, TEXT_WEAK);
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                state.details_open = !state.details_open;
            }
            if state.details_open {
                for item in items {
                    ui.add_space(3.0);
                    ui.add(egui::Label::new(RichText::new(item).font(regular(SMALL_SIZE)).color(TEXT_FAINT)).wrap());
                }
            }
        }
    }
}

/// Knopf links, Nebeninfos (z. B. Zwischenablage) klein und blass rechts.
fn footer(ui: &mut Ui, notice: &Notice, chips: &[(&Icon, &str)], events: &mut Events) {
    ui.horizontal(|ui| {
        if let Some(action) = &notice.action {
            let button = egui::Button::new(RichText::new(action.label()).font(regular(SMALL_SIZE)).color(TEXT))
                .fill(tint(TEXT, 0.09))
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(6))
                .min_size(vec2(0.0, 24.0));
            if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                events.action = true;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (icon, text) in chips {
                ui.label(RichText::new(*text).font(regular(SMALL_SIZE - 0.5)).color(TEXT_FAINT));
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                match icon {
                    Icon::Clipboard => icons::clipboard(ui.painter(), rect.center(), TEXT_FAINT, 0.8),
                    Icon::Info => icons::info(ui.painter(), rect.center(), TEXT_FAINT),
                }
            }
        });
    });
}

fn pill_size(ui: &Ui, text: &str) -> egui::Vec2 {
    let galley = ui.painter().layout_no_wrap(text.to_string(), semibold(SMALL_SIZE), TEXT);
    galley.size() + vec2(14.0, 4.0)
}

/// Abgerundetes Badge mit getöntem Hintergrund.
fn pill(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), semibold(SMALL_SIZE), color);
    let size = galley.size() + vec2(14.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same((size.y / 2.0) as u8), tint(color, 0.15));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// Kürzt auf höchstens `lines` Zeilen und `chars` Zeichen, mit „…“.
fn shorten(text: &str, lines: usize, chars: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut out = all.iter().take(lines).copied().collect::<Vec<_>>().join("\n");
    let mut cut = all.len() > lines;
    if out.chars().count() > chars {
        out = out.chars().take(chars).collect::<String>().trim_end().to_string();
        cut = true;
    }
    if cut {
        out.push('…');
    }
    out
}
