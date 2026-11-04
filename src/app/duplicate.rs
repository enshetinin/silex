//! Duplicating a page, a row or a database (H11.4): «Duplicar» in the
//! page menu, the row's «⋯», the palette and the selection bar. A page
//! with subpages (or a database with rows) asks whether they come along.
//! A copied page or database opens; a copied row stays in its table, right
//! below the original, and Cmd+Z takes it back (to the trash).
use anyhow::Result;
use eframe::egui;
use silex_desktop::model::PageKind;

use super::*;

/// «Duplicar» waiting for an answer: with the subpages (rows) or without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DuplicateDialog {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) database: bool,
    /// How many live pages are right below it.
    pub(super) children: usize,
}

impl DatabaseState {
    /// Puts the row `copy` right below `original` in the view (rows read
    /// again go to the end), and keeps it in sight whatever the filters.
    pub(super) fn place_after(&mut self, original: &str, copy: &str) {
        let index_of = |id: &str| self.rows.iter().position(|r| r.id == id);
        let (Some(original), Some(copy_index)) = (index_of(original), index_of(copy)) else {
            return;
        };
        self.order.retain(|&i| i != copy_index);
        let at = self
            .order
            .iter()
            .position(|&i| i == original)
            .map_or(self.order.len(), |at| at + 1);
        self.order.insert(at, copy_index);
        self.pinned.insert(copy.to_owned());
    }
}

impl SilexApp {
    /// «Duplicar»: asks first if the page has subpages (or rows).
    pub(super) fn start_duplicate(&mut self, id: &str) -> Result<()> {
        let Some(page) = self.pages.iter().find(|p| p.id == id) else {
            return Ok(());
        };
        let children = self
            .pages
            .iter()
            .filter(|p| p.parent_id.as_deref() == Some(id))
            .count();
        if children == 0 {
            return self.duplicate(id, false);
        }
        self.duplicate_dialog = Some(DuplicateDialog {
            id: id.to_owned(),
            title: display_title(&page.title).to_owned(),
            database: page.kind == PageKind::Database,
            children,
        });
        Ok(())
    }

    /// Duplicates `id` (with its subpages if `children`). A row of the
    /// open table stays there, below the original; anything else opens.
    pub(super) fn duplicate(&mut self, id: &str, children: bool) -> Result<()> {
        self.save_now()?;
        self.save_table_edits()?;
        let parent = self
            .pages
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.parent_id.clone());
        let copy = self.db.duplicate_page(id, children)?;
        self.reload_pages()?;
        let in_table = self
            .database
            .as_ref()
            .is_some_and(|d| parent.as_deref() == Some(d.id.as_str()));
        if in_table {
            if let Some(state) = self.database.as_mut() {
                state.values = self.db.database_values(&state.id)?;
                state.place_after(id, &copy);
            }
            self.record_table(TableOp::AddRow(copy));
            return Ok(());
        }
        self.open_page(copy)
    }

    /// «Duplicar» in the selection bar (H11.2): every chosen row, with its
    /// subpages, in one transaction and one step of Cmd+Z.
    pub(super) fn duplicate_selected(&mut self) -> Result<()> {
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        let rows = state.chosen();
        if rows.is_empty() {
            return Ok(());
        }
        self.save_now()?;
        self.save_table_edits()?;
        let copies = self.db.duplicate_pages(&rows)?;
        self.reload_pages()?;
        if let Some(state) = self.database.as_mut() {
            state.values = self.db.database_values(&state.id)?;
            for (original, copy) in rows.iter().zip(&copies) {
                state.place_after(original, copy);
            }
            state.clear_selection();
        }
        self.record_table(TableOp::AddRows(copies));
        Ok(())
    }

    /// The question of «Duplicar» for a page with subpages or rows.
    pub(super) fn duplicate_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.duplicate_dialog.clone() else {
            return;
        };
        let mut answer = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("silex_duplicate")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(format!("Duplicar «{}»", short_title(&dialog.title)));
            ui.add_space(theme::space::SM);
            let (has, with, without) = match (dialog.database, dialog.children) {
                (true, 1) => (
                    "Tiene 1 fila.".to_owned(),
                    "Con la fila",
                    "Solo la estructura",
                ),
                (true, n) => (
                    format!("Tiene {n} filas."),
                    "Con las filas",
                    "Solo la estructura",
                ),
                (false, 1) => (
                    "Tiene 1 subpágina.".to_owned(),
                    "Con la subpágina",
                    "Solo esta página",
                ),
                (false, n) => (
                    format!("Tiene {n} subpáginas."),
                    "Con las subpáginas",
                    "Solo esta página",
                ),
            };
            ui.label(has);
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                if ui.button(with).clicked() {
                    answer = Some(true);
                }
                if ui.button(without).clicked() {
                    answer = Some(false);
                }
                cancel = ui.button("Cancelar").clicked();
            });
        });
        if let Some(children) = answer {
            self.duplicate_dialog = None;
            let result = self.duplicate(&dialog.id, children);
            self.report(result);
        } else if cancel || response.should_close() {
            self.duplicate_dialog = None;
        }
    }
}
