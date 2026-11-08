//! Fenster mit allen Einstellungen: Format, Qualität, Größe, Zieldateigröße.
//! Dateien kommen aus dem Kontextmenü (auch nachträglich) oder per Drag & Drop.

use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use rayon::prelude::*;

use super::batch::Queue;
use super::{convert_file, copy_to_clipboard, describe, human_size, Format, Options, Outcome, Resize, INPUT_EXTENSIONS};
use crate::settings_ui::{accent_button, apply_theme, window_icon, ACCENT, CARD_BG, PAGE_BG};

const GREEN: Color32 = Color32::from_rgb(0x3C, 0xA0, 0x50);
const RED: Color32 = Color32::from_rgb(0xD0, 0x40, 0x30);

pub fn run(files: Vec<PathBuf>, queue: Option<Queue>) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("UtilityCrab – Bilder konvertieren")
            .with_inner_size([700.0, 640.0])
            .with_min_inner_size([560.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    let result = eframe::run_native(
        "UtilityCrab-Konverter",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            let mut app = ConverterApp::new(queue);
            app.add_files(files);
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = result {
        crate::report(Err(format!("Konverter-Fenster: {e}")));
    }
}

#[derive(Clone)]
enum Status {
    Ready,
    Working,
    Done(Outcome),
    Failed(String),
}

struct Item {
    path: PathBuf,
    size: u64,
    status: Arc<Mutex<Status>>,
    /// Fortschritt in Promille.
    progress: Arc<AtomicU32>,
}

struct ConverterApp {
    items: Vec<Item>,
    options: Options,
    saved: Options,
    queue: Option<Queue>,
    last_poll: Instant,
    running: Arc<Mutex<bool>>,
    /// Zuletzt eingestellte Werte, damit sie beim Umschalten der Größenart erhalten bleiben.
    long_edge: u32,
    percent: u32,
    max_kb: u32,
    message: Option<String>,
    /// Hinweis nach dem Durchlauf, z. B. dass die Dateien in der Zwischenablage liegen.
    notice: Arc<Mutex<Option<String>>>,
}

fn options_path() -> PathBuf {
    crate::data_dir().join("konverter.json")
}

impl ConverterApp {
    fn new(queue: Option<Queue>) -> Self {
        let options: Options = fs::read_to_string(options_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let long_edge = match options.resize {
            Resize::LongEdge(v) => v,
            _ => 1920,
        };
        let percent = match options.resize {
            Resize::Percent(v) => v,
            _ => 50,
        };
        Self {
            items: Vec::new(),
            saved: options.clone(),
            max_kb: options.max_kb.unwrap_or(500),
            options,
            queue,
            last_poll: Instant::now(),
            running: Arc::new(Mutex::new(false)),
            long_edge,
            percent,
            message: None,
            notice: Arc::default(),
        }
    }

    fn is_running(&self) -> bool {
        *self.running.lock().unwrap()
    }

    fn add_files(&mut self, files: Vec<PathBuf>) {
        for path in files {
            if path.is_dir() {
                // Ordner: die Bilder darin (nicht rekursiv) übernehmen.
                if let Ok(dir) = fs::read_dir(&path) {
                    let mut inner: Vec<PathBuf> = dir.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
                    inner.sort();
                    self.add_files(inner.into_iter().filter(|p| is_image(p)).collect());
                }
                continue;
            }
            if !is_image(&path) {
                self.message = Some(format!("{} ist kein unterstütztes Bild.", file_name(&path)));
                continue;
            }
            if self.items.iter().any(|i| i.path == path) {
                continue;
            }
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.items.push(Item {
                path,
                size,
                status: Arc::new(Mutex::new(Status::Ready)),
                progress: Arc::default(),
            });
        }
    }

    fn start(&mut self, ctx: egui::Context) {
        let jobs: Vec<(PathBuf, Arc<Mutex<Status>>, Arc<AtomicU32>)> = self
            .items
            .iter()
            .map(|i| {
                *i.status.lock().unwrap() = Status::Ready;
                i.progress.store(0, Ordering::Relaxed);
                (i.path.clone(), i.status.clone(), i.progress.clone())
            })
            .collect();
        let options = self.options.clone();
        let running = self.running.clone();
        let notice = self.notice.clone();
        *running.lock().unwrap() = true;
        *notice.lock().unwrap() = None;
        self.message = None;
        thread::spawn(move || {
            jobs.par_iter().for_each(|(path, status, progress)| {
                *status.lock().unwrap() = Status::Working;
                ctx.request_repaint();
                let on_progress = |p: f32| {
                    progress.store((p * 1000.0) as u32, Ordering::Relaxed);
                    ctx.request_repaint();
                };
                let result = match convert_file(path, &options, &on_progress) {
                    Ok(outcome) => Status::Done(outcome),
                    Err(e) => {
                        crate::log_error(&e);
                        Status::Failed(e)
                    }
                };
                *status.lock().unwrap() = result;
                progress.store(1000, Ordering::Relaxed);
                ctx.request_repaint();
            });

            let written: Vec<PathBuf> = jobs
                .iter()
                .filter_map(|(_, status, _)| match &*status.lock().unwrap() {
                    Status::Done(Outcome::Written { path, .. }) => Some(path.clone()),
                    _ => None,
                })
                .collect();
            if !written.is_empty() {
                *notice.lock().unwrap() = Some(match copy_to_clipboard(&written) {
                    Ok(()) if written.len() == 1 => "Die neue Datei liegt in der Zwischenablage (Strg+V zum Einfügen).".into(),
                    Ok(()) => format!("Die {} neuen Dateien liegen in der Zwischenablage (Strg+V zum Einfügen).", written.len()),
                    Err(e) => e,
                });
            }
            *running.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }

    fn written_files(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter_map(|i| match &*i.status.lock().unwrap() {
                Status::Done(Outcome::Written { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }
}

impl eframe::App for ConverterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Dateien, die nachträglich per Kontextmenü dazukommen.
        if self.last_poll.elapsed() > Duration::from_millis(500) {
            self.last_poll = Instant::now();
            if let Some(queue) = &self.queue {
                let files = queue.drain();
                if !files.is_empty() {
                    self.add_files(files);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
        // Per Drag & Drop hineingezogene Dateien.
        let dropped: Vec<PathBuf> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() && !self.is_running() {
            self.add_files(dropped);
        }
        let hovering = ui.input(|i| !i.raw.hovered_files.is_empty());

        let page = egui::Frame::new().fill(PAGE_BG).inner_margin(egui::Margin::symmetric(24, 16));
        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Bilder konvertieren & verkleinern");
                ui.label(RichText::new("Die Originale bleiben unverändert, neue Dateien landen daneben.").weak());
                ui.add_space(10.0);
                self.file_list(ui, hovering);
                ui.add_space(12.0);
                self.settings(ui);
                ui.add_space(14.0);
                self.actions(ui, &ctx);
                if let Some(message) = &self.message {
                    ui.add_space(8.0);
                    ui.colored_label(RED, message);
                }
            });
        });

        if self.options != self.saved {
            if let Ok(json) = serde_json::to_string_pretty(&self.options) {
                let _ = fs::create_dir_all(crate::data_dir());
                let _ = fs::write(options_path(), json);
            }
            self.saved = self.options.clone();
        }

        // Warteschlange regelmäßig ansehen, auch ohne Mausbewegung.
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl ConverterApp {
    fn file_list(&mut self, ui: &mut egui::Ui, hovering: bool) {
        let running = self.is_running();
        let stroke = if hovering {
            egui::Stroke::new(2.0, ACCENT)
        } else {
            egui::Stroke::NONE
        };
        let mut remove = None;
        egui::Frame::new()
            .fill(CARD_BG)
            .stroke(stroke)
            .inner_margin(egui::Margin::same(14))
            .corner_radius(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.items.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Bilder oder Ordner hierher ziehen").size(17.0));
                        ui.label(
                            RichText::new("PNG, JPG, WebP, BMP, GIF, TIFF, ICO, HEIC, AVIF")
                                .small()
                                .weak(),
                        );
                    });
                    ui.add_space(24.0);
                    return;
                }
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    egui::Grid::new("files")
                        .num_columns(3)
                        .spacing([12.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for (index, item) in self.items.iter().enumerate() {
                                ui.label(file_name(&item.path)).on_hover_text(item.path.display().to_string());
                                match &*item.status.lock().unwrap() {
                                    Status::Ready => ui.label(RichText::new(human_size(item.size)).weak()),
                                    Status::Working => ui.add(
                                        egui::ProgressBar::new(item.progress.load(Ordering::Relaxed) as f32 / 1000.0)
                                            .desired_width(160.0)
                                            .show_percentage(),
                                    ),
                                    Status::Done(outcome @ Outcome::Written { .. }) => {
                                        ui.label(RichText::new(describe(outcome)).color(GREEN))
                                    }
                                    Status::Done(outcome) => ui.label(RichText::new(describe(outcome)).weak()),
                                    Status::Failed(e) => {
                                        let short = e.split_once(": ").map(|(_, m)| m).unwrap_or(e);
                                        ui.label(RichText::new(short).color(RED))
                                    }
                                };
                                if ui.add_enabled(!running, egui::Button::new("Entfernen").small()).clicked() {
                                    remove = Some(index);
                                }
                                ui.end_row();
                            }
                        });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} Dateien – weitere hierher ziehen", self.items.len())).weak());
                    if ui.add_enabled(!running, egui::Button::new("Liste leeren").small()).clicked() {
                        self.items.clear();
                    }
                });
            });
        if let Some(index) = remove {
            self.items.remove(index);
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Vorlagen:").weak());
            if ui.button("Für E-Mail").on_hover_text("JPG, max. 1600 px, Qualität 75").clicked() {
                self.options = Options {
                    format: Some(Format::Jpg),
                    quality: 75,
                    resize: Resize::LongEdge(1600),
                    max_kb: None,
                };
                self.long_edge = 1600;
            }
            if ui.button("Für Webseiten").on_hover_text("WebP, max. 1920 px, Qualität 80").clicked() {
                self.options = Options {
                    format: Some(Format::Webp),
                    quality: 80,
                    resize: Resize::LongEdge(1920),
                    max_kb: None,
                };
                self.long_edge = 1920;
            }
            if ui.button("Unter 1 MB").on_hover_text("JPG, höchstens 1 MB pro Bild").clicked() {
                self.options = Options {
                    format: Some(Format::Jpg),
                    quality: 90,
                    resize: Resize::None,
                    max_kb: Some(1000),
                };
                self.max_kb = 1000;
            }
            if ui.button("Nur verkleinern").on_hover_text("Format behalten, sparsamer speichern").clicked() {
                self.options = Options::shrink();
            }
        });
        ui.add_space(8.0);

        let format = self.options.format;
        // Mit „Wie Original“ kann das Ziel JPG/WebP sein, Qualität also einstellbar lassen.
        let lossy = format.is_none_or(Format::lossy);

        egui::Grid::new("options")
            .num_columns(2)
            .spacing([16.0, 12.0])
            .show(ui, |ui| {
                ui.label("Zielformat");
                let current = format.map(Format::label).unwrap_or("Wie Original");
                egui::ComboBox::from_id_salt("format")
                    .selected_text(current)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.options.format, None, "Wie Original")
                            .on_hover_text("BMP/TIFF werden zu PNG, HEIC/AVIF zu JPG");
                        for f in Format::ALL {
                            ui.selectable_value(&mut self.options.format, Some(f), f.label());
                        }
                    });
                ui.end_row();

                ui.label("Qualität");
                ui.add_enabled_ui(lossy, |ui| {
                    ui.add(egui::Slider::new(&mut self.options.quality, 1..=100))
                        .on_hover_text("Nur für JPG und WebP. 75–85 ist meist kaum vom Original zu unterscheiden. WebP mit 100 = verlustfrei.");
                });
                ui.end_row();

                ui.label("Bildgröße");
                ui.horizontal(|ui| {
                    let mut kind = match self.options.resize {
                        Resize::None => 0,
                        Resize::LongEdge(_) => 1,
                        Resize::Percent(_) => 2,
                    };
                    let labels = ["Nicht ändern", "Längste Seite höchstens", "Prozent"];
                    egui::ComboBox::from_id_salt("resize")
                        .selected_text(labels[kind])
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (i, label) in labels.iter().enumerate() {
                                ui.selectable_value(&mut kind, i, *label);
                            }
                        });
                    match kind {
                        1 => {
                            ui.add(egui::DragValue::new(&mut self.long_edge).range(16..=20000).suffix(" px"));
                            self.options.resize = Resize::LongEdge(self.long_edge);
                        }
                        2 => {
                            ui.add(egui::DragValue::new(&mut self.percent).range(1..=400).suffix(" %"));
                            self.options.resize = Resize::Percent(self.percent);
                        }
                        _ => self.options.resize = Resize::None,
                    }
                });
                ui.end_row();

                ui.label("Dateigröße");
                ui.add_enabled_ui(lossy, |ui| {
                    ui.horizontal(|ui| {
                        let mut limit = self.options.max_kb.is_some();
                        ui.checkbox(&mut limit, "Höchstens");
                        ui.add_enabled(limit, egui::DragValue::new(&mut self.max_kb).range(10..=100_000).suffix(" KB"));
                        self.options.max_kb = limit.then_some(self.max_kb);
                    })
                    .response
                    .on_hover_text("Senkt Qualität und notfalls Auflösung, bis die Datei klein genug ist. Nur JPG und WebP.");
                });
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new("Metadaten wie Kamera und GPS-Position werden dabei entfernt.").weak(),
        );
    }

    fn actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let running = self.is_running();
        if running {
            let total: u32 = self.items.iter().map(|i| i.progress.load(Ordering::Relaxed)).sum();
            let fraction = total as f32 / (1000 * self.items.len().max(1)) as f32;
            ui.add(egui::ProgressBar::new(fraction).show_percentage().fill(ACCENT));
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            let label = if running { "Läuft …" } else { "Konvertieren" };
            let enabled = !running && !self.items.is_empty();
            if ui.add_enabled(enabled, accent_button(label).min_size(egui::vec2(160.0, 34.0))).clicked() {
                self.start(ctx.clone());
            }
            let written = self.written_files();
            if !running && !written.is_empty() {
                if ui.button("Im Explorer zeigen").clicked() {
                    // raw_arg, weil Explorer das Anführungszeichen erst nach dem Komma erwartet.
                    let _ = std::process::Command::new("explorer")
                        .raw_arg(format!("/select,\"{}\"", written[0].display()))
                        .spawn();
                }
                let (before, after) = self.items.iter().fold((0u64, 0u64), |(b, a), item| {
                    match &*item.status.lock().unwrap() {
                        Status::Done(Outcome::Written { before, after, .. }) => (b + before, a + after),
                        _ => (b, a),
                    }
                });
                ui.label(RichText::new(format!("Gesamt: von {} auf {}", human_size(before), human_size(after))).weak());
            }
        });
        if !running {
            if let Some(notice) = &*self.notice.lock().unwrap() {
                ui.add_space(6.0);
                ui.label(RichText::new(notice).color(GREEN));
            }
        }
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INPUT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
