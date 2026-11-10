//! Auswahl über dem eingefrorenen Bildschirm: Rechteck, Fenster, ganzer Bildschirm oder
//! Freihand. Oben mittig eine Leiste zum Umschalten, am Mauszeiger eine Lupe mit Pixelfarbe.
//! Rechnet intern in physischen Pixeln, damit es auf Monitoren mit Skalierung stimmt.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind};
use image::RgbaImage;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
};

use super::capture::{self, PxRect, Screen};
use super::icons::{self, Glyph};
use super::{export, Prefs, EDIT_ARG};
use crate::popup::{Action, Block, Icon, Kind, Notice};
use crate::settings::Settings;
use crate::ui_kit;

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

    let result: Arc<Mutex<Option<RgbaImage>>> = Arc::default();
    let shared = result.clone();
    let area = capture::virtual_screen();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Screenshot")
            .with_position([area.x as f32, area.y as f32])
            .with_inner_size([area.w as f32, area.h as f32])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top(),
        ..Default::default()
    };
    let outcome = eframe::run_native(
        "UtilityCrab-Screenshot",
        options,
        Box::new(move |cc| {
            ui_kit::apply(&cc.egui_ctx);
            let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(HWND(h.hwnd.get() as *mut _)),
                _ => None,
            };
            Ok(Box::new(Overlay::new(&cc.egui_ctx, screen, prefs, hwnd, shared)))
        }),
    );
    if let Err(e) = outcome {
        return crate::report(Err(format!("Screenshot-Auswahl: {e}")));
    }
    let image = result.lock().unwrap().take();
    if let Some(image) = image {
        finish(image);
    }
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

struct Overlay {
    screen: Screen,
    texture: egui::TextureHandle,
    mode: CaptureMode,
    prefs: Prefs,
    hwnd: Option<HWND>,
    /// Bis das Fenster sicher über allen Bildschirmen liegt (Windows passt es bei DPI-Wechseln an).
    placement_checks: u32,
    drag_start: Option<Pos2>,
    drag_now: Option<Pos2>,
    lasso: Vec<Pos2>,
    result: Arc<Mutex<Option<RgbaImage>>>,
}

impl Overlay {
    fn new(
        ctx: &egui::Context,
        screen: Screen,
        prefs: Prefs,
        hwnd: Option<HWND>,
        result: Arc<Mutex<Option<RgbaImage>>>,
    ) -> Self {
        let max = ctx.input(|i| i.max_texture_side) as u32;
        let (texture, _) = upload(ctx, "bildschirm", &screen.image, max);
        Self {
            screen,
            texture,
            mode: prefs.mode,
            prefs,
            hwnd,
            placement_checks: 30,
            drag_start: None,
            drag_now: None,
            lasso: Vec::new(),
            result,
        }
    }

