//! Auswahl über dem eingefrorenen Bildschirm: Rechteck, Fenster, ganzer Bildschirm oder
//! Freihand. Oben mittig eine Leiste zum Umschalten, am Mauszeiger eine Lupe mit Pixelfarbe.
//! Rechnet intern in physischen Pixeln, damit es auf Monitoren mit Skalierung stimmt.
//!
//! Für jeden Bildschirm gibt es ein eigenes randloses Fenster mit dessen Ausschnitt des
//! Bildschirmfotos. So hat jedes Fenster die Skalierung seines Bildschirms, und Leiste und
//! Lupe sind auf Bildschirmen mit unterschiedlicher Skalierung gleich groß. Die Auswahl
//! selbst ([`Shot`]) teilen sich alle Fenster; sie kann über Bildschirmgrenzen gehen.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::*;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE};

use super::capture::{self, PxRect, Screen};
use super::icons::{self, Glyph};
use super::{export, Prefs, EDIT_ARG};
use crate::geom::{pos2, vec2, Color32, Pos2, Rect};
use crate::popup::{Action, Block, Icon, Kind, Notice};
use crate::settings::Settings;
use crate::ui_kit::{self, h_flex, text, v_flex, Painter};

/// Aufnahmearten. Eine neue Art: Variante + Einträge in den `match`es + Logik in `Overlay::ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CaptureMode {
    #[default]
    Rect,
    Window,
    Screen,
    Freeform,
}

impl CaptureMode {
    pub const ALL: [CaptureMode; 4] = [CaptureMode::Rect, CaptureMode::Window, CaptureMode::Screen, CaptureMode::Freeform];

    pub fn id(self) -> &'static str {
        match self {
            CaptureMode::Rect => "rechteck",
            CaptureMode::Window => "fenster",
            CaptureMode::Screen => "vollbild",
            CaptureMode::Freeform => "freihand",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            CaptureMode::Rect => "Rechteck",
            CaptureMode::Window => "Fenster",
            CaptureMode::Screen => "Vollbild",
            CaptureMode::Freeform => "Freihand",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            CaptureMode::Rect => Glyph::ModeRect,
            CaptureMode::Window => Glyph::ModeWindow,
            CaptureMode::Screen => Glyph::ModeScreen,
            CaptureMode::Freeform => Glyph::ModeFreeform,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            CaptureMode::Rect => "Bereich aufziehen · Klick nimmt das Fenster darunter · Esc bricht ab",
            CaptureMode::Window => "Fenster anklicken · Esc bricht ab",
            CaptureMode::Screen => "Bildschirm anklicken · Enter nimmt alle Bildschirme · Esc bricht ab",
            CaptureMode::Freeform => "Form mit gedrückter Maustaste umfahren · Esc bricht ab",
        }
    }
}

/// Nimmt auf, lässt auswählen und gibt das Ergebnis weiter (Zwischenablage, Speichern, Editor).
pub fn run(mode: Option<CaptureMode>, delay: Duration) {
    capture::make_dpi_aware();
    std::thread::sleep(delay);
    let screen = match capture::grab() {
        Ok(screen) => screen,
        Err(e) => return crate::report(Err(e)),
    };
    let mut prefs = Prefs::load();
    if let Some(mode) = mode {
        if mode != prefs.mode {
            prefs.mode = mode;
            prefs.save();
        }
    }
    if mode == Some(CaptureMode::Screen) {
        // Direkt aus dem Menü: alle Bildschirme ohne Auswahl.
        return finish(screen.image);
    }

    let result: Rc<RefCell<Option<RgbaImage>>> = Rc::default();
    let shared = result.clone();
    let area = capture::virtual_screen();
    ui_kit::run_app(move |cx| {
        let monitors = if screen.monitors.is_empty() {
            vec![PxRect { x: 0, y: 0, w: area.w, h: area.h }]
        } else {
            screen.monitors.clone()
        };
        let shot = cx.new(|_| Shot::new(screen, area, prefs, shared));
        let mut opened = 0;
        for (index, monitor) in monitors.into_iter().enumerate() {
            if open_monitor_window(&shot, monitor, area, index == 0, cx) {
                opened += 1;
            }
        }
        if opened == 0 {
            crate::report(Err("Screenshot-Auswahl: kein Fenster".into()));
            cx.quit();
        }
    });
    let image = result.borrow_mut().take();
    if let Some(image) = image {
        finish(image);
    }
}

