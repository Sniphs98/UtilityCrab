//! Baut die Karte eines Hinweises.
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

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::icons;
use super::model::{Block, Icon, Kind, Notice};
use super::theme::*;
use super::Popup;
use crate::geom::{vec2, Color32};
use crate::ui_kit::painted;

/// Was rechts in der Titelzeile steht.
enum Trailing {
    Badge { text: String, color: Color32, hint: Option<String> },
    Percent,
}

/// Alles, was die Karte außer dem Hinweis braucht.
pub struct CardState {
    pub details_open: bool,
    /// Angezeigter Fortschritt (läuft dem echten weich hinterher).
    pub shown_fraction: f32,
    /// Verbleibender Anteil des Timers (für die Leiste unten).
    pub remaining: Option<f32>,
    /// Sekunden seit dem Öffnen, für den Drehkreis.
    pub time: f64,
    /// Hierhin schreibt die Karte ihre gemessene Höhe (für die Fenstergröße).
    pub measured_height: Rc<Cell<f32>>,
}

fn regular(content: impl Into<SharedString>, size: f32, color: Color32) -> Div {
    div().text_size(px(size)).text_color(color).child(content.into())
}

fn semibold(content: impl Into<SharedString>, size: f32, color: Color32) -> Div {
    regular(content, size, color).font_weight(FontWeight::SEMIBOLD)
}

/// Die ganze Karte.
pub fn card(notice: &Notice, state: &CardState, cx: &mut Context<Popup>) -> Div {
    let trailing = notice.blocks.iter().find_map(|b| match b {
        Block::Stat {
            badge: Some((text, tone)),
            badge_hint,
            ..
        } => Some(Trailing::Badge {
            text: text.clone(),
            color: tone_color(*tone),
            hint: badge_hint.clone(),
        }),
        Block::Progress { .. } => Some(Trailing::Percent),
        _ => None,
    });
    let chips: Vec<(Icon, String)> = notice
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Chip { icon, text } => Some((*icon, text.clone())),
            _ => None,
        })
        .collect();

    let mut body = v_flex().w_full().pl(px(INDENT)).pr(px(4.0));
    for block in notice.blocks.iter().filter(|b| !matches!(b, Block::Chip { .. })) {
        body = body.child(div().mt(px(GAP)).child(draw_block(block, state, cx)));
    }
    if notice.action.is_some() || !chips.is_empty() {
        body = body.child(footer(notice, chips, cx));
    }

    let measured = state.measured_height.clone();
    let timer = state.remaining.map(|r| r.clamp(0.0, 1.0));
    let timer_color = tint(kind_color(notice.kind), 0.5);

    v_flex()
        .relative()
        .w(px(CARD_WIDTH))
        .bg(CARD_BG)
        .pl(px(PAD_X))
        .pr(px(PAD_X - 4.0))
        .py(px(PAD_Y))
        .font_family(crate::ui_kit::FONT)
        .child(header(notice, trailing, state, cx))
        .child(body)
        // Höhe messen, damit das Fenster genau die Karte ist.
        .child(
            canvas(move |bounds, _, _| measured.set(bounds.size.height.as_f32()), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
        // Timer-Leiste ganz unten: läuft von voll nach leer.
        .when_some(timer, |el, remaining| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(2.0))
                    .w(px(CARD_WIDTH * remaining))
                    .bg(timer_color),
            )
        })
}

fn header(notice: &Notice, trailing: Option<Trailing>, state: &CardState, cx: &mut Context<Popup>) -> Div {
    let title_height = TITLE_SIZE * 1.35;
    // Titel, Badge und ✕ auf Höhe der Symbolmitte, solange es nur eine Zeile ist.
    let line_offset = ((ICON_SIZE - title_height) / 2.0).max(0.0);
    let first_line = if notice.subtitle.is_some() { 0.0 } else { line_offset };
    let close_size = 20.0;

    let kind: Kind = notice.kind;
    let time = state.time;
    let icon = painted(move |p, rect| icons::status(p, rect, kind, time)).size(px(ICON_SIZE)).flex_none();

    let titles = v_flex()
        .flex_1()
        .min_w_0()
        .ml(px(INDENT - ICON_SIZE))
        .pt(px(first_line))
        .child(semibold(notice.title.clone(), TITLE_SIZE, TEXT).line_height(px(title_height)))
        .when_some(notice.subtitle.clone(), |el, subtitle| {
            el.child(regular(subtitle, SMALL_SIZE, TEXT_WEAK).mt(px(1.0)).truncate())
        });

    let trailing = match trailing {
        Some(Trailing::Badge { text, color, hint }) => {
            let pill = div()
                .id("badge")
                .px(px(7.0))
                .py(px(2.0))
                .rounded_full()
                .bg(tint(color, 0.15))
                .child(semibold(text, SMALL_SIZE, color))
                .when_some(hint, |el, hint| {
                    el.cursor(CursorStyle::Arrow)
                        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                });
            Some(div().flex_none().ml(px(6.0)).pt(px(first_line + 1.0)).child(pill))
        }
        Some(Trailing::Percent) => Some(
            div()
                .flex_none()
                .ml(px(6.0))
                .pt(px(first_line + 1.0))
                .child(regular(format!("{:.0} %", state.shown_fraction * 100.0), SMALL_SIZE, TEXT_WEAK)),
        ),
        None => None,
    };

    let close = div()
        .id("schliessen")
        .flex_none()
        .ml(px(6.0))
        .mt(px((first_line + title_height / 2.0 - close_size / 2.0).max(0.0)))
        .size(px(close_size))
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(tint(TEXT, 0.09)))
        .child(painted(|p, rect| icons::cross(p, rect.center(), 3.5, 1.4, TEXT_WEAK)).size_full())
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close(window, cx)));

    h_flex()
        .w_full()
        .items_start()
        .child(icon)
        .child(titles)
        .children(trailing)
        .child(close)
}

