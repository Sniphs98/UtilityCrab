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

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gpui_kit::*;
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

use crate::geom::vec2;
pub use model::{Action, Block, Icon, Kind, Notice, Tone};
use theme::{BOTTOM_GAP, CARD_WIDTH};

struct Shared {
    notice: Notice,
    /// Zählt jede Änderung, damit das Fenster den Timer neu startet.
    version: u64,
    closed: bool,
}

/// Verbindung zu einem offenen (oder gleich offenen) Popup. Billig zu klonen, thread-sicher.
/// Das Fenster fragt den Inhalt regelmäßig ab, Änderungen erscheinen also ohne weiteres Zutun.
#[derive(Clone)]
pub struct Handle(Arc<Mutex<Shared>>);

impl Handle {
    pub fn new(notice: Notice) -> Self {
        Self(Arc::new(Mutex::new(Shared {
            notice,
            version: 0,
            closed: false,
        })))
    }

    /// Ersetzt den Inhalt. Wird ignoriert, wenn das Popup schon geschlossen ist.
    pub fn set(&self, notice: Notice) {
        let mut shared = self.0.lock().unwrap();
        if shared.notice != notice {
            shared.notice = notice;
            shared.version += 1;
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
///
/// GPUI läuft nur einmal pro Prozess: höchstens ein Aufruf von `run`/`show` je Prozess.
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
    let opened = Rc::new(Cell::new(false));
    let handle_for_app = handle.clone();
    let opened_for_app = opened.clone();
    crate::ui_kit::run_app(move |cx| {
        let bounds = Bounds::new(point(px(-32000.0), px(-32000.0)), size(px(CARD_WIDTH), px(160.0)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            // Das Popup ist nie aktiv – trotzdem flüssig animieren.
            inactive_frame_interval: None,
            app_id: Some("UtilityCrab-Hinweis".into()),
            ..Default::default()
        };
        let handle = handle_for_app.clone();
        let result = crate::ui_kit::open_window(options, cx, move |window, cx| {
            let hwnd = crate::ui_kit::hwnd(window);
            if let Some(hwnd) = hwnd {
                style_window(hwnd);
            }
            cx.new(|cx| Popup::new(handle, hwnd, window, cx))
        });
        match result {
            Some(_) => opened_for_app.set(true),
            None => cx.quit(),
        }
    });
    handle.0.lock().unwrap().closed = true;
    if opened.get() {
        Ok(())
    } else {
        Err("Hinweisfenster konnte nicht geöffnet werden".into())
    }
}

/// Klicks aktivieren das Fenster nicht (man kann einfach weitertippen); Windows 11 zeichnet
/// runde Ecken, Schatten und einen dezenten Rahmen in Kartenfarbe.
fn style_window(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
        let c = theme::CARD_BORDER;
        let border = c.r as u32 | (c.g as u32) << 8 | (c.b as u32) << 16;
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

pub struct Popup {
    handle: Handle,
    hwnd: Option<HWND>,
    notice: Notice,
    version: u64,
    /// Höhe, für die das Fenster zuletzt platziert wurde (logische Pixel).
    placed_height: Option<f32>,
    measured_height: Rc<Cell<f32>>,
    /// Abgelaufene Timer-Zeit in Sekunden (steht, solange die Maus darauf ist).
    elapsed: f32,
    last_tick: Instant,
    start: Instant,
    hovered: bool,
    closing: bool,
    /// Einblenden: 0 → 1, sobald das Fenster an seinem Platz ist.
    appear: f32,
    shown_fraction: f32,
    pub(crate) details_open: bool,
    _ticker: Task<()>,
}

impl Popup {
    fn new(handle: Handle, hwnd: Option<HWND>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (notice, version) = handle.snapshot();
        let ticker = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                break;
            }
        });
        Self {
            handle,
            hwnd,
            shown_fraction: notice.progress().map_or(0.0, |(f, _)| f),
            notice,
            version,
            placed_height: if hwnd.is_some() { None } else { Some(0.0) },
            measured_height: Rc::new(Cell::new(0.0)),
            elapsed: 0.0,
            last_tick: Instant::now(),
            start: Instant::now(),
            hovered: false,
            closing: false,
            appear: 0.0,
            details_open: false,
            _ticker: ticker,
        }
    }

    /// Läuft etwa 60-mal pro Sekunde: Inhalt abholen, Timer, Animationen, Fenstergröße.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let (notice, version) = self.handle.snapshot();
        let changed = version != self.version;
        if changed {
            self.version = version;
            self.elapsed = 0.0;
            self.notice = notice;
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;
        if !self.hovered && self.placed_height.is_some() {
            self.elapsed += dt;
        }
        if let Some(timeout) = self.notice.timeout() {
            if self.elapsed >= timeout.as_secs_f32().max(0.1) {
                self.close(window, cx);
                return;
            }
        }

        // Fenster an den Inhalt anpassen.
        let height = self.measured_height.get().ceil();
        if let Some(hwnd) = self.hwnd {
            if height > 0.0 && self.placed_height.is_none_or(|h| (h - height).abs() > 0.5) {
                place(hwnd, vec2(CARD_WIDTH, height));
                self.placed_height = Some(height);
            }
        }

        let target = self.notice.progress().map_or(0.0, |(f, _)| f.clamp(0.0, 1.0));
        let before = (self.appear, self.shown_fraction);
        if self.placed_height.is_some() {
            self.appear = (self.appear + dt / 0.15).min(1.0);
        }
        self.shown_fraction += (target - self.shown_fraction) * (dt / 0.12).min(1.0);
        if (self.shown_fraction - target).abs() < 0.001 {
            self.shown_fraction = target;
        }

        // Drehkreis, Timer und Animationen brauchen laufende Bilder; sonst nur bei Änderungen.
        let busy = self.notice.kind == Kind::Running || self.notice.timeout().is_some();
        if changed || busy || before != (self.appear, self.shown_fraction) {
            cx.notify();
        }
    }

    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        window.remove_window();
        cx.quit();
    }

    pub(crate) fn run_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(action) = &self.notice.action {
            action.run();
        }
        self.close(window, cx);
    }
}

impl Render for Popup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let remaining = self
            .notice
            .timeout()
            .map(|t| 1.0 - self.elapsed / t.as_secs_f32().max(0.1));
        let state = render::CardState {
            details_open: self.details_open,
            shown_fraction: self.shown_fraction,
            remaining,
            time: self.start.elapsed().as_secs_f64(),
            measured_height: self.measured_height.clone(),
        };
        let card = render::card(&self.notice, &state, cx);
        div()
            .id("hinweis")
            .size_full()
            .bg(theme::CARD_BG)
            .opacity(self.appear)
            .on_hover(cx.listener(|this, hovered: &bool, _, _| this.hovered = *hovered))
            .child(card)
    }
}

/// Setzt das Fenster unten mittig auf den Bildschirm mit dem Mauszeiger, knapp über die Taskleiste.
fn place(hwnd: HWND, size: crate::geom::Vec2) {
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
