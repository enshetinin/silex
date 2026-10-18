use std::time::{Duration, Instant};

use anyhow::Result;
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::Database,
    model::{Block, Page},
};

pub struct SilexApp {
    db: Database,
    pages: Vec<Page>,
    selected: Option<String>,
    title: String,
    blocks: Vec<Block>,
    new_page_title: String,
    query: String,
    status: String,
    modified_at: Option<Instant>,
}

impl SilexApp {
    pub fn new(db: Database) -> Result<Self> {
        let mut pages = db.list_pages()?;
        if pages.is_empty() {
            db.create_page("Mi primera página", None)?;
            pages = db.list_pages()?;
        }
        let mut app = Self {
            db,
            pages,
            selected: None,
            title: String::new(),
            blocks: Vec::new(),
            new_page_title: String::new(),
            query: String::new(),
            status: "Guardado localmente".into(),
            modified_at: None,
        };
        if let Some(page) = app.pages.first() {
            app.open_page(page.id.clone())?;
        }
        Ok(app)
    }

    fn open_page(&mut self, id: String) -> Result<()> {
        self.save_if_dirty()?;
        let page = self.db.get_page(&id)?;
        if let Some(page) = page {
            self.blocks = self.db.list_blocks(&id)?;
            self.title = page.title;
            self.selected = Some(id);
            self.status = "Guardado localmente".into();
        }
        Ok(())
    }

    fn reload_pages(&mut self) -> Result<()> {
        self.pages = self.db.list_pages()?;
        Ok(())
    }

    fn mark_dirty(&mut self) {
        self.modified_at = Some(Instant::now());
        self.status = "Cambios sin guardar…".into();
    }

    fn save_if_dirty(&mut self) -> Result<()> {
        if self.modified_at.is_none() {
            return Ok(());
        }
        if let Some(ref id) = self.selected {
            self.db.save_document(id, &self.title, &self.blocks)?;
            self.reload_pages()?;
        }
        self.modified_at = None;
        self.status = "Guardado localmente".into();
        Ok(())
    }

    fn report<T>(&mut self, result: Result<T>) {
        if let Err(err) = result {
            self.status = format!("Error: {err:#}");
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Silex");
        ui.label("Tu espacio local");
        ui.separator();
        ui.label("Buscar (título y contenido)");
        ui.add(TextEdit::singleline(&mut self.query).hint_text("Buscar…"));
        ui.separator();
        ui.label("Páginas");
        let found = if self.query.is_empty() {
            Ok(self.pages.clone())
        } else {
            self.db.search_pages(&self.query)
        };
        match found {
            Ok(pages) => {
                let mut open = None;
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for page in &pages {
                            let selected = self.selected.as_deref() == Some(&page.id);
                            if ui.selectable_label(selected, &page.title).clicked() {
                                open = Some(page.id.clone());
                            }
                        }
                    });
                if let Some(id) = open {
                    let result = self.open_page(id);
                    self.report(result);
                }
            }
            Err(err) => self.status = format!("Error de búsqueda: {err}"),
        }
        ui.separator();
        ui.add(TextEdit::singleline(&mut self.new_page_title).hint_text("Nombre de página"));
        if ui.button("+ Crear página").clicked() {
            let result = (|| -> Result<()> {
                self.save_if_dirty()?;
                let title = self.new_page_title.trim();
                let title = if title.is_empty() {
                    "Sin título"
                } else {
                    title
                };
                let id = self.db.create_page(title, None)?;
                self.new_page_title.clear();
                self.reload_pages()?;
                self.open_page(id)
            })();
            self.report(result);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(page_id) = self.selected.clone() else {
            ui.label("Crea una página para empezar.");
            return;
        };
        ui.horizontal(|ui| {
            ui.heading("Documento");
            if ui.button("Guardar").clicked() {
                let result = self.save_if_dirty();
                self.report(result);
            }
        });
        ui.separator();
        let changed = ui
            .add(
                TextEdit::singleline(&mut self.title)
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            )
            .changed();
        if changed {
            self.mark_dirty();
        }
        ui.add_space(14.0);
        let mut modified = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for block in &mut self.blocks {
                ui.horizontal(|ui| {
                    if block.is_todo() {
                        modified |= ui.checkbox(&mut block.checked, "").changed();
                    }
                    if block.kind == "heading" {
                        modified |= ui
                            .add(
                                TextEdit::singleline(&mut block.content)
                                    .font(egui::TextStyle::Heading)
                                    .desired_width(f32::INFINITY),
                            )
                            .changed();
                    } else {
                        modified |= ui
                            .add(
                                TextEdit::multiline(&mut block.content)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            )
                            .changed();
                    }
                });
                ui.add_space(8.0);
            }
        });
        if modified {
            self.mark_dirty();
        }
        ui.separator();
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("+ Texto", "paragraph"),
                ("+ Título", "heading"),
                ("+ Tarea", "todo"),
            ] {
                if ui.button(label).clicked() {
                    let result = (|| -> Result<()> {
                        self.save_if_dirty()?;
                        self.db.add_block(&page_id, kind)?;
                        self.blocks = self.db.list_blocks(&page_id)?;
                        Ok(())
                    })();
                    self.report(result);
                }
            }
        });
        ui.add_space(16.0);
        ui.small(&self.status);
    }
}

impl eframe::App for SilexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(230.0);
                self.sidebar(ui);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(500.0);
                self.editor(ui);
            });
        });
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            let result = self.save_if_dirty();
            self.report(result);
        }
        if self
            .modified_at
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(650))
        {
            let result = self.save_if_dirty();
            self.report(result);
        }
        // UI may be idle between edits; schedule a repaint for the autosave timer.
        if self.modified_at.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Explicit shutdown hook: flush pending edits before the window closes.
    fn on_exit(&mut self) {
        if let Err(err) = self.save_if_dirty() {
            eprintln!("Silex: no se guardaron cambios al cerrar: {err:#}");
        }
    }
}

/// Safety net in case `on_exit` did not run; a second save is a no-op.
impl Drop for SilexApp {
    fn drop(&mut self) {
        if let Err(err) = self.save_if_dirty() {
            eprintln!("Silex: no se guardaron cambios al cerrar: {err:#}");
        }
    }
}
