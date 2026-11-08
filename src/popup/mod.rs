//! Eigene Hinweis-Popups unten mittig über der Taskleiste.
//!
//! Wiederverwendbar: Ein [`Notice`] beschreibt den Inhalt aus Bausteinen ([`Block`]),
//! ein [`Handle`] aktualisiert ihn aus einem anderen Thread (z. B. Fortschritt).
//! Die Fenstergröße folgt dem Inhalt, das Popup nimmt keinen Fokus weg und
//! schließt sich nach Ablauf des Timers (Maus darauf hält ihn an).
//!
//! - im selben Prozess: [`run`] (Fenster auf dem Haupt-Thread, Arbeit im Hintergrund)
//! - aus der Tray-App: [`remote::notify`] startet `--hinweis` als eigenen Prozess

pub mod demo;
mod icons;
mod model;
pub mod remote;
mod render;
mod theme;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use eframe::egui::{self, vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWM_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

pub use model::{Action, Block, Icon, Kind, Notice, Tone};
use theme::{BOTTOM_GAP, CARD_WIDTH};

struct Shared {
    notice: Notice,
    /// Zählt jede Änderung, damit das Fenster den Timer neu startet.
    version: u64,
    ctx: Option<egui::Context>,
    closed: bool,
}

/// Verbindung zu einem offenen (oder gleich offenen) Popup. Billig zu klonen, thread-sicher.
#[derive(Clone)]
pub struct Handle(Arc<Mutex<Shared>>);

impl Handle {
    pub fn new(notice: Notice) -> Self {
        Self(Arc::new(Mutex::new(Shared {
            notice,
            version: 0,
            ctx: None,
            closed: false,
        })))
    }

    /// Ersetzt den Inhalt. Wird ignoriert, wenn das Popup schon geschlossen ist.
    pub fn set(&self, notice: Notice) {
        let mut shared = self.0.lock().unwrap();
        if shared.notice != notice {
            shared.notice = notice;
            shared.version += 1;
            if let Some(ctx) = &shared.ctx {
                ctx.request_repaint();
            }
        }
    }

    pub fn current(&self) -> Notice {
        self.0.lock().unwrap().notice.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.0.lock().unwrap().closed
    }

    fn snapshot(&self) -> (Notice, u64) {
        let shared = self.0.lock().unwrap();
        (shared.notice.clone(), shared.version)
    }
}

/// Zeigt `initial` und lässt `work` im Hintergrund laufen, das über den Handle aktualisiert.
/// Kehrt zurück, wenn das Popup zu ist *und* die Arbeit fertig ist.
/// Konnte kein Fenster geöffnet werden, kommt der letzte Inhalt zurück (z. B. für einen Toast).
pub fn run<F>(initial: Notice, work: F) -> Result<(), (String, Box<Notice>)>
where
    F: FnOnce(&Handle) + Send,
{
    let handle = Handle::new(initial);
    let shown = thread::scope(|scope| {
        scope.spawn(|| work(&handle));
        show(&handle)
    });
    shown.map_err(|e| (e, Box::new(handle.current())))
}

/// Öffnet das Popup und blockiert, bis es geschlossen ist. Muss auf dem Haupt-Thread laufen.
pub fn show(handle: &Handle) -> Result<(), String> {
    let width = CARD_WIDTH;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab")
            .with_inner_size([width, 160.0])
            // Erst außerhalb des Bildschirms; `place` setzt es, sobald die Höhe bekannt ist.
            .with_position([-32000.0, -32000.0])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_active(false)
            .with_always_on_top(),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab-Hinweis",
        options,
        Box::new(|cc| {
            crate::settings_ui::apply_theme(&cc.egui_ctx);
            style(&cc.egui_ctx);
            handle.0.lock().unwrap().ctx = Some(cc.egui_ctx.clone());
            let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(HWND(h.hwnd.get() as *mut _)),
                _ => None,
            };
            if let Some(hwnd) = hwnd {
                unsafe {
                    // Klicks aktivieren das Fenster nicht – man kann einfach weitertippen.
                    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
                    // Windows 11: runde Ecken, Schatten und ein dezenter Rahmen in Kartenfarbe.
                    let [r, g, b, _] = theme::CARD_BORDER.to_array();
                    let border = r as u32 | (g as u32) << 8 | (b as u32) << 16;
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_BORDER_COLOR,
                        &border as *const _ as *const _,
                        size_of::<u32>() as u32,
                    );
                    let corners = DWMWCP_ROUND;
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &corners as *const _ as *const _,
                        size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                    );
                }
            }
            Ok(Box::new(Popup {
                handle: handle.clone(),
                hwnd,
                placed_height: None,
                version: u64::MAX,
                elapsed: 0.0,
                last_frame: Instant::now(),
                start: Instant::now(),
                closing: false,
                view: render::ViewState::default(),
            }))
        }),
    );
    handle.0.lock().unwrap().closed = true;
    result.map_err(|e| format!("Hinweisfenster: {e}"))
}

