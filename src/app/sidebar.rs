//! Sidebar: page tree, favorites, search results, page menu and rename.
use std::collections::HashSet;

use anyhow::Result;
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::Shift,
    model::{Page, TreeRow, descendants, filter_tree},
};

use super::*;

/// Inline rename in the sidebar.
pub(super) struct Rename {
    pub(super) id: String,
    pub(super) text: String,
    /// Focus the text field on the next frame it is drawn.
    pub(super) focus: bool,
}

/// Something the user asked for on a page row (click, menu, keyboard).
pub(super) enum RowAction {
    Open(String),
    Toggle(String),
    StartRename(String),
    CommitRename,
    CancelRename,
    Subpage(String),
    Favorite(String, bool),
    Reorder(String, Shift),
    Move(String, Option<String>),
    Trash(String),
}

pub(super) fn search_id() -> egui::Id {
    egui::Id::unique("silex-search")
}

impl SilexApp {
    /// Favorites section; hidden when there are none.
    pub(super) fn favorite_rows(&mut self, ui: &mut egui::Ui) {
        if self.favorites.is_empty() {
            return;
        }
        ui.label("Favoritos");
        let rows: Vec<TreeRow> = self
            .favorites
            .iter()
            .map(|page| TreeRow {
                page: page.clone(),
                depth: 0,
                has_children: false,
            })
            .collect();
        self.page_rows(ui, &rows, false, "fav");
        ui.separator();
    }