/// Öffnet das Auswahlfenster für einen Bildschirm (`monitor` in Bildpixeln).
fn open_monitor_window(shot: &Entity<Shot>, monitor: PxRect, area: PxRect, first: bool, cx: &mut App) -> bool {
    // Den GPUI-Bildschirm über das Win32-Handle des Monitors finden.
    let center = POINT {
        x: area.x + monitor.x + monitor.w / 2,
        y: area.y + monitor.y + monitor.h / 2,
    };
    let hmonitor = unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST) };
    let display_id = DisplayId::new(hmonitor.0 as u64);
    let display = cx.find_display(display_id).or_else(|| cx.primary_display());
    let bounds = display
        .as_ref()
        .map(|d| d.bounds())
        .unwrap_or_else(|| Bounds::new(point(px(monitor.x as f32), px(monitor.y as f32)), size(px(monitor.w as f32), px(monitor.h as f32))));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: first,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: display.map(|d| d.id()),
        inactive_frame_interval: None,
        app_id: Some("UtilityCrab-Screenshot".into()),
        ..Default::default()
    };
    let crop = shot.read(cx).crop(monitor);
    let shot = shot.clone();
    ui_kit::open_window(options, cx, move |window, cx| {
        let hwnd = ui_kit::hwnd(window);
        let view = cx.new(|cx| MonitorWindow::new(shot, monitor, area, crop, hwnd, window, cx));
        if first {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        view
    })
    .is_some()
}

/// Nach der Auswahl: kopieren, ggf. speichern, Editor öffnen oder Hinweis zeigen.
fn finish(image: RgbaImage) {
    let settings = Settings::load();
    if settings.snip_copy {
        if let Err(e) = export::copy(&image) {
            crate::log_error(&e);
        }
    }
    let saved = if settings.snip_autosave {
        match export::autosave(&image, &settings) {
            Ok(path) => Some(path),
            Err(e) => {
                crate::log_error(&e);
                None
            }
        }
    } else {
        None
    };
    let path = match saved.clone().map(Ok).unwrap_or_else(|| super::write_temp(&image)) {
        Ok(path) => path,
        Err(e) => return crate::report(Err(e)),
    };
    let path_arg = path.to_string_lossy().into_owned();

    if settings.snip_open_editor {
        crate::report(super::spawn(&[EDIT_ARG, &path_arg]));
        return;
    }
    let mut notice = Notice::new(Kind::Success, "Screenshot aufgenommen")
        .subtitle(format!("{} × {} Pixel", image.width(), image.height()))
        .action(Action::Run {
            label: "Bearbeiten".into(),
            args: vec![EDIT_ARG.into(), path_arg],
        });
    if settings.snip_copy {
        notice = notice.block(Block::Chip {
            icon: Icon::Clipboard,
            text: "In Zwischenablage · Strg+V".into(),
        });
    }
    if let Some(saved) = saved {
        notice = notice.block(Block::Text(format!("Gespeichert: {}", saved.display())));
    }
    crate::popup::remote::notify(notice);
}

// ---------------------------------------------------------------------------------------------
// Gemeinsame Auswahl

/// Stand der Auswahl, geteilt von allen Bildschirmfenstern. Alles in Bildpixeln
/// (physische Pixel relativ zur linken oberen Ecke des virtuellen Bildschirms).
pub struct Shot {
    screen: Screen,
    /// Lage des Bildes auf dem virtuellen Bildschirm.
    area: PxRect,
    mode: CaptureMode,
    prefs: Prefs,
    /// Maus gedrückt (Auswahl läuft).
    pressed: bool,
    drag_start: Option<Pos2>,
    drag_now: Option<Pos2>,
    lasso: Vec<Pos2>,
    result: Rc<RefCell<Option<RgbaImage>>>,
    done: bool,
}

impl Shot {
    fn new(screen: Screen, area: PxRect, prefs: Prefs, result: Rc<RefCell<Option<RgbaImage>>>) -> Self {
        Self {
            screen,
            area,
            mode: prefs.mode,
            prefs,
            pressed: false,
            drag_start: None,
            drag_now: None,
            lasso: Vec::new(),
            result,
            done: false,
        }
    }