struct Popup {
    handle: Handle,
    hwnd: Option<HWND>,
    /// Höhe, für die das Fenster zuletzt platziert wurde (logische Pixel).
    placed_height: Option<f32>,
    version: u64,
    /// Abgelaufene Timer-Zeit in Sekunden (steht, solange die Maus darauf ist).
    elapsed: f32,
    last_frame: Instant,
    start: Instant,
    closing: bool,
    view: render::ViewState,
}

impl eframe::App for Popup {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::from(theme::CARD_BG).to_array()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (notice, version) = self.handle.snapshot();
        if version != self.version {
            self.version = version;
            self.elapsed = 0.0;
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let hovered = ui.input(|i| i.pointer.has_pointer());
        if !hovered && self.placed_height.is_some() {
            self.elapsed += dt;
        }
        let remaining = notice
            .timeout()
            .map(|t| 1.0 - self.elapsed / t.as_secs_f32().max(0.1));
        if remaining.is_some_and(|r| r <= 0.0) {
            self.closing = true;
        }

        // Inhalt kurz einblenden, sobald das Fenster an seinem Platz ist.
        let appear = ctx.animate_bool_with_time(egui::Id::unique("einblenden"), self.placed_height.is_some(), 0.15);
        let time = self.start.elapsed().as_secs_f64();

        let mut card_rect = None;
        let mut events = render::Events::default();
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                ui.set_opacity(appear);
                let (rect, ev) = render::card(ui, &notice, &mut self.view, remaining, time);
                card_rect = Some(rect);
                events = ev;
            });

        if events.close {
            self.closing = true;
        }
        if events.action {
            if let Some(action) = &notice.action {
                action.run();
            }
            self.closing = true;
        }

        // Fenster an den Inhalt anpassen.
        if let (Some(rect), Some(hwnd)) = (card_rect, self.hwnd) {
            let height = rect.height().ceil();
            if self.placed_height.is_none_or(|h| (h - height).abs() > 0.5) {
                place(hwnd, vec2(CARD_WIDTH, height));
                self.placed_height = Some(height);
            }
        } else if self.hwnd.is_none() {
            self.placed_height = Some(0.0);
        }

        if self.closing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Drehkreis, Timer und Animationen brauchen laufende Bilder; sonst sparsam.
        let busy = notice.kind == Kind::Running || appear < 1.0 || remaining.is_some();
        ctx.request_repaint_after(std::time::Duration::from_millis(if busy { 16 } else { 250 }));
    }
}

/// Ruhiger Tooltip-Stil (die Schrift setzt schon `apply_theme`).
fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.interaction.tooltip_delay = 0.15;
        style.interaction.show_tooltips_only_when_still = false;
        style.spacing.tooltip_width = 250.0;
        style.visuals.window_fill = theme::TOOLTIP_BG;
        style.visuals.window_stroke = egui::Stroke::new(1.0, theme::CARD_BORDER);
        style.visuals.window_corner_radius = egui::CornerRadius::same(6);
        style.visuals.popup_shadow = egui::Shadow::NONE;
        style.spacing.menu_margin = egui::Margin::symmetric(8, 6);
    });
}

/// Setzt das Fenster unten mittig auf den Bildschirm mit dem Mauszeiger, knapp über die Taskleiste.
fn place(hwnd: HWND, size: egui::Vec2) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x as f32 / 96.0;

        let work = info.rcWork;
        let w = (size.x * scale).round() as i32;
        let h = (size.y * scale).round() as i32;
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.bottom - h - (BOTTOM_GAP * scale) as i32;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}
