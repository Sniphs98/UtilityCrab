//! Das Fenster der Schnellsteuerung. Sieht aus wie eine Hinweis-Karte:
//!
//! ```text
//! (⏸)  Vorlesen                                ■  ✕
//!      „Anfang des Textes …“
//!  🔊  ───────●────────────  70 %              ⚙
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use windows::Win32::Foundation::HWND;

use super::{Control, Link, State};
use crate::geom::{vec2, Color32, Pos2, Rect};
use crate::popup::theme::*;
use crate::popup::{place, style_window, Anchor};
use crate::ui_kit::{painted, Painter};

const WIDTH: f32 = 340.0;
const PLAY_SIZE: f32 = 34.0;
const BUTTON_SIZE: f32 = 26.0;
/// Text und Lautstärke beginnen bündig rechts vom großen Knopf.
const TEXT_INDENT: f32 = PLAY_SIZE + 12.0;
/// Lautstärke-Schritt pro Mausrad-Raste.
const WHEEL_STEP: f32 = 0.05;

/// Öffnet die Steuerung und blockiert, bis sie geschlossen ist. Muss auf dem Haupt-Thread laufen.
pub(super) fn show(link: Link) -> Result<(), String> {
    let opened = Rc::new(Cell::new(false));
    let opened_for_app = opened.clone();
    crate::ui_kit::run_app(move |cx| {
        let bounds = Bounds::new(point(px(-32000.0), px(-32000.0)), size(px(WIDTH), px(90.0)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            inactive_frame_interval: None,
            app_id: Some("UtilityCrab-Wiedergabe".into()),
            ..Default::default()
        };
        let link = link.clone();
        let result = crate::ui_kit::open_window(options, cx, move |window, cx| {
            let hwnd = crate::ui_kit::hwnd(window);
            if let Some(hwnd) = hwnd {
                style_window(hwnd);
            }
            cx.new(|cx| Controls::new(link, hwnd, window, cx))
        });
        match result {
            Some(_) => opened_for_app.set(true),
            None => cx.quit(),
        }
    });
    if opened.get() {
        Ok(())
    } else {
        Err("Schnellsteuerung konnte nicht geöffnet werden".into())
    }
}

struct Controls {
    link: Link,
    hwnd: Option<HWND>,
    state: State,
    version: u64,
    volume: Entity<SliderState>,
    placed_height: Option<f32>,
    measured_height: Rc<Cell<f32>>,
    appear: f32,
    last_tick: Instant,
    closing: bool,
    _subscription: Subscription,
    _ticker: Task<()>,
}

impl Controls {
    fn new(link: Link, hwnd: Option<HWND>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (state, version, _) = link.snapshot();
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(state.volume)
        });
        // Die Steuerung ist für die Lautstärke maßgeblich, solange sie offen ist: Der Tray
        // meldet nur zurück, was sie geschickt hat. So springt der Regler beim Ziehen nicht.
        let subscription = cx.subscribe_in(&volume, window, |this, _, event: &SliderEvent, _, cx| {
            let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
            this.set_volume(value.end(), cx);
        });
        let ticker = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                break;
            }
        });
        Self {
            link,
            hwnd,
            state,
            version,
            volume,
            placed_height: if hwnd.is_some() { None } else { Some(0.0) },
            measured_height: Rc::new(Cell::new(0.0)),
            appear: 0.0,
            last_tick: Instant::now(),
            closing: false,
            _subscription: subscription,
            _ticker: ticker,
        }
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let (state, version, ended) = self.link.snapshot();
        if ended {
            self.close(window, cx);
            return;
        }
        let mut changed = false;
        if version != self.version {
            self.version = version;
            // Lautstärke bleibt die eigene (siehe oben), alles andere kommt vom Tray.
            self.state = State {
                volume: self.state.volume,
                ..state
            };
            changed = true;
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        let height = self.measured_height.get().ceil();
        if let Some(hwnd) = self.hwnd {
            if height > 0.0 && self.placed_height.is_none_or(|h| (h - height).abs() > 0.5) {
                place(hwnd, vec2(WIDTH, height), Anchor::BottomRight);
                self.placed_height = Some(height);
            }
        }
        let before = self.appear;
        if self.placed_height.is_some() {
            self.appear = (self.appear + dt / 0.15).min(1.0);
        }
        if changed || before != self.appear {
            cx.notify();
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        window.remove_window();
        cx.quit();
    }

    fn send(&mut self, control: Control, cx: &mut Context<Self>) {
        // Sofort anzeigen, der Tray bestätigt kurz danach.
        match control {
            Control::TogglePause => self.state.paused = !self.state.paused,
            Control::SetMuted(muted) => self.state.muted = muted,
            _ => {}
        }
        self.link.send(control);
        cx.notify();
    }

    fn set_volume(&mut self, level: f32, cx: &mut Context<Self>) {
        let level = (level.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        if (level - self.state.volume).abs() < 0.005 && !(self.state.muted && level > 0.0) {
            return;
        }
        self.state.volume = level;
        self.link.send(Control::SetVolume(level));
        // Lauter drehen hebt die Stummschaltung auf – sonst wirkt der Regler kaputt.
        if self.state.muted && level > 0.0 {
            self.send(Control::SetMuted(false), cx);
        }
        cx.notify();
    }

    /// Mausrad über der Karte ändert die Lautstärke.
    fn scroll_volume(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let dy = event.delta.pixel_delta(px(20.0)).y.as_f32();
        if dy.abs() < 0.5 {
            return;
        }
        let steps = (dy / 20.0).clamp(-3.0, 3.0);
        let level = (self.state.volume + steps * WHEEL_STEP).clamp(0.0, 1.0);
        self.volume.update(cx, |slider, cx| slider.set_value(level, window, cx));
        self.set_volume(level, cx);
    }
}

impl Render for Controls {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let paused = state.paused;
        let muted = state.muted;

        let play = div()
            .id("pause")
            .flex_none()
            .size(px(PLAY_SIZE))
            .rounded_full()
            .bg(ACCENT)
            .hover(|s| s.opacity(0.88))
            .cursor_pointer()
            .child(painted(move |p, rect| play_pause(p, rect, paused, TEXT)).size_full())
            .tooltip(move |window, cx| Tooltip::new(if paused { "Weiter" } else { "Pause" }).build(window, cx))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(Control::TogglePause, cx)));

        let title = if paused { "Angehalten" } else { "Vorlesen" };
        let titles = v_flex()
            .flex_1()
            .min_w_0()
            .ml(px(TEXT_INDENT - PLAY_SIZE))
            .child(
                div()
                    .text_size(px(TITLE_SIZE))
                    .text_color(TEXT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .when(!state.text.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(SMALL_SIZE))
                        .text_color(TEXT_WEAK)
                        .mt(px(1.0))
                        .truncate()
                        .child(format!("„{}“", state.text)),
                )
            });

        let header = h_flex()
            .w_full()
            .items_center()
            .child(play)
            .child(titles)
            .child(
                icon_button("stopp", "Stopp", |p, rect| stop_icon(p, rect, TEXT_WEAK))
                    .ml(px(6.0))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(Control::Stop, cx))),
            )
            .child(
                icon_button("ausblenden", "Ausblenden (liest weiter)", |p, rect| {
                    crate::popup::icons_cross(p, rect.center(), 3.5, 1.4, TEXT_WEAK)
                })
                .ml(px(2.0))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(Control::Hide, cx))),
            );

        let percent = if muted {
            "Stumm".to_string()
        } else {
            format!("{} %", (state.volume * 100.0).round() as u32)
        };
        let level = state.volume;
        let volume_row = h_flex()
            .id("lautstaerke")
            .w_full()
            .mt(px(10.0))
            .items_center()
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                this.scroll_volume(event, window, cx)
            }))
            .child(
                // Auf Höhe der Mitte des großen Knopfs, damit die Spalte ruhig wirkt.
                div().flex_none().w(px(PLAY_SIZE)).flex().justify_center().child(
                    icon_button(
                        "stumm",
                        if muted { "Ton an" } else { "Stumm" },
                        move |p, rect| speaker_icon(p, rect, level, muted, TEXT_WEAK),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.send(Control::SetMuted(!muted), cx)
                    })),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .ml(px(TEXT_INDENT - PLAY_SIZE))
                    .when(muted, |el| el.opacity(0.45))
                    .child(Slider::new(&self.volume)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(44.0))
                    .ml(px(8.0))
                    .text_size(px(SMALL_SIZE))
                    .text_color(TEXT_WEAK)
                    .child(percent),
            )
            .child(
                icon_button("einstellungen", "Einstellungen", |p, rect| gear_icon(p, rect, TEXT_WEAK))
                    .ml(px(2.0))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send(Control::OpenSettings, cx))),
            );

        let measured = self.measured_height.clone();
        div().id("wiedergabe").size_full().bg(CARD_BG).opacity(self.appear).child(
            v_flex()
                .relative()
                .w_full()
                .bg(CARD_BG)
                .pl(px(PAD_X))
                .pr(px(PAD_X - 4.0))
                .py(px(PAD_Y))
                .font_family(crate::ui_kit::FONT)
                .child(header)
                .child(volume_row)
                .child(
                    canvas(move |bounds, _, _| measured.set(bounds.size.height.as_f32()), |_, _, _, _| {})
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                ),
        )
    }
}