    /// Page rows (tree, favorites or search): indentation, optional
    /// expand arrow, title or inline rename field, and a context menu.
    /// `section` keeps widget ids distinct when a page appears twice.
    pub(super) fn page_rows(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[TreeRow],
        arrows: bool,
        section: &str,
    ) {
        let mut actions = Vec::new();
        for row in rows {
            ui.push_id((section, &row.page.id), |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 14.0);
                    if arrows {
                        let (_, response) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                        if row.has_children {
                            let openness = if self.expanded.contains(&row.page.id) {
                                1.0
                            } else {
                                0.0
                            };
                            let response = response
                                .on_hover_text("Mostrar u ocultar subpáginas")
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            egui::collapsing_header::paint_default_icon(ui, openness, &response);
                            if response.clicked() {
                                actions.push(RowAction::Toggle(row.page.id.clone()));
                            }
                        }
                    }
                    if let Some(action) = self.row_label(ui, &row.page, section) {
                        actions.push(action);
                    }
                });
            });
        }
        for action in actions {
            let result = self.apply(action);
            self.report(result);
        }
    }

    /// Title (with context menu) or, while renaming, the inline text field.
    pub(super) fn row_label(
        &mut self,
        ui: &mut egui::Ui,
        page: &Page,
        section: &str,
    ) -> Option<RowAction> {
        if let Some(rename) = self.rename.as_mut().filter(|r| r.id == page.id) {
            let response = ui.add(
                TextEdit::singleline(&mut rename.text)
                    .id(egui::Id::unique(("silex-rename", section, &page.id)))
                    .desired_width(f32::INFINITY),
            );
            if rename.focus {
                response.request_focus();
                rename.focus = false;
            }
            if response.lost_focus() {
                return Some(if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    RowAction::CancelRename
                } else {
                    RowAction::CommitRename
                });
            }
            return None;
        }
        let selected = self.selected.as_deref() == Some(page.id.as_str());
        let response = ui.selectable_label(selected, display_title(&page.title));
        let mut action = response.clicked().then(|| RowAction::Open(page.id.clone()));
        response.context_menu(|ui| {
            if let Some(chosen) = self.page_menu(ui, &page.id) {
                action = Some(chosen);
                ui.close();
            }
        });
        action
    }

    /// Context menu for a page.
    pub(super) fn page_menu(&self, ui: &mut egui::Ui, id: &str) -> Option<RowAction> {
        let page = self.pages.iter().find(|p| p.id == id)?;
        let siblings: Vec<&str> = self
            .pages
            .iter()
            .filter(|p| p.parent_id == page.parent_id)
            .map(|p| p.id.as_str())
            .collect();
        let index = siblings.iter().position(|s| *s == id);
        let is_favorite = self.favorites.iter().any(|p| p.id == id);
        let owned = || id.to_owned();
        let mut action = None;
        if ui.button("Abrir").clicked() {
            action = Some(RowAction::Open(owned()));
        }
        if ui.button("Renombrar").clicked() {
            action = Some(RowAction::StartRename(owned()));
        }
        if ui.button("Nueva subpágina").clicked() {
            action = Some(RowAction::Subpage(owned()));
        }
        let favorite_label = if is_favorite {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        };
        if ui.button(favorite_label).clicked() {
            action = Some(RowAction::Favorite(owned(), !is_favorite));
        }
        ui.separator();
        if ui
            .add_enabled(index.is_some_and(|i| i > 0), egui::Button::new("Subir"))
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Up));
        }
        if ui
            .add_enabled(
                index.is_some_and(|i| i + 1 < siblings.len()),
                egui::Button::new("Bajar"),
            )
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Down));
        }
        ui.menu_button("Mover a…", |ui| {
            let blocked = descendants(&self.pages, id);
            egui::ScrollArea::vertical()
                .id_salt("silex_menu_move")
                .max_height(240.0)
                .show(ui, |ui| {
                    if page.parent_id.is_some() && ui.button("Raíz").clicked() {
                        action = Some(RowAction::Move(owned(), None));
                    }
                    for target in &self.pages {
                        let valid = target.id != id
                            && !blocked.contains(&target.id)
                            && page.parent_id.as_deref() != Some(target.id.as_str());
                        if valid && ui.button(display_title(&target.title)).clicked() {
                            action = Some(RowAction::Move(owned(), Some(target.id.clone())));
                        }
                    }
                });
        });
        ui.separator();
        if ui.button("Enviar a la papelera").clicked() {
            action = Some(RowAction::Trash(owned()));
        }
        action
    }

    pub(super) fn apply(&mut self, action: RowAction) -> Result<()> {
        match action {
            RowAction::Open(id) => self.open_page(id),
            RowAction::Toggle(id) => {
                self.toggle_expanded(&id);
                Ok(())
            }
            RowAction::StartRename(id) => {
                self.start_rename(&id);
                Ok(())
            }
            RowAction::CommitRename => self.commit_rename(),
            RowAction::CancelRename => {
                self.rename = None;
                Ok(())
            }
            RowAction::Subpage(id) => self.create_subpage(&id),
            RowAction::Favorite(id, favorite) => self.set_favorite(&id, favorite),
            RowAction::Reorder(id, shift) => self.reorder_page(&id, shift),
            RowAction::Move(id, parent) => self.move_page(&id, parent.as_deref()),
            RowAction::Trash(id) => self.trash_page(&id),
        }
    }

    pub(super) fn start_rename(&mut self, id: &str) {
        let current = if self.selected.as_deref() == Some(id) {
            // The open page may have an unsaved title in the editor.
            Some(self.title.clone())
        } else {
            self.pages
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.title.clone())
        };
        if let Some(text) = current {
            self.rename = Some(Rename {
                id: id.to_owned(),
                text,
                focus: true,
            });
        }
    }

    pub(super) fn commit_rename(&mut self) -> Result<()> {
        let Some(rename) = self.rename.take() else {
            return Ok(());
        };
        let title = match rename.text.trim() {
            "" => "Sin título".to_owned(),
            t => t.to_owned(),
        };
        if self.selected.as_deref() == Some(rename.id.as_str()) {
            // Keep the editor and the database in agreement: save through the document.
            if title != self.title {
                self.title = title;
                self.mark_dirty();
                self.save_now()?;
            }
            Ok(())
        } else {
            self.db.rename_page(&rename.id, &title)?;
            self.reload_pages()
        }
    }

    /// Recomputes search results when the query or the pages changed.
    pub(super) fn refresh_search(&mut self) {
        if self.searched_query.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.searched_query = Some(self.query.clone());
        self.filtered = filter_tree(&self.pages, &self.query);
        let shown: HashSet<&str> = self.filtered.iter().map(|r| r.page.id.as_str()).collect();
        self.content_matches = match self.db.search_pages(&self.query) {
            Ok(found) => found
                .into_iter()
                .filter(|p| !shown.contains(p.id.as_str()))
                .collect(),
            Err(err) => {
                self.notice = Some(format!("Error de búsqueda: {err:#}"));
                Vec::new()
            }
        };
    }

    /// Ids of the page rows currently visible, top to bottom (for keyboard navigation).
    pub(super) fn visible_page_ids(&self) -> Vec<String> {
        if self.query.trim().is_empty() {
            self.tree.iter().map(|r| r.page.id.clone()).collect()
        } else {
            self.filtered
                .iter()
                .map(|r| &r.page)
                .chain(&self.content_matches)
                .map(|p| p.id.clone())
                .collect()
        }
    }

    pub(super) fn open_adjacent(&mut self, step: isize) -> Result<()> {
        let ids = self.visible_page_ids();
        if ids.is_empty() {
            return Ok(());
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|sel| ids.iter().position(|id| id == sel));
        let next = match current {
            Some(i) => i.saturating_add_signed(step).min(ids.len() - 1),
            None => 0,
        };
        if Some(next) == current {
            return Ok(());
        }
        self.open_page(ids[next].clone())
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Silex");
        ui.label("Tu espacio local");
        ui.separator();
        ui.label("Buscar (título y contenido)");
        ui.add(
            TextEdit::singleline(&mut self.query)
                .id(search_id())
                .hint_text(format!("Buscar… ({}+K)", cmd_label(ui.ctx()))),
        );
        ui.separator();
        if self.query.trim().is_empty() {
            self.favorite_rows(ui);
            ui.label("Páginas");
            let rows = self.tree.clone();
            egui::ScrollArea::vertical()
                .id_salt("silex_sidebar_tree")
                .max_height(400.0)
                .show(ui, |ui| self.page_rows(ui, &rows, true, "tree"));
        } else {
            self.search_results(ui);
        }
        ui.separator();
        ui.add(TextEdit::singleline(&mut self.new_page_title).hint_text("Nombre de página"));
        if ui.button("+ Crear página").clicked() {
            let title = match self.new_page_title.trim() {
                "" => "Sin título".to_owned(),
                t => t.to_owned(),
            };
            let result = self.create_root_page(&title);
            if result.is_ok() {
                self.new_page_title.clear();
            }
            self.report(result);
        }
        if ui
            .button(format!("Papelera ({})", self.trash_count))
            .clicked()
        {
            self.open_trash_dialog();
        }
        let can_export = self.export_job.is_none() && self.db.path().is_some();
        let label = if self.export_job.is_some() {
            "Exportando…"
        } else {
            "Exportar todo a Markdown"
        };
        if ui
            .add_enabled(can_export, egui::Button::new(label))
            .on_hover_text("Todas las páginas (sin la papelera) en una carpeta nueva")
            .clicked()
        {
            let result = silex_desktop::export::export_root()
                .and_then(|root| self.start_workspace_export(ui.ctx(), root));
            self.report(result);
        }
        Self::shortcuts_help(ui);
        ui.separator();
        ui.small(&self.backup_label);
        let can_backup = self.backup_job.is_none() && self.db.path().is_some();
        if ui
            .add_enabled(can_backup, egui::Button::new("Crear copia ahora"))
            .clicked()
        {
            self.start_backup(ui.ctx());
        }
        if ui
            .add_enabled(can_backup, egui::Button::new("Restaurar una copia…"))
            .clicked()
        {
            self.open_restore_dialog();
        }
    }

    /// Search: title matches as a filtered tree, then pages that only match
    /// in their content (FTS, which also ignores accents).
    pub(super) fn search_results(&mut self, ui: &mut egui::Ui) {
        self.refresh_search();
        let filtered = self.filtered.clone();
        let content: Vec<TreeRow> = self
            .content_matches
            .iter()
            .map(|page| TreeRow {
                page: page.clone(),
                depth: 0,
                has_children: false,
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("silex_sidebar_search")
            .max_height(400.0)
            .show(ui, |ui| {
                ui.label("Páginas");
                if filtered.is_empty() {
                    ui.weak("Ningún título coincide.");
                }
                self.page_rows(ui, &filtered, false, "filter");
                if !content.is_empty() {
                    ui.add_space(6.0);
                    ui.label("En el contenido");
                    self.page_rows(ui, &content, false, "content");
                }
            });
    }

    pub(super) fn shortcuts_help(ui: &mut egui::Ui) {
        let cmd = cmd_label(ui.ctx());
        ui.collapsing("Atajos de teclado", |ui| {
            for (keys, what) in [
                (format!("{cmd}+N"), "Nueva página"),
                (format!("{cmd}+Mayús+N"), "Nueva subpágina"),
                (format!("{cmd}+K"), "Buscar"),
                ("Alt+↑ / Alt+↓".to_owned(), "Página anterior / siguiente"),
                ("F2".to_owned(), "Renombrar la página abierta"),
                (format!("{cmd}+S"), "Guardar ahora"),
                (
                    format!("{cmd}+Z / {cmd}+Mayús+Z"),
                    "Deshacer / rehacer texto",
                ),
                ("Clic derecho".to_owned(), "Acciones de una página"),
            ] {
                ui.horizontal(|ui| {
                    ui.monospace(keys);
                    ui.label(what);
                });
            }
        });
    }
}
