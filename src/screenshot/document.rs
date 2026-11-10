//! Das bearbeitete Bild: Ausgangsbild plus Anmerkungen in Zeichenreihenfolge,
//! dazu ein einfacher Verlauf aus Schnappschüssen (Rückgängig/Wiederholen).

use std::sync::Arc;

use eframe::egui::{Rect, Vec2};
use image::RgbaImage;

use super::annotations::Annotation;
use super::canvas::{Canvas, IRect};

#[derive(Clone)]
pub struct Document {
    /// Geteilt, damit Schnappschüsse billig sind.
    pub base: Arc<RgbaImage>,
    pub items: Vec<Box<dyn Annotation>>,
}

impl Document {
    pub fn new(image: RgbaImage) -> Self {
        Self {
            base: Arc::new(image),
            items: Vec::new(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.base.dimensions()
    }

    pub fn image_rect(&self) -> Rect {
        let (w, h) = self.size();
        IRect::new(0, 0, w as i32, h as i32).to_rect()
    }

    /// Fertiges Bild mit allen Anmerkungen.
    pub fn compose(&self) -> Canvas {
        let mut canvas = Canvas::from_image(&self.base);
        for item in &self.items {
            item.render(&mut canvas);
        }
        canvas
    }

    /// Oberste Anmerkung unter `p`.
    pub fn hit(&self, p: eframe::egui::Pos2, tolerance: f32) -> Option<usize> {
        self.items.iter().rposition(|item| item.hit(p, tolerance))
    }

    /// Schneidet Bild und Anmerkungen auf `rect` zu.
    pub fn crop(&mut self, rect: IRect) {
        let (w, h) = self.size();
        let Some(r) = rect.intersect(&IRect::new(0, 0, w as i32, h as i32)) else {
            return;
        };
        let cropped = image::imageops::crop_imm(&*self.base, r.x as u32, r.y as u32, r.w as u32, r.h as u32).to_image();
        self.base = Arc::new(cropped);
        let delta = Vec2::new(-r.x as f32, -r.y as f32);
        for item in &mut self.items {
            item.translate(delta);
        }
    }

    pub fn next_step_number(&self) -> u32 {
        self.items.iter().filter_map(|i| i.step_number()).max().unwrap_or(0) + 1
    }
}

pub struct History {
    pub doc: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// Zählt jede Änderung; daran erkennt der Editor, wann neu gerendert werden muss.
    revision: u64,
}

impl History {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Merkt den jetzigen Stand fürs Rückgängigmachen. Einmal pro Geste aufrufen,
    /// danach beliebig oft über [`History::doc_mut`] ändern.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.doc.clone());
        self.redo.clear();
        // Speicher begrenzen; das Bild selbst ist geteilt, nur Zuschneiden erzeugt neue Bilder.
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    pub fn doc_mut(&mut self) -> &mut Document {
        self.revision += 1;
        &mut self.doc
    }

    /// Schnappschuss + Änderung in einem.
    pub fn edit(&mut self, change: impl FnOnce(&mut Document)) {
        self.checkpoint();
        change(self.doc_mut());
    }

    pub fn push(&mut self, item: Box<dyn Annotation>) {
        self.edit(|doc| doc.items.push(item));
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, previous));
            self.revision += 1;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.revision += 1;
        }
    }

    /// Neues Bild laden (z. B. per Drag & Drop) – rückgängig machbar.
    pub fn replace(&mut self, doc: Document) {
        self.checkpoint();
        *self.doc_mut() = doc;
    }
}