    fn place_window(&mut self) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        if self.placement_checks == 0 {
            return;
        }
        self.placement_checks -= 1;
        let area = capture::virtual_screen();
        unsafe {
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            if (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top) != (area.x, area.y, area.w, area.h) {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), area.x, area.y, area.w, area.h, SWP_NOACTIVATE);
            }
            if self.placement_checks % 10 == 9 {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }

    /// Fenster unter dem Punkt (physische Pixel), sonst der Bildschirm.
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

    fn select(&mut self, rect: PxRect, ctx: &egui::Context) {
        let (w, h) = self.screen.image.dimensions();
        if let Some(r) = rect.clip(w, h) {
            let image = image::imageops::crop_imm(&self.screen.image, r.x as u32, r.y as u32, r.w as u32, r.h as u32).to_image();
            *self.result.lock().unwrap() = Some(image);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn select_lasso(&mut self, ctx: &egui::Context) {
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
        *self.result.lock().unwrap() = Some(image);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn set_mode(&mut self, mode: CaptureMode) {
        self.mode = mode;
        self.drag_start = None;
        self.lasso.clear();
        if self.prefs.mode != mode {
            self.prefs.mode = mode;
            self.prefs.save();
        }
    }
}

impl eframe::App for Overlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.place_window();
        let ppp = ctx.pixels_per_point();
        let to_pt = |p: Pos2| pos2(p.x / ppp, p.y / ppp);
        let rect_pt = |r: PxRect| Rect::from_min_size(pos2(r.x as f32, r.y as f32) / ppp, vec2(r.w as f32, r.h as f32) / ppp);
        let (w, h) = self.screen.image.dimensions();
        let full = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32) / ppp);

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.drag_start.is_some() || !self.lasso.is_empty() {
                self.drag_start = None;
                self.lasso.clear();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.mode == CaptureMode::Screen && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.select(PxRect { x: 0, y: 0, w: w as i32, h: h as i32 }, &ctx);
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let response = ui.interact(full, egui::Id::unique("auswahl"), Sense::click_and_drag());
            let painter = ui.painter().clone();
            painter.image(self.texture.id(), full, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);

            let pointer_px = response.hover_pos().or(ctx.pointer_latest_pos()).map(|p| pos2(p.x * ppp, p.y * ppp));
            let dim = Color32::from_black_alpha(110);
            let mut highlight: Option<PxRect> = None;
            let mut magnifier = false;

            match self.mode {
                CaptureMode::Rect => {
                    if response.drag_started() {
                        // Ab dem Klickpunkt, nicht erst ab der Zieh-Schwelle.
                        let origin = ctx.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                        self.drag_start = origin.map(|p| pos2(p.x * ppp, p.y * ppp));
                    }
                    if response.dragged() {
                        self.drag_now = pointer_px;
                    }
                    let selection = match (self.drag_start, self.drag_now) {
                        (Some(a), Some(b)) => {
                            let r = Rect::from_two_pos(a, b);
                            Some(PxRect {
                                x: r.min.x.round() as i32,
                                y: r.min.y.round() as i32,
                                w: r.width().round() as i32,
                                h: r.height().round() as i32,
                            })
                        }
                        _ => None,
                    };
                    if response.drag_stopped() {
                        self.drag_start = None;
                        self.drag_now = None;
                        if let Some(sel) = selection.filter(|s| s.w >= 3 && s.h >= 3) {
                            return self.select(sel, &ctx);
                        }
                    }
                    if response.clicked() {
                        if let Some(win) = pointer_px.and_then(|p| self.window_at(p)) {
                            return self.select(win, &ctx);
                        }
                    }
                    match selection {
                        Some(sel) => highlight = Some(sel),
                        None => {
                            // Vor dem Ziehen: das Fenster unter der Maus dezent andeuten.
                            if let Some(win) = pointer_px.and_then(|p| self.window_at(p)) {
                                painter.rect_filled(full, 0.0, dim);
                                painter.rect_filled(rect_pt(win), 0.0, Color32::from_white_alpha(10));
                                painter.rect_stroke(rect_pt(win), 0.0, Stroke::new(1.0, Color32::from_white_alpha(70)), StrokeKind::Inside);
                            }
                        }
                    }
                    magnifier = true;
                }
                CaptureMode::Window | CaptureMode::Screen => {
                    let target = pointer_px.and_then(|p| {
                        if self.mode == CaptureMode::Window {
                            self.window_at(p)
                        } else {
                            self.monitor_at(p)
                        }
                    });
                    if response.clicked() {
                        if let Some(t) = target {
                            return self.select(t, &ctx);
                        }
                    }
                    highlight = target;
                }
                CaptureMode::Freeform => {
                    if response.drag_started() {
                        self.lasso.clear();
                        if let Some(p) = ctx.input(|i| i.pointer.press_origin()) {
                            self.lasso.push(pos2(p.x * ppp, p.y * ppp));
                        }
                    }
                    if response.dragged() {
                        if let Some(p) = pointer_px {
                            if self.lasso.last().is_none_or(|l| l.distance(p) >= 2.0) {
                                self.lasso.push(p);
                            }
                        }
                    }
                    if response.drag_stopped() {
                        return self.select_lasso(&ctx);
                    }
                    painter.rect_filled(full, 0.0, dim);
                    if self.lasso.len() > 1 {
                        let points: Vec<Pos2> = self.lasso.iter().map(|p| to_pt(*p)).collect();
                        painter.add(egui::Shape::line(points.clone(), Stroke::new(3.0, Color32::from_black_alpha(120))));
                        painter.add(egui::Shape::line(points, Stroke::new(1.5, Color32::WHITE)));
                    }
                    magnifier = true;
                }
            }

            if let Some(sel) = highlight {
                let r = rect_pt(sel);
                for part in [
                    Rect::from_min_max(full.min, pos2(full.max.x, r.min.y)),
                    Rect::from_min_max(pos2(full.min.x, r.max.y), full.max),
                    Rect::from_min_max(pos2(full.min.x, r.min.y), pos2(r.min.x, r.max.y)),
                    Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(full.max.x, r.max.y)),
                ] {
                    painter.rect_filled(part, 0.0, dim);
                }
                painter.rect_stroke(r, 0.0, Stroke::new(2.0, ui_kit::ACCENT), StrokeKind::Outside);
                size_label(&painter, r, full, &format!("{} × {}", sel.w, sel.h));
            } else if self.mode == CaptureMode::Rect && self.drag_start.is_none() && pointer_px.is_none() {
                painter.rect_filled(full, 0.0, dim);
            }

            if magnifier {
                if let Some(p) = pointer_px {
                    self.magnifier(&painter, p, ppp, full);
                }
            }
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        });

        self.toolbar(&ctx, full);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl Overlay {
    fn toolbar(&mut self, ctx: &egui::Context, full: Rect) {
        let mut chosen = None;
        let mut close = false;
        let mut bar_rect = Rect::NOTHING;
        // Auf dem Bildschirm mit der Maus, sonst dem ersten.
        let monitor = ctx
            .pointer_latest_pos()
            .map(|p| p * ctx.pixels_per_point())
            .and_then(|p| self.monitor_at(p))
            .or_else(|| self.screen.monitors.first().copied());
        let ppp = ctx.pixels_per_point();
        let top_center = monitor
            .map(|m| pos2((m.x as f32 + m.w as f32 / 2.0) / ppp, m.y as f32 / ppp + 14.0))
            .unwrap_or(pos2(full.center().x, 14.0));
        egui::Area::new(egui::Id::unique("aufnahme-leiste"))
            .fixed_pos(top_center)
            .pivot(Align2::CENTER_TOP)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let frame = egui::Frame::new()
                    .fill(ui_kit::GROUP_BG)
                    .stroke(Stroke::new(1.0, ui_kit::DIVIDER))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(5))
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(110),
                    });
                let inner = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for mode in CaptureMode::ALL {
                            if icons::button(ui, mode.glyph(), self.mode == mode, true, mode.label()).clicked() {
                                chosen = Some(mode);
                            }
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                        if icons::button(ui, Glyph::Close, false, true, "Abbrechen (Esc)").clicked() {
                            close = true;
                        }
                    });
                });
                bar_rect = inner.response.rect;
            });
        // Bedienhilfe als kleine Pille unter der Leiste.
        egui::Area::new(egui::Id::unique("aufnahme-hinweis"))
            .fixed_pos(pos2(top_center.x, bar_rect.bottom() + 8.0))
            .pivot(Align2::CENTER_TOP)
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(170))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(12, 5))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(self.mode.hint()).font(ui_kit::regular(ui_kit::SMALL_SIZE)).color(Color32::WHITE));
                    });
            });
        if let Some(mode) = chosen {
            self.set_mode(mode);
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Lupe neben dem Mauszeiger: vergrößerte Pixel, Fadenkreuz, Position und Farbe.
    fn magnifier(&self, painter: &egui::Painter, p: Pos2, ppp: f32, full: Rect) {
        let (w, h) = self.screen.image.dimensions();
        let px = (p.x.floor() as i64).clamp(0, w as i64 - 1) as u32;
        let py = (p.y.floor() as i64).clamp(0, h as i64 - 1) as u32;
        const CELLS: f32 = 15.0;
        let size = 120.0;
        let cursor = pos2(p.x / ppp, p.y / ppp);
        let mut origin = cursor + vec2(24.0, 24.0);
        if origin.x + size > full.max.x {
            origin.x = cursor.x - 24.0 - size;
        }
        if origin.y + size + 40.0 > full.max.y {
            origin.y = cursor.y - 24.0 - size - 40.0;
        }
        let rect = Rect::from_min_size(origin, vec2(size, size));
        let half = CELLS / 2.0;
        // UV ist relativ, gilt also auch für eine verkleinerte Textur.
        let uv = Rect::from_min_max(
            pos2((px as f32 + 0.5 - half) / w as f32, (py as f32 + 0.5 - half) / h as f32),
            pos2((px as f32 + 0.5 + half) / w as f32, (py as f32 + 0.5 + half) / h as f32),
        );
        painter.rect_filled(rect.expand(3.0), CornerRadius::same(6), ui_kit::GROUP_BG);
        painter.image(self.texture.id(), rect, uv, Color32::WHITE);
        let cell = size / CELLS;
        let center = Rect::from_center_size(rect.center(), vec2(cell, cell));
        painter.rect_stroke(center, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        painter.rect_stroke(center.expand(1.0), 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);

        let [r, g, b, _] = self.screen.image.get_pixel(px, py).0;
        let info = Rect::from_min_size(rect.left_bottom() + vec2(-3.0, 6.0), vec2(size + 6.0, 34.0));
        painter.rect_filled(info, CornerRadius::same(6), ui_kit::GROUP_BG);
        painter.rect_filled(
            Rect::from_min_size(info.min + vec2(8.0, 9.0), vec2(16.0, 16.0)),
            CornerRadius::same(3),
            Color32::from_rgb(r, g, b),
        );
        let font = ui_kit::regular(11.0);
        painter.text(info.min + vec2(30.0, 4.0), Align2::LEFT_TOP, format!("#{r:02X}{g:02X}{b:02X}"), font.clone(), ui_kit::TEXT);
        painter.text(info.min + vec2(30.0, 18.0), Align2::LEFT_TOP, format!("{px}, {py}"), font, ui_kit::TEXT_WEAK);
    }
}

fn size_label(painter: &egui::Painter, sel: Rect, full: Rect, text: &str) {
    let font = ui_kit::semibold(ui_kit::SMALL_SIZE);
    let galley = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let size = galley.size() + vec2(12.0, 6.0);
    let mut pos = sel.left_top() - vec2(0.0, size.y + 6.0);
    if pos.y < full.min.y + 4.0 {
        pos.y = sel.min.y + 6.0;
        pos.x += 6.0;
    }
    let bg = Rect::from_min_size(pos, size);
    painter.rect_filled(bg, CornerRadius::same(5), Color32::from_black_alpha(190));
    painter.galley(bg.min + vec2(6.0, 3.0), galley, Color32::WHITE);
}

/// Lädt ein Bild als Textur und verkleinert es, falls es die Grafikkarte nicht schafft.
/// Gibt auch den Faktor zurück (1.0 = Originalgröße).
pub fn upload(ctx: &egui::Context, name: &str, image: &RgbaImage, max_side: u32) -> (egui::TextureHandle, f32) {
    let (w, h) = image.dimensions();
    let longest = w.max(h).max(1);
    let options = egui::TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Linear,
        ..Default::default()
    };
    if longest <= max_side {
        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.as_raw());
        return (ctx.load_texture(name, color, options), 1.0);
    }
    let scale = max_side as f32 / longest as f32;
    let (nw, nh) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let small = image::imageops::resize(image, nw, nh, image::imageops::FilterType::Triangle);
    let color = egui::ColorImage::from_rgba_unmultiplied([nw as usize, nh as usize], small.as_raw());
    (ctx.load_texture(name, color, options), scale)
}
