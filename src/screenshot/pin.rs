//! Bild als kleines, rahmenloses Fenster immer im Vordergrund („anheften“), z. B. als
//! Vorlage beim Abtippen. Ziehen verschiebt, Mausrad zoomt, Doppelklick oder Esc schließt.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;
use image::RgbaImage;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE};

use super::{export, EDIT_ARG};
use crate::geom::Rect;
use crate::ui_kit::{self, Painter};

pub fn run(path: PathBuf) {
    let image = match export::load(&path) {
        Ok(image) => image,
        Err(e) => return crate::report(Err(e)),
    };
    // 1 Bildpixel = 1 Bildschirmpixel, höchstens 60 % des Bildschirms mit der Maus.
    let (w, h) = image.dimensions();
    let (monitor_w, monitor_h, center) = cursor_monitor();
    let zoom = (monitor_w as f32 * 0.6 / w as f32).min(monitor_h as f32 * 0.6 / h as f32).min(1.0);

    ui_kit::run_app(move |cx| {
        let bounds = Bounds::centered(None, size(px(w as f32 * zoom), px(h as f32 * zoom)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_resizable: false,
            is_minimizable: false,
            app_id: Some("UtilityCrab-Angeheftet".into()),
            ..Default::default()
        };
        ui_kit::open_window(options, cx, move |window, cx| {
            let hwnd = ui_kit::hwnd(window);
            let pin = Pin {
                render: ui_kit::render_image(&image),
                image,
                path,
                zoom,
                hwnd,
                scale: 0.0,
                focus: cx.focus_handle(),
            };
            // Gleich in Bildschirmpixeln mittig auf den Bildschirm mit der Maus setzen.
            pin.place(center);
            let focus = pin.focus.clone();
            window.focus(&focus, cx);
            cx.new(|_| pin)
        });
    });
}

/// Größe (physische Pixel) und Mitte des Bildschirms mit der Maus.
fn cursor_monitor() -> (i32, i32, (i32, i32)) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return (1600, 900, (800, 450));
        }
        let r = info.rcWork;
        (r.right - r.left, r.bottom - r.top, ((r.left + r.right) / 2, (r.top + r.bottom) / 2))
    }
}

struct Pin {
    image: RgbaImage,
    render: Arc<RenderImage>,
    path: PathBuf,
    /// Bildschirmpixel (physisch) pro Bildpixel. In Pixeln statt logischen Punkten, damit
    /// das Bild beim Wechsel auf einen anders skalierten Bildschirm gleich groß bleibt.
    zoom: f32,
    hwnd: Option<HWND>,
    /// Skalierung im letzten Bild, um Bildschirmwechsel zu bemerken.
    scale: f32,
    focus: FocusHandle,
}

impl Pin {
    fn physical_size(&self) -> (i32, i32) {
        let (w, h) = self.image.dimensions();
        (((w as f32 * self.zoom).round() as i32).max(16), ((h as f32 * self.zoom).round() as i32).max(16))
    }

    fn place(&self, center: (i32, i32)) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let (w, h) = self.physical_size();
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), center.0 - w / 2, center.1 - h / 2, w, h, SWP_NOACTIVATE);
        }
    }

    /// Fenstergröße an Zoom und Bildschirmskalierung anpassen.
    fn resize(&self, window: &mut Window) {
        let (w, h) = self.physical_size();
        let scale = window.scale_factor();
        window.resize(size(px(w as f32 / scale), px(h as f32 / scale)));
    }

    fn close(&mut self, window: &mut Window) {
        window.remove_window();
    }
}

impl Render for Pin {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = window.scale_factor();
        if self.scale > 0.0 && (scale - self.scale).abs() > 0.001 {
            // Auf einen Bildschirm mit anderer Skalierung gezogen: Windows hat das Fenster
            // mitskaliert, das Bild soll aber scharf in seiner Pixelgröße bleiben.
            self.resize(window);
        }
        self.scale = scale;

        let image = self.render.clone();
        let view = cx.entity().downgrade();
        div()
            .id("angeheftet")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    this.close(window);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, _| {
                    if event.click_count >= 2 {
                        this.close(window);
                    } else {
                        window.start_window_move();
                    }
                }),
            )
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                let dy = event.delta.pixel_delta(px(20.0)).y.as_f32();
                if dy != 0.0 {
                    this.zoom = (this.zoom * (1.0 + dy * 0.002)).clamp(0.05, 16.0);
                    this.resize(window);
                    cx.notify();
                }
            }))
            .tooltip(|window, cx| {
                Tooltip::new("Ziehen verschiebt · Mausrad zoomt · Doppelklick schließt · Rechtsklick für mehr").build(window, cx)
            })
            .child(
                ui_kit::painted(move |p: &mut Painter, rect: Rect| {
                    p.image(rect, rect, &image);
                    p.rect_stroke_inside(rect, 1.0, ui_kit::ACCENT);
                })
                .size_full(),
            )
            .context_menu(move |menu, _, _| {
                let copy = view.clone();
                let edit = view.clone();
                let original = view.clone();
                let close = view.clone();
                menu.item(PopupMenuItem::new("Kopieren").on_click(move |_, _, cx| {
                    if let Some(pin) = copy.upgrade() {
                        if let Err(e) = export::copy(&pin.read(cx).image) {
                            crate::log_error(&e);
                        }
                    }
                }))
                .item(PopupMenuItem::new("Bearbeiten").on_click(move |_, _, cx| {
                    if let Some(pin) = edit.upgrade() {
                        crate::report(super::spawn(&[EDIT_ARG, &pin.read(cx).path.to_string_lossy()]));
                    }
                }))
                .item(PopupMenuItem::new("Originalgröße").on_click(move |_, window, cx| {
                    let _ = original.update(cx, |pin, cx| {
                        pin.zoom = 1.0;
                        pin.resize(window);
                        cx.notify();
                    });
                }))
                .separator()
                .item(PopupMenuItem::new("Schließen").on_click(move |_, window, cx| {
                    let _ = close.update(cx, |pin, _| pin.close(window));
                }))
            })
    }
}