/// Runder, dezenter Symbolknopf mit Tooltip.
fn icon_button(
    id: &'static str,
    hint: &'static str,
    paint: impl Fn(&mut Painter, Rect) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(BUTTON_SIZE))
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(tint(TEXT, 0.09)))
        .child(painted(paint).size_full())
        .tooltip(move |window, cx| Tooltip::new(hint).build(window, cx))
}

// ---------------------------------------------------------------------------------------------
// Symbole

/// Dreieck (angehalten → „Weiter“) bzw. zwei Balken (läuft → „Pause“).
fn play_pause(p: &mut Painter, rect: Rect, paused: bool, color: Color32) {
    let c = rect.center();
    let u = rect.width() / 34.0;
    if paused {
        // Optisch mittig: Dreiecke wirken links schwerer.
        let c = c + vec2(1.5 * u, 0.0);
        p.polygon(
            &[c + vec2(-5.0, -7.0) * u, c + vec2(7.0, 0.0) * u, c + vec2(-5.0, 7.0) * u],
            color,
        );
    } else {
        for dx in [-3.5, 3.5] {
            p.rect_filled(Rect::from_center_size(c + vec2(dx, 0.0) * u, vec2(3.4, 13.0) * u), 1.2 * u, color);
        }
    }
}

fn stop_icon(p: &mut Painter, rect: Rect, color: Color32) {
    p.rect_filled(Rect::from_center_size(rect.center(), vec2(9.0, 9.0)), 1.8, color);
}

