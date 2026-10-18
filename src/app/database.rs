//! A database page (H6.1, ADR 0006): its views and a table of its rows.
//! Rows are pages: the name cell is the page title, «Abrir» opens the row
//! as a page, and right-click shows the usual page menu.
use std::collections::HashSet;

use anyhow::Result;
use eframe::egui::{self, TextEdit};
use silex_desktop::model::{DatabaseView, Page};

use super::*;
use super::{icons, theme};

/// The open database: its rows and views, and name edits not saved yet.
pub(super) struct DatabaseState {
    pub(super) id: String,
    pub(super) rows: Vec<Page>,
    pub(super) views: Vec<DatabaseView>,
    /// Rows whose name was edited in the table and is not saved yet.
    pub(super) dirty: HashSet<String>,
    /// Focus this row's name on the next frame (a new row, «Renombrar»).
    pub(super) focus: Option<String>,
}

/// Height of a table row.
const ROW_HEIGHT: f32 = 34.0;

pub(super) fn row_name_id(row_id: &str) -> egui::Id {
    egui::Id::unique(("silex-row-name", row_id))
}

/// What the user did in the table this frame (applied after drawing).
#[derive(Default)]
struct TableOutcome {
    open: Option<String>,
    add_row: bool,
    action: Option<RowAction>,
}

impl SilexApp {
    /// Loads the rows and views of the database `id`.
    pub(super) fn load_database(&mut self, id: &str) -> Result<()> {
        self.database = Some(DatabaseState {
            id: id.to_owned(),
            rows: self.db.database_rows(id)?,
            views: self.db.database_views(id)?,
            dirty: HashSet::new(),
            focus: None,
        });
        Ok(())
    }

    /// Saves edited row names, then reloads the rows (they may have been
    /// renamed, moved or trashed from elsewhere).
    pub(super) fn refresh_database(&mut self) -> Result<()> {
        self.save_row_names()?;
        if let Some(state) = self.database.as_mut() {
            state.rows = self.db.database_rows(&state.id)?;
        }
        Ok(())
    }

    /// Saves the row names edited in the table. On error the edits stay
    /// marked, so a later save tries again.
    pub(super) fn save_row_names(&mut self) -> Result<()> {
        let Some(state) = self.database.as_mut() else {
            return Ok(());
        };
        if state.dirty.is_empty() {
            return Ok(());
        }
        for row in state.rows.iter().filter(|r| state.dirty.contains(&r.id)) {
            self.db.rename_page(&row.id, &row.title)?;
        }
        state.dirty.clear();
        // The sidebar, the search and the breadcrumbs use these titles.
        self.pages = self.db.list_pages()?;
        self.rebuild_tree();
        Ok(())
    }

    /// Adds an empty row to `database` and puts the cursor in its name,
    /// opening the database first if needed.
    pub(super) fn add_row(&mut self, database: &str) -> Result<()> {
        self.save_now()?;
        let row = self.db.create_row(database, "")?;
        self.reload_pages()?;
        if self.selected.as_deref() != Some(database) {
            self.open_page(database.to_owned())?;
        }
        if let Some(state) = self.database.as_mut() {
            state.focus = Some(row);
        }
        Ok(())
    }

    /// The database's view tabs and table, below its title.
    pub(super) fn database_table(&mut self, ui: &mut egui::Ui) {
        let Some(mut state) = self.database.take() else {
            return;
        };
        let outcome = self.draw_table(ui, &mut state);
        let database = state.id.clone();
        let editing = state
            .rows
            .iter()
            .any(|r| ui.memory(|m| m.has_focus(row_name_id(&r.id))));
        self.database = Some(state);
        // A name is saved once its cell is left (or scrolled away).
        if !editing {
            let result = self.save_row_names();
            self.report(result);
        }
        let result = (|| -> Result<()> {
            if let Some(row) = outcome.open {
                self.open_page(row)?;
            } else if outcome.add_row {
                self.add_row(&database)?;
            } else if let Some(action) = outcome.action {
                match action {
                    // Rows are not in the sidebar: rename in the table.
                    RowAction::StartRename(id) => {
                        if let Some(state) = self.database.as_mut() {
                            state.focus = Some(id);
                        }
                    }
                    other => self.apply(other)?,
                }
            }
            Ok(())
        })();
        self.report(result);
    }