    /// Ausschnitt des Bildschirmfotos für ein Fenster.
    fn crop(&self, monitor: PxRect) -> Arc<RenderImage> {
        let (w, h) = self.screen.image.dimensions();
        let image = match monitor.clip(w, h) {
            Some(r) => image::imageops::crop_imm(&self.screen.image, r.x as u32, r.y as u32, r.w as u32, r.h as u32).to_image(),
            None => RgbaImage::new(1, 1),
        };
        ui_kit::render_image(&image)
    }

    /// Mauszeiger in Bildpixeln, direkt von Windows – stimmt auf jedem Bildschirm, egal in
    /// welchem Fenster das Ereignis ankam.
    fn cursor_px(&self) -> Option<Pos2> {
        capture::cursor_pos().map(|(x, y)| pos2((x - self.area.x) as f32, (y - self.area.y) as f32))
    }

    /// Fenster unter dem Punkt, sonst der Bildschirm.
    fn window_at(&self, p: Pos2) -> Option<PxRect> {
        self.screen
            .windows
            .iter()
            .find(|w| w.contains(p.x, p.y))
            .copied()
            .or_else(|| self.monitor_at(p))
    }

    fn monitor_at(&self, p: Pos2) -> Option<PxRect> {
        self.screen.monitors.iter().find(|m| m.contains(p.x, p.y)).copied()
    }

    fn selection(&self) -> Option<PxRect> {
        let (a, b) = (self.drag_start?, self.drag_now?);
        let r = Rect::from_two_pos(a, b);
        Some(PxRect {
            x: r.min.x.round() as i32,
            y: r.min.y.round() as i32,
            w: r.width().round() as i32,
            h: r.height().round() as i32,
        })
    }

    fn press(&mut self) {
        let Some(p) = self.cursor_px() else {
            return;
        };
        self.pressed = true;
        match self.mode {
            CaptureMode::Rect => {
                // Ab dem Klickpunkt, nicht erst ab einer Zieh-Schwelle.
                self.drag_start = Some(p);
                self.drag_now = Some(p);
            }
            CaptureMode::Freeform => self.lasso = vec![p],
            CaptureMode::Window | CaptureMode::Screen => {}
        }
    }

    fn moved(&mut self) {
        if !self.pressed {
            return;
        }
        let Some(p) = self.cursor_px() else {
            return;
        };
        match self.mode {
            CaptureMode::Rect => self.drag_now = Some(p),
            CaptureMode::Freeform => {
                if self.lasso.last().is_none_or(|l| l.distance(p) >= 2.0) {
                    self.lasso.push(p);
                }
            }
            CaptureMode::Window | CaptureMode::Screen => {}
        }
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        if !self.pressed {
            return;
        }
        self.pressed = false;
        let p = self.cursor_px();
        match self.mode {
            CaptureMode::Rect => {
                let selection = self.selection();
                self.drag_start = None;
                self.drag_now = None;
                match selection.filter(|s| s.w >= 3 && s.h >= 3) {
                    Some(sel) => self.select(sel, cx),
                    // Nur geklickt: das Fenster darunter.
                    None => {
                        if let Some(win) = p.and_then(|p| self.window_at(p)) {
                            self.select(win, cx);
                        }
                    }
                }
            }
            CaptureMode::Window | CaptureMode::Screen => {
                let target = p.and_then(|p| {
                    if self.mode == CaptureMode::Window {
                        self.window_at(p)
                    } else {
                        self.monitor_at(p)
                    }
                });
                if let Some(t) = target {
                    self.select(t, cx);
                }
            }
            CaptureMode::Freeform => self.select_lasso(cx),
        }
    }

    fn key(&mut self, key: &str, cx: &mut Context<Self>) {
        match key {
            "escape" => {
                if self.drag_start.is_some() || !self.lasso.is_empty() {
                    self.pressed = false;
                    self.drag_start = None;
                    self.drag_now = None;
                    self.lasso.clear();
                } else {
                    cx.quit();
                }
            }
            "enter" if self.mode == CaptureMode::Screen => {
                let (w, h) = self.screen.image.dimensions();
                self.select(PxRect { x: 0, y: 0, w: w as i32, h: h as i32 }, cx);
            }
            _ => {}
        }
    }