/// Lautsprecher mit bis zu zwei Wellen je nach Lautstärke; stumm mit kleinem Kreuz.
fn speaker_icon(p: &mut Painter, rect: Rect, level: f32, muted: bool, color: Color32) {
    let c = rect.center() + vec2(-2.5, 0.0);
    let body: [Pos2; 6] = [
        c + vec2(-5.0, -2.5),
        c + vec2(-2.0, -2.5),
        c + vec2(2.0, -6.0),
        c + vec2(2.0, 6.0),
        c + vec2(-2.0, 2.5),
        c + vec2(-5.0, 2.5),
    ];
    p.polygon(&body, color);
    if muted {
        crate::popup::icons_cross(p, c + vec2(7.0, 0.0), 2.6, 1.4, color);
        return;
    }
    let half = std::f32::consts::FRAC_PI_4;
    if level > 0.0 {
        p.arc(c + vec2(2.0, 0.0), 3.5, -half, half, 1.4, color);
    }
    if level > 0.5 {
        p.arc(c + vec2(2.0, 0.0), 7.0, -half, half, 1.4, color);
    }
}

/// Zahnrad: Ring mit acht Zähnen.
fn gear_icon(p: &mut Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let dir = vec2(a.cos(), a.sin());
        p.line(&[c + dir * 4.5, c + dir * 6.5], 2.2, color);
    }
    p.circle_stroke(c, 4.2, 1.6, color);
}
