//! Bild als kleines, rahmenloses Fenster immer im Vordergrund („anheften“), z. B. als
//! Vorlage beim Abtippen. Ziehen verschiebt, Mausrad zoomt, Doppelklick oder Esc schließt.

use std::path::PathBuf;

use eframe::egui::{self, vec2, Color32, Rect, Sense, Stroke, StrokeKind};
use image::RgbaImage;

use super::{export, overlay::upload, EDIT_ARG};
use crate::ui_kit;

pub fn run(path: PathBuf) {
    let image = match export::load(&path) {
        Ok(image) => image,
        Err(e) => return crate::report(Err(e)),
    };
    // Physische Pixel; die tatsächliche Größe setzt das Fenster im ersten Bild (Skalierung bekannt).
    let (w, h) = image.dimensions();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Angeheftet")
            .with_inner_size([w.min(900) as f32, h.min(700) as f32])
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_icon(crate::settings_ui::window_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab-Angeheftet",
        options,
        Box::new(move |cc| {
            ui_kit::apply(&cc.egui_ctx);
            Ok(Box::new(Pin {
                image,
                path,
                texture: None,
                zoom: None,
                ppp: 0.0,
            }))
        }),
    );
    if let Err(e) = result {
        crate::report(Err(format!("Angeheftetes Bild: {e}")));
    }
}

struct Pin {
    image: RgbaImage,
    path: PathBuf,
    /// Erst im ersten Bild hochgeladen: Vorher kennt egui die Texturgrenze der
    /// Grafikkarte nicht und würde große Bilder auf 2048 Pixel verkleinern.
    texture: Option<egui::TextureHandle>,
    /// Bildschirmpixel (physisch) pro Bildpixel; `None` bis zum ersten Bild. In Pixeln statt
    /// Punkten, damit das Bild beim Wechsel auf einen anders skalierten Bildschirm gleich bleibt.
    zoom: Option<f32>,
    /// Skalierung im letzten Bild, um Bildschirmwechsel zu bemerken.
    ppp: f32,
}

impl Pin {
    fn resize(&self, ctx: &egui::Context) {
        if let Some(zoom) = self.zoom {
            let (w, h) = self.image.dimensions();
            let points = vec2(w as f32, h as f32) * zoom / ctx.pixels_per_point();
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(points));
        }
    }
}

impl eframe::App for Pin {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.zoom.is_none() {
            // 1 Bildpixel = 1 Bildschirmpixel, höchstens 60 % des Bildschirms.
            let ppp = ctx.pixels_per_point();
            let (w, h) = self.image.dimensions();
            let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(vec2(1600.0, 900.0)) * ppp;
            let fit = (monitor.x * 0.6 / w as f32).min(monitor.y * 0.6 / h as f32);
            self.zoom = Some(fit.min(1.0));
            self.ppp = ppp;
            self.resize(&ctx);
        } else if (ctx.pixels_per_point() - self.ppp).abs() > 0.001 {
            // Auf einen Bildschirm mit anderer Skalierung gezogen: Windows hat das Fenster
            // mitskaliert, das Bild soll aber scharf in seiner Pixelgröße bleiben.
            self.ppp = ctx.pixels_per_point();
            self.resize(&ctx);
        }
        let texture = self
            .texture
            .get_or_insert_with(|| {
                let max = ctx.input(|i| i.max_texture_side) as u32;
                upload(&ctx, "angeheftet", &self.image, max).0
            })
            .id();

        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(rect, egui::Id::unique("angeheftet"), Sense::click_and_drag());
            ui.painter().image(texture, rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
            ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, ui_kit::ACCENT), StrokeKind::Inside);

            if response.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if response.double_clicked() {
                close = true;
            }
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if response.hovered() && scroll != 0.0 {
                if let Some(zoom) = &mut self.zoom {
                    *zoom = (*zoom * (1.0 + scroll * 0.002)).clamp(0.05, 16.0);
                }
                self.resize(&ctx);
            }
            response
                .on_hover_text("Ziehen verschiebt · Mausrad zoomt · Doppelklick schließt · Rechtsklick für mehr")
                .context_menu(|ui| {
                    if ui.button("Kopieren").clicked() {
                        if let Err(e) = export::copy(&self.image) {
                            crate::log_error(&e);
                        }
                        ui.close();
                    }
                    if ui.button("Bearbeiten").clicked() {
                        crate::report(super::spawn(&[EDIT_ARG, &self.path.to_string_lossy()]));
                        ui.close();
                    }
                    if ui.button("Originalgröße").clicked() {
                        self.zoom = Some(1.0);
                        self.resize(&ctx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Schließen").clicked() {
                        close = true;
                    }
                });
        });
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