    fn select(&mut self, rect: PxRect, cx: &mut Context<Self>) {
        let (w, h) = self.screen.image.dimensions();
        if let Some(r) = rect.clip(w, h) {
            let image = image::imageops::crop_imm(&self.screen.image, r.x as u32, r.y as u32, r.w as u32, r.h as u32).to_image();
            *self.result.borrow_mut() = Some(image);
        }
        self.done = true;
        cx.quit();
    }

    fn select_lasso(&mut self, cx: &mut Context<Self>) {
        if self.lasso.len() < 3 {
            self.lasso.clear();
            return;
        }
        let bounds = Rect::from_points(&self.lasso);
        let rect = PxRect {
            x: bounds.min.x.floor() as i32,
            y: bounds.min.y.floor() as i32,
            w: bounds.width().ceil() as i32 + 1,
            h: bounds.height().ceil() as i32 + 1,
        };
        let (w, h) = self.screen.image.dimensions();
        let Some(r) = rect.clip(w, h) else {
            self.lasso.clear();
            return;
        };
        let mut image = image::imageops::crop_imm(&self.screen.image, r.x as u32, r.y as u32, r.w as u32, r.h as u32).to_image();
        // Außerhalb der Form durchsichtig, mit weicher Kante.
        let local: Vec<Pos2> = self.lasso.iter().map(|p| pos2(p.x - r.x as f32, p.y - r.y as f32)).collect();
        if let (Some(path), Some(mut mask)) = (super::canvas::polygon(&local), tiny_skia::Mask::new(r.w as u32, r.h as u32)) {
            mask.fill_path(&path, tiny_skia::FillRule::Winding, true, tiny_skia::Transform::identity());
            for (px, m) in image.pixels_mut().zip(mask.data()) {
                px.0[3] = ((px.0[3] as u16 * *m as u16) / 255) as u8;
            }
        }
        *self.result.borrow_mut() = Some(image);
        self.done = true;
        cx.quit();
    }