    fn draw_table(&self, ui: &mut egui::Ui, state: &mut DatabaseState) -> TableOutcome {
        let p = theme::current(ui.ctx());
        let mut outcome = TableOutcome::default();

        // View tabs (one «Tabla» view until H6.3) and the row count.
        ui.horizontal(|ui| {
            for (index, view) in state.views.iter().enumerate() {
                let selected = index == 0;
                let color = if selected { p.text } else { p.text_muted };
                let tab = ui.add(
                    egui::Button::new((
                        icons::icon(icons::TABLE).color(color),
                        egui::RichText::new(&view.name).color(color),
                    ))
                    .frame_when_inactive(false)
                    .selected(selected),
                );
                if selected {
                    let r = tab.rect;
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 3.0,
                        egui::Stroke::new(2.0, p.accent),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = match state.rows.len() {
                    1 => "1 fila".to_owned(),
                    n => format!("{n} filas"),
                };
                ui.label(egui::RichText::new(count).small().color(p.text_muted));
            });
        });
        ui.add_space(theme::space::XS);
        line(ui, p.border);

        // Header: one column for now, the name.
        ui.horizontal(|ui| {
            ui.set_min_height(30.0);
            ui.add_space(theme::space::XS);
            ui.label(icons::icon(icons::TEXT_AA).color(p.text_muted));
            ui.label(egui::RichText::new("Nombre").small().color(p.text_muted));
        });
        line(ui, p.border);

        let width = ui.available_width();
        for row in state.rows.iter_mut() {
            let name_id = row_name_id(&row.id);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
            let focused = ui.memory(|m| m.has_focus(name_id));
            let wanted = state.focus.as_deref() == Some(row.id.as_str());
            // Rows out of sight only take their space (thousands of rows).
            if !ui.is_rect_visible(rect) && !focused && !wanted {
                continue;
            }
            if wanted {
                ui.scroll_to_rect(rect, None);
            }
            let hovered = ui.rect_contains_pointer(rect);
            if hovered || focused {
                ui.painter().rect_filled(rect, 0.0, p.subtle);
            }
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, (1.0, p.border));
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(egui::vec2(theme::space::XS, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    let open_width = 72.0;
                    let name = ui.add(
                        TextEdit::singleline(&mut row.title)
                            .id(name_id)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::symmetric(4, 2))
                            .align(egui::Align2::LEFT_CENTER)
                            .min_size(egui::vec2(0.0, ROW_HEIGHT - 4.0))
                            .hint_text(egui::RichText::new("Sin título").color(p.text_faint))
                            .desired_width(ui.available_width() - open_width),
                    );
                    if wanted {
                        name.request_focus();
                        state.focus = None;
                    }
                    if name.changed() {
                        state.dirty.insert(row.id.clone());
                    }
                    name.context_menu(|ui| {
                        if let Some(chosen) = self.page_menu(ui, &row.id) {
                            outcome.action = Some(chosen);
                            ui.close();
                        }
                    });
                    let open = ui
                        .add(
                            egui::Button::new((
                                icons::icon(icons::OPEN_EXTERNAL).color(p.text_muted),
                                egui::RichText::new("Abrir").small().color(p.text_muted),
                            ))
                            .frame_when_inactive(false),
                        )
                        .on_hover_text("Abrir como página")
                        .accessible_name(format!("Abrir «{}»", display_title(&row.title)));
                    if open.clicked() {
                        outcome.open = Some(row.id.clone());
                    }
                },
            );
        }
        if state.rows.is_empty() {
            ui.add_space(theme::space::SM);
            ui.label(egui::RichText::new("Todavía no hay filas.").color(p.text_muted));
        }

        ui.add_space(theme::space::SM);
        let add = ui
            .add(
                egui::Button::new((
                    icons::icon(icons::PLUS).color(p.text_muted),
                    egui::RichText::new(" Nueva fila").color(p.text_muted),
                ))
                .frame_when_inactive(false),
            )
            .accessible_name("Nueva fila");
        outcome.add_row = add.clicked();
        outcome
    }
}

/// A full-width 1 px line.
fn line(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, (1.0, color));
}