fn draw_block(block: &Block, state: &CardState, cx: &mut Context<Popup>) -> AnyElement {
    match block {
        Block::Progress { label, .. } => {
            let shown = state.shown_fraction.clamp(0.0, 1.0);
            v_flex()
                .w_full()
                .pt(px(2.0))
                .child(
                    div().w_full().h(px(4.0)).rounded(px(2.0)).bg(TRACK).child(
                        div()
                            .h_full()
                            .rounded(px(2.0))
                            .bg(ACCENT)
                            .w(relative(shown))
                            .min_w(px(if shown > 0.0 { 4.0 } else { 0.0 })),
                    ),
                )
                .child(regular(label.clone(), SMALL_SIZE, TEXT_FAINT).mt(px(5.0)).truncate())
                .into_any_element()
        }
        Block::Stat { before, after, .. } => h_flex()
            .gap(px(5.0))
            .child(regular(before.clone(), BODY_SIZE, TEXT_WEAK))
            .child(painted(|p, rect| icons::arrow(p, rect.center() + vec2(0.0, 1.0), 11.0, TEXT_FAINT)).w(px(14.0)).h(px(12.0)))
            .child(semibold(after.clone(), BODY_SIZE, TEXT))
            .into_any_element(),
        Block::Text(text) => regular(text.clone(), BODY_SIZE, TEXT).into_any_element(),
        Block::Quote(text) => div()
            .mt(px(2.0))
            .w_full()
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(tint(TEXT, 0.05))
            .child(regular(shorten(text, 3, 150), SMALL_SIZE, TEXT))
            .into_any_element(),
        Block::Hint(text) => regular(text.clone(), SMALL_SIZE, TEXT_FAINT).into_any_element(),
        Block::Chip { .. } => div().into_any_element(),
        Block::Details { label, items } => {
            let open = state.details_open;
            v_flex()
                .child(
                    h_flex()
                        .id("details")
                        .gap(px(3.0))
                        .cursor_pointer()
                        .child(regular(label.clone(), SMALL_SIZE, TEXT_WEAK))
                        .child(
                            painted(move |p, rect| icons::chevron(p, rect.center() + vec2(0.0, 1.0), open, TEXT_WEAK))
                                .size(px(10.0)),
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.details_open = !this.details_open;
                            cx.notify();
                        })),
                )
                .when(open, |el| {
                    el.children(items.iter().map(|item| regular(item.clone(), SMALL_SIZE, TEXT_FAINT).mt(px(3.0))))
                })
                .into_any_element()
        }
    }
}

/// Knopf links, Nebeninfos (z. B. Zwischenablage) klein und blass rechts.
fn footer(notice: &Notice, chips: Vec<(Icon, String)>, cx: &mut Context<Popup>) -> Div {
    let action = notice.action.as_ref().map(|action| {
        div()
            .id("aktion")
            .h(px(24.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .bg(tint(TEXT, 0.09))
            .hover(|s| s.bg(tint(TEXT, 0.14)))
            .cursor_pointer()
            .child(regular(action.label().to_string(), SMALL_SIZE, TEXT))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.run_action(window, cx)))
    });
    h_flex()
        .mt(px(FOOTER_GAP))
        .w_full()
        .items_center()
        .children(action)
        .child(div().flex_1())
        .children(chips.into_iter().map(|(icon, text)| {
            h_flex()
                .ml(px(8.0))
                .gap(px(4.0))
                .child(
                    painted(move |p, rect| match icon {
                        Icon::Clipboard => icons::clipboard(p, rect.center(), TEXT_FAINT, 0.8),
                        Icon::Info => icons::info(p, rect.center(), TEXT_FAINT),
                    })
                    .size(px(12.0)),
                )
                .child(regular(text, SMALL_SIZE - 0.5, TEXT_FAINT))
        }))
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