    fn set_mode(&mut self, mode: CaptureMode) {
        self.mode = mode;
        self.pressed = false;
        self.drag_start = None;
        self.drag_now = None;
        self.lasso.clear();
        if self.prefs.mode != mode {
            self.prefs.mode = mode;
            self.prefs.save();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Ein Fenster pro Bildschirm

struct MonitorWindow {
    shot: Entity<Shot>,
    /// Bereich dieses Bildschirms im Bild (physische Pixel).
    monitor: PxRect,
    area: PxRect,
    image: Arc<RenderImage>,
    hwnd: Option<HWND>,
    focus: FocusHandle,
    /// Lupe: Pixel, für den sie zuletzt gebaut wurde, und ihr Bild.
    magnifier: Option<((u32, u32), Arc<RenderImage>)>,
    stale_images: Vec<Arc<RenderImage>>,
    _observe: Subscription,
    _placement: Task<()>,
}

impl MonitorWindow {
    fn new(
        shot: Entity<Shot>,
        monitor: PxRect,
        area: PxRect,
        image: Arc<RenderImage>,
        hwnd: Option<HWND>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&shot, |_, _, cx| cx.notify());
        // Windows passt neue Fenster bei unterschiedlicher Skalierung manchmal an: eine Weile
        // prüfen, dass das Fenster genau auf seinem Bildschirm liegt.
        let placement = cx.spawn_in(window, async move |this, cx| {
            for check in 0..30u32 {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                let ok = this.update_in(cx, |this, window, _| this.place_window(check, window));
                if ok.is_err() {
                    break;
                }
            }
        });
        Self {
            shot,
            monitor,
            area,
            image,
            hwnd,
            focus: cx.focus_handle(),
            magnifier: None,
            stale_images: Vec::new(),
            _observe: observe,
            _placement: placement,
        }
    }

    fn place_window(&mut self, check: u32, window: &mut Window) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        let (x, y, w, h) = (self.area.x + self.monitor.x, self.area.y + self.monitor.y, self.monitor.w, self.monitor.h);
        unsafe {
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            if (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top) != (x, y, w, h) {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
            }
            if check % 10 == 0 && window.is_window_active() {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    /// Physische Pixel pro logischem Pixel in diesem Fenster.
    fn px_per_pt(&self, window: &Window) -> f32 {
        let width = window.viewport_size().width.as_f32();
        if width > 0.0 {
            self.monitor.w as f32 / width
        } else {
            window.scale_factor()
        }
    }

    /// Lupenbild (vergrößerte Pixel um `px`), nur neu gebaut, wenn sich der Pixel ändert.
    fn magnifier_image(&mut self, shot: &Shot, px: (u32, u32), scale: f32) -> Arc<RenderImage> {
        if let Some((at, image)) = &self.magnifier {
            if *at == px {
                return image.clone();
            }
        }
        let (w, h) = shot.screen.image.dimensions();
        // So groß, dass jeder Bildpixel scharf als Block erscheint.
        let k = ((MAGNIFIER_SIZE * scale) / MAGNIFIER_CELLS as f32).ceil().max(1.0) as u32;
        let side = MAGNIFIER_CELLS * k;
        let half = MAGNIFIER_CELLS as i64 / 2;
        let mut out = RgbaImage::from_pixel(side, side, image::Rgba([0, 0, 0, 255]));
        for cy in 0..MAGNIFIER_CELLS {
            for cx in 0..MAGNIFIER_CELLS {
                let sx = px.0 as i64 + cx as i64 - half;
                let sy = px.1 as i64 + cy as i64 - half;
                if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                    continue;
                }
                let color = *shot.screen.image.get_pixel(sx as u32, sy as u32);
                for y in 0..k {
                    for x in 0..k {
                        out.put_pixel(cx * k + x, cy * k + y, image::Rgba([color.0[0], color.0[1], color.0[2], 255]));
                    }
                }
            }
        }
        let image = ui_kit::render_image(&out);
        if let Some((_, old)) = self.magnifier.replace((px, image.clone())) {
            self.stale_images.push(old);
        }
        image
    }
}

const MAGNIFIER_CELLS: u32 = 15;
/// Kantenlänge der Lupe in logischen Pixeln.
const MAGNIFIER_SIZE: f32 = 120.0;

/// Was ein Fenster zeichnet, in seinen logischen Koordinaten.
struct Plan {
    full: Rect,
    image: Arc<RenderImage>,
    /// Ganz abdunkeln (vor der Auswahl, Freihand).
    dim_all: bool,
    /// Fenster unter der Maus dezent andeuten.
    hint: Option<Rect>,
    highlight: Option<Rect>,
    lasso: Vec<Pos2>,
    magnifier: Option<(Rect, Arc<RenderImage>)>,
}

fn paint_plan(p: &mut Painter, plan: &Plan) {
    let dim = Color32::from_black_alpha(110);
    p.image(plan.full, plan.full, &plan.image);
    if plan.dim_all {
        p.rect_filled(plan.full, 0.0, dim);
    }
    if let Some(r) = plan.hint {
        p.rect_filled(r, 0.0, Color32::from_white_alpha(10));
        p.rect_stroke_inside(r, 1.0, Color32::from_white_alpha(70));
    }
    if let Some(r) = plan.highlight {
        let full = plan.full;
        for part in [
            Rect::from_min_max(full.min, pos2(full.max.x, r.min.y)),
            Rect::from_min_max(pos2(full.min.x, r.max.y), full.max),
            Rect::from_min_max(pos2(full.min.x, r.min.y), pos2(r.min.x, r.max.y)),
            Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(full.max.x, r.max.y)),
        ] {
            if part.is_positive() {
                p.rect_filled(part.intersect(full), 0.0, dim);
            }
        }
        p.rect_stroke_outside(r, 2.0, ui_kit::ACCENT);
    }
    if plan.lasso.len() > 1 {
        p.line(&plan.lasso, 3.0, Color32::from_black_alpha(120));
        p.line(&plan.lasso, 1.5, Color32::WHITE);
    }
    if let Some((rect, image)) = &plan.magnifier {
        p.rect_filled(rect.expand(3.0), 6.0, ui_kit::GROUP_BG);
        p.image(*rect, *rect, image);
        let cell = rect.width() / MAGNIFIER_CELLS as f32;
        let center = Rect::from_center_size(rect.center(), vec2(cell, cell));
        p.rect_stroke_outside(center, 1.0, Color32::WHITE);
        p.rect_stroke_outside(center.expand(1.0), 1.0, Color32::BLACK);
    }
}

impl Render for MonitorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.stale_images.drain(..) {
            ui_kit::drop_image(image, window);
        }
        let s = self.px_per_pt(window);
        let viewport = window.viewport_size();
        let full = Rect::from_min_size(Pos2::ZERO, vec2(viewport.width.as_f32(), viewport.height.as_f32()));
        let origin = pos2(self.monitor.x as f32, self.monitor.y as f32);
        let to_local = |p: Pos2| ((p - origin) / s).to_pos2();
        let rect_local = |r: PxRect| Rect::from_min_size(to_local(pos2(r.x as f32, r.y as f32)), vec2(r.w as f32, r.h as f32) / s);

        let shot_entity = self.shot.clone();
        let shot = shot_entity.read(cx);
        let cursor = shot.cursor_px();
        // Leiste, Hinweis und Lupe nur auf dem Bildschirm mit der Maus.
        let here = cursor.is_some_and(|p| self.monitor.contains(p.x, p.y))
            || (cursor.is_none() && shot.screen.monitors.first().is_none_or(|m| *m == self.monitor));
        let mode = shot.mode;

        let mut plan = Plan {
            full,
            image: self.image.clone(),
            dim_all: false,
            hint: None,
            highlight: None,
            lasso: Vec::new(),
            magnifier: None,
        };
        let mut size_label: Option<(Rect, String)> = None;
        let mut magnifier = false;
        match mode {
            CaptureMode::Rect => {
                match shot.selection() {
                    Some(sel) if shot.pressed => {
                        plan.highlight = Some(rect_local(sel));
                        size_label = Some((rect_local(sel), format!("{} × {}", sel.w, sel.h)));
                    }
                    _ => {
                        plan.dim_all = true;
                        // Vor dem Ziehen: das Fenster unter der Maus dezent andeuten.
                        if let Some(win) = cursor.and_then(|p| shot.window_at(p)) {
                            plan.hint = Some(rect_local(win));
                        }
                    }
                }
                magnifier = true;
            }
            CaptureMode::Window | CaptureMode::Screen => {
                let target = cursor.and_then(|p| {
                    if mode == CaptureMode::Window {
                        shot.window_at(p)
                    } else {
                        shot.monitor_at(p)
                    }
                });
                match target {
                    Some(t) => {
                        plan.highlight = Some(rect_local(t));
                        size_label = Some((rect_local(t), format!("{} × {}", t.w, t.h)));
                    }
                    None => plan.dim_all = true,
                }
            }
            CaptureMode::Freeform => {
                plan.dim_all = true;
                plan.lasso = shot.lasso.iter().map(|p| to_local(*p)).collect();
                magnifier = true;
            }
        }

        // Lupe neben dem Mauszeiger: vergrößerte Pixel, Fadenkreuz, Position und Farbe.
        let mut magnifier_info: Option<(Pos2, String, String, Color32)> = None;
        if let (true, true, Some(p)) = (magnifier, here, cursor) {
            let (w, h) = shot.screen.image.dimensions();
            let px = ((p.x.floor() as i64).clamp(0, w as i64 - 1) as u32, (p.y.floor() as i64).clamp(0, h as i64 - 1) as u32);
            let [r, g, b, _] = shot.screen.image.get_pixel(px.0, px.1).0;
            let cursor_local = to_local(p);
            let mut origin = cursor_local + vec2(24.0, 24.0);
            if origin.x + MAGNIFIER_SIZE > full.max.x {
                origin.x = cursor_local.x - 24.0 - MAGNIFIER_SIZE;
            }
            if origin.y + MAGNIFIER_SIZE + 40.0 > full.max.y {
                origin.y = cursor_local.y - 24.0 - MAGNIFIER_SIZE - 40.0;
            }
            let rect = Rect::from_min_size(origin, vec2(MAGNIFIER_SIZE, MAGNIFIER_SIZE));
            let shot_ref = self.shot.clone();
            let image = self.magnifier_image(shot_ref.read(cx), px, s);
            plan.magnifier = Some((rect, image));
            magnifier_info = Some((
                rect.left_bottom() + vec2(-3.0, 6.0),
                format!("#{r:02X}{g:02X}{b:02X}"),
                format!("{}, {}", px.0, px.1),
                Color32::from_rgb(r, g, b),
            ));
        }

        let this = cx.entity();
        let surface = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, cx| {
                window.set_cursor_style(CursorStyle::Crosshair, &hitbox);
                let shot = this.read(cx).shot.clone();
                let hb = hitbox.clone();
                let s1 = shot.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left && hb.is_hovered(window) {
                        s1.update(cx, |shot, cx| {
                            shot.press();
                            cx.notify();
                        });
                    }
                });
                let s2 = shot.clone();
                window.on_mouse_event(move |_: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        s2.update(cx, |shot, cx| {
                            shot.moved();
                            cx.notify();
                        });
                    }
                });
                let s3 = shot.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        s3.update(cx, |shot, cx| {
                            shot.release(cx);
                            cx.notify();
                        });
                    }
                });
                paint_plan(&mut Painter::new(window), &plan);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let label = size_label.map(|(sel, label)| {
            let mut pos = sel.left_top() - vec2(0.0, 26.0);
            if pos.y < 4.0 {
                pos = sel.left_top() + vec2(6.0, 6.0);
            }
            div()
                .absolute()
                .left(px(pos.x))
                .top(px(pos.y))
                .px(px(6.0))
                .py(px(3.0))
                .rounded(px(5.0))
                .bg(Color32::from_black_alpha(190))
                .child(text(label, ui_kit::SMALL_SIZE, Color32::WHITE).font_weight(FontWeight::SEMIBOLD))
        });
        let info = magnifier_info.map(|(at, hex, pos, color)| {
            h_flex()
                .absolute()
                .left(px(at.x))
                .top(px(at.y))
                .w(px(MAGNIFIER_SIZE + 6.0))
                .h(px(34.0))
                .px(px(8.0))
                .gap(px(6.0))
                .rounded(px(6.0))
                .bg(ui_kit::GROUP_BG)
                .child(div().size(px(16.0)).rounded(px(3.0)).bg(color))
                .child(
                    v_flex()
                        .child(text(hex, 11.0, ui_kit::TEXT))
                        .child(text(pos, 11.0, ui_kit::TEXT_WEAK)),
                )
        });
        let toolbar = here.then(|| self.toolbar(mode, cx));

        div()
            .id("auswahl")
            .size_full()
            .relative()
            .bg(Color32::BLACK)
            .font_family(ui_kit::FONT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.clone();
                this.shot.update(cx, |shot, cx| {
                    shot.key(&key, cx);
                    cx.notify();
                });
            }))
            .child(surface)
            .children(label)
            .children(info)
            .children(toolbar)
    }
}

impl MonitorWindow {
    /// Leiste oben mittig zum Umschalten der Aufnahmeart, darunter eine Bedienhilfe.
    fn toolbar(&self, mode: CaptureMode, cx: &mut Context<Self>) -> impl IntoElement {
        let modes = CaptureMode::ALL.into_iter().map(|m| {
            icons::button(m.id(), m.glyph(), mode == m, true, m.label()).on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.shot.update(cx, |shot, cx| {
                        shot.set_mode(m);
                        cx.notify();
                    });
                },
            ))
        });
        v_flex()
            .absolute()
            .top(px(14.0))
            .left_0()
            .w_full()
            .items_center()
            .gap(px(8.0))
            .child(
                h_flex()
                    .occlude()
                    .gap(px(2.0))
                    .p(px(5.0))
                    .rounded(px(10.0))
                    .bg(ui_kit::GROUP_BG)
                    .border_1()
                    .border_color(ui_kit::DIVIDER)
                    .shadow_lg()
                    .children(modes)
                    .child(ui_kit::vertical_divider())
                    .child(
                        icons::button("abbrechen", Glyph::Close, false, true, "Abbrechen (Esc)")
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
            .child(
                div()
                    .px(px(12.0))
                    .py(px(5.0))
                    .rounded(px(12.0))
                    .bg(Color32::from_black_alpha(170))
                    .child(text(mode.hint(), ui_kit::SMALL_SIZE, Color32::WHITE)),
            )
    }
}
