//! A database page (ADR 0006): its views and a table of its rows.
//! Rows are pages (H6.1): the name cell is the page title, «Abrir» opens the
//! row as a page, and right-click shows the usual page menu. Typed
//! properties (H6.2) are the other columns; each cell is edited by its type
//! (`cells.rs`) and saved at once in its own transaction. The arrow keys
//! move between cells (H6.2b), and a select property's menu manages its
//! options. A board view (H6.5) shares the toolbar and draws cards instead
//! (`board.rs`).
use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use eframe::egui::{self, TextEdit};
use silex_desktop::db::Shift;
use silex_desktop::model::{DatabaseView, Page, ViewKind};
use silex_desktop::property::{
    CellValue, OptionColor, Property, PropertyKind, SelectOption, TableValues,
};
use silex_desktop::view::{ViewConfig, board_property, visible_rows};

use super::*;
use super::{icons, theme};

/// The open database: its rows, views, properties and values, and the
/// edits in progress.
pub(super) struct DatabaseState {
    pub(super) id: String,
    pub(super) rows: Vec<Page>,
    pub(super) views: Vec<DatabaseView>,
    /// The columns after «Nombre», in order.
    pub(super) properties: Vec<Property>,
    /// Deleted properties, offered in the "…" menu to restore.
    pub(super) deleted: Vec<Property>,
    pub(super) values: TableValues,
    /// Rows whose name was edited in the table and is not saved yet.
    pub(super) dirty: HashSet<String>,
    /// Give the keyboard focus to this cell on the next frame: row id and
    /// column (0 is the name). A new row, «Renombrar», the arrow keys.
    pub(super) focus: Option<(String, usize)>,
    /// The cell that had the keyboard focus last frame (row index, column),
    /// where the arrow keys start from.
    pub(super) cursor: Option<(usize, usize)>,
    /// The text, number or date cell being typed in.
    pub(super) editing: Option<CellEdit>,
    /// A property's new name, typed in its menu; saved when the menu closes.
    pub(super) renaming: Option<(String, String)>,
    /// Name typed in the «+» menu for the next property.
    pub(super) new_property: String,
    /// What is typed in the search field of the open select menu.
    pub(super) option_filter: String,
    /// How far the rows are scrolled (the header follows sideways).
    pub(super) scroll: egui::Vec2,
    /// Option names typed in a select property's menu (option id → name),
    /// saved when they lose the focus or the menu closes.
    pub(super) option_names: HashMap<String, String>,
    /// The option whose colors are shown in its property's menu.
    pub(super) option_colors: Option<String>,
    /// The option waiting for «Borrar» to be confirmed.
    pub(super) option_delete: Option<String>,
    /// A board column's new name, typed in its menu (option id, name);
    /// saved when the menu closes (H6.5).
    pub(super) group_rename: Option<(String, String)>,
    /// The view on show, an index into `views` (the first until H6.3b).
    pub(super) view: usize,
    /// The rows in sight, in the view's order: indices into `rows`.
    pub(super) order: Vec<usize>,
    /// The properties shown as columns, in order: indices into `properties`.
    pub(super) columns: Vec<usize>,
    /// Rows added since the view last changed: shown even where its filters
    /// would leave them out, so a new row does not vanish while named.
    pub(super) pinned: HashSet<String>,
    /// The view's settings changed and are not saved yet.
    pub(super) view_dirty: bool,
    /// A filter's value is being typed (the view is saved after).
    pub(super) typing_filter: bool,
    /// The inline list open in a view menu (column, operator…).
    pub(super) view_picker: Option<String>,
    /// A view's new name, typed in its tab's menu; saved when it closes.
    pub(super) view_rename: Option<(String, String)>,
    /// The view waiting for «Borrar» to be confirmed.
    pub(super) view_delete: Option<String>,
    /// What can be undone in this table (H6.4).
    pub(super) history: TableHistory,
}

impl DatabaseState {
    /// The settings of the view on show.
    pub(super) fn config(&self) -> Option<&ViewConfig> {
        self.views.get(self.view).map(|v| &v.config)
    }

    /// Whether the view on show is a board (H6.5).
    pub(super) fn is_board(&self) -> bool {
        self.views
            .get(self.view)
            .is_some_and(|v| v.kind == ViewKind::Board)
    }

    /// Works out which rows and columns are in sight, and in what order.
    /// Editing a cell does not call it: rows do not jump or vanish while
    /// they are being edited, only when the view, the rows or the
    /// properties change (or the database is opened again).
    pub(super) fn apply_view(&mut self) {
        let fallback = ViewConfig::default();
        let config = self.config().unwrap_or(&fallback);
        let mut order = visible_rows(&self.rows, &self.values, &self.properties, config);
        let shown: HashSet<usize> = order.iter().copied().collect();
        order.extend(
            (0..self.rows.len())
                .filter(|i| !shown.contains(i) && self.pinned.contains(&self.rows[*i].id)),
        );
        let columns = (0..self.properties.len())
            .filter(|&i| !config.is_hidden(&self.properties[i].id))
            .collect();
        self.order = order;
        self.columns = columns;
    }

    /// After the rows were read again (added, renamed, trashed): the rows
    /// in sight stay where they were, gone ones leave, and rows that were
    /// not there before (`known`) or were added here (pinned) go at the
    /// end. The view is not worked out again, so nothing jumps or vanishes
    /// just because a row was added.
    fn keep_view(&mut self, before: &[String], known: &HashSet<String>) {
        let index: HashMap<&str, usize> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.as_str(), i))
            .collect();
        let mut order: Vec<usize> = before
            .iter()
            .filter_map(|id| index.get(id.as_str()).copied())
            .collect();
        let shown: HashSet<usize> = order.iter().copied().collect();
        order.extend((0..self.rows.len()).filter(|i| {
            let id = &self.rows[*i].id;
            !shown.contains(i) && (self.pinned.contains(id) || !known.contains(id))
        }));
        self.order = order;
    }

    /// After a change to the view's settings: apply it and save it soon.
    pub(super) fn view_changed(&mut self) {
        self.pinned.clear();
        self.apply_view();
        self.view_dirty = true;
    }
}

/// Height of a table row.
const ROW_HEIGHT: f32 = 34.0;
/// Height of the header row.
const HEADER_HEIGHT: f32 = 30.0;
/// The name column takes the room left by the others, but not less.
const NAME_MIN_WIDTH: f32 = 240.0;
/// The «+» column that adds a property.
const ADD_COLUMN_WIDTH: f32 = 40.0;
fn column_width(kind: PropertyKind) -> f32 {
    match kind {
        PropertyKind::Text => 180.0,
        PropertyKind::Number => 110.0,
        PropertyKind::Checkbox => 96.0,
        PropertyKind::Date => 124.0,
        PropertyKind::Select => 150.0,
    }
}

pub(super) fn row_name_id(row_id: &str) -> egui::Id {
    egui::Id::unique(("silex-row-name", row_id))
}

/// The header button of a property (opens its menu).
pub(super) fn property_header_id(property: &str) -> egui::Id {
    egui::Id::unique(("silex-property", property))
}

fn property_menu_id(property: &str) -> egui::Id {
    egui::Id::unique(("silex-property-menu", property))
}

/// What the user did in the table this frame, applied after drawing.
pub(super) enum TableAction {
    Open(String),
    AddRow,
    Row(RowAction),
    AddProperty(PropertyKind, String),
    RenameProperty(String, String),
    MoveProperty(String, Shift),
    DeleteProperty(String),
    RenameOption(String, String),
    SetOptionColor(String, OptionColor),
    DeleteOption(String),
    /// Move an option one place (a board's column, H6.5).
    MoveOption(String, Shift),
    /// Hide a property's column in the view on show.
    HideProperty(String),
    /// A new card in a board's column: the option it gets (none for the
    /// column of rows without one).
    AddCard(Option<String>),
    /// A new option of a select property (a board's new column): its
    /// property and name.
    AddOption(String, String),
    /// Views as tabs (H6.3b): by index in `views`, or by id.
    SwitchView(usize),
    AddView(ViewKind),
    DuplicateView(usize),
    RenameView(String, String),
    MoveView(String, Shift),
    DeleteView(String),
}

impl SilexApp {
    /// Loads the database `id`: rows, views, properties and values.
    pub(super) fn load_database(&mut self, id: &str) -> Result<()> {
        self.database = Some(DatabaseState {
            id: id.to_owned(),
            rows: self.db.database_rows(id)?,
            views: self.db.database_views(id)?,
            properties: self.db.database_properties(id)?,
            deleted: self.db.deleted_properties(id)?,
            values: self.db.database_values(id)?,
            dirty: HashSet::new(),
            focus: None,
            cursor: None,
            editing: None,
            renaming: None,
            new_property: String::new(),
            option_filter: String::new(),
            scroll: egui::Vec2::ZERO,
            option_names: HashMap::new(),
            option_colors: None,
            option_delete: None,
            group_rename: None,
            view: 0,
            order: Vec::new(),
            columns: Vec::new(),
            pinned: HashSet::new(),
            view_dirty: false,
            typing_filter: false,
            view_picker: None,
            view_rename: None,
            view_delete: None,
            history: TableHistory::default(),
        });
        // The view last open in this session, else the first.
        let last = self.last_views.get(id).cloned();
        if let Some(state) = self.database.as_mut() {
            state.view = last
                .and_then(|last| state.views.iter().position(|v| v.id == last))
                .unwrap_or(0);
            state.apply_view();
        }
        Ok(())
    }

    /// Saves the table's pending edits, then reloads the rows (they may
    /// have been added, renamed, moved or trashed from elsewhere). Values
    /// are not reloaded (~80 ms with 10 000 rows): they change in the table
    /// itself or on a row's page, and opening the database reads them again
    /// (a row restored from the trash opens as a page too).
    pub(super) fn refresh_database(&mut self) -> Result<()> {
        self.save_table_edits()?;
        if let Some(state) = self.database.as_mut() {
            let before: Vec<String> = state
                .order
                .iter()
                .filter_map(|&i| state.rows.get(i).map(|r| r.id.clone()))
                .collect();
            let known: HashSet<String> = state.rows.iter().map(|r| r.id.clone()).collect();
            state.rows = self.db.database_rows(&state.id)?;
            state.keep_view(&before, &known);
        }
        Ok(())
    }

    /// Reloads the columns after a change to them (and the values, which
    /// follow their properties out and back).
    pub(super) fn reload_properties(&mut self) -> Result<()> {
        if let Some(state) = self.database.as_mut() {
            state.properties = self.db.database_properties(&state.id)?;
            state.deleted = self.db.deleted_properties(&state.id)?;
            state.values = self.db.database_values(&state.id)?;
            state.apply_view();
        }
        Ok(())
    }

    /// Stores the view's settings if they changed.
    pub(super) fn save_view(&mut self) -> Result<()> {
        let Some(state) = self.database.as_mut() else {
            return Ok(());
        };
        if !state.view_dirty {
            return Ok(());
        }
        if let Some(view) = state.views.get(state.view) {
            self.db.set_view_config(&view.id, &view.config)?;
        }
        state.view_dirty = false;
        Ok(())
    }

    /// Saves everything the table and the row's page have pending: edited
    /// row names and the text typed in an open cell (which stays open).
    pub(super) fn save_table_edits(&mut self) -> Result<()> {
        let view = self.save_view();
        let names = self.save_row_names().and(view);
        let open = [
            self.database.as_ref().and_then(|d| d.editing.clone()),
            self.row_page.as_ref().and_then(|p| p.editing.clone()),
        ];
        let mut cells = Ok(());
        for edit in open.into_iter().flatten() {
            cells = cells.and(self.commit_cell_edit(&edit));
        }
        names.and(cells)
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
        let mut renamed = Vec::new();
        for row in state.rows.iter().filter(|r| state.dirty.contains(&r.id)) {
            self.db.rename_page(&row.id, &row.title)?;
            // The page list still has the name it had (undo, H6.4).
            let before = self
                .pages
                .iter()
                .find(|p| p.id == row.id)
                .map(|p| p.title.clone())
                .unwrap_or_default();
            if before != row.title {
                renamed.push(TableOp::Name {
                    row: row.id.clone(),
                    before,
                    after: row.title.clone(),
                });
            }
        }
        state.dirty.clear();
        for op in renamed {
            state.history.record(op);
        }
        // The sidebar, the search and the breadcrumbs use these titles.
        self.pages = self.db.list_pages()?;
        self.rebuild_tree();
        Ok(())
    }

    /// Adds a property as the last column of the open database. An empty
    /// name becomes the name of its type («Número»).
    pub(super) fn add_property(&mut self, kind: PropertyKind, name: &str) -> Result<String> {
        let database = self
            .database
            .as_ref()
            .map(|d| d.id.clone())
            .ok_or_else(|| anyhow!("No hay ninguna base de datos abierta"))?;
        let name = match name.trim() {
            "" => kind.label(),
            name => name,
        };
        let id = self.db.add_property(&database, name, kind)?;
        self.reload_properties()?;
        self.record_table(TableOp::AddProperty(id.clone()));
        Ok(id)
    }

    /// Deletes a property (logically) and says where to get it back.
    pub(super) fn delete_property(&mut self, id: &str) -> Result<()> {
        let name = self
            .database
            .as_ref()
            .and_then(|d| d.properties.iter().find(|p| p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.db.delete_property(id)?;
        self.reload_properties()?;
        self.record_table(TableOp::DeleteProperty(id.to_owned()));
        self.notice = Some(format!(
            "Se borró la propiedad «{name}». Cmd+Z la devuelve, y también «…» → «Propiedades borradas»."
        ));
        Ok(())
    }

    pub(super) fn restore_property(&mut self, id: &str) -> Result<()> {
        self.db.restore_property(id)?;
        self.reload_properties()
    }

    /// Adds an empty row to `database` and puts the cursor in its name,
    /// opening the database first if needed.
    pub(super) fn add_row(&mut self, database: &str) -> Result<()> {
        self.insert_row(database).map(|_| ())
    }

    /// A new card in a board's column (H6.5): a row whose grouping
    /// property is `option` (none: the column of rows without one), its
    /// name ready to type. One undo removes it.
    pub(super) fn add_card(&mut self, database: &str, option: Option<String>) -> Result<()> {
        let property = self
            .database
            .as_ref()
            .and_then(|d| board_property(&d.properties, d.config()?))
            .map(|p| p.id.clone());
        let row = self.insert_row(database)?;
        if let (Some(property), Some(option)) = (property, option) {
            self.store_cell(&row, &property, Some(CellValue::Select(option)))?;
        }
        Ok(())
    }

    /// [`Self::add_row`], returning the new row's id.
    fn insert_row(&mut self, database: &str) -> Result<String> {
        self.save_now()?;
        let row = self.db.create_row(database, "")?;
        self.reload_pages()?;
        if self.selected.as_deref() != Some(database) {
            self.open_page(database.to_owned())?;
        }
        if let Some(state) = self.database.as_mut() {
            // In sight even if the view's filters would hide an empty row.
            if state.pinned.insert(row.clone())
                && let Some(index) = state.rows.iter().position(|r| r.id == row)
                && !state.order.contains(&index)
            {
                state.order.push(index);
            }
            state.focus = Some((row.clone(), 0));
        }
        self.record_table(TableOp::AddRow(row.clone()));
        Ok(row)
    }

    /// The database's view tabs and table, below its title.
    pub(super) fn database_table(&mut self, ui: &mut egui::Ui) {
        let Some(mut state) = self.database.take() else {
            return;
        };
        if !state.is_board() {
            table_keys(ui.ctx(), &mut state);
        }
        let (actions, cells) = self.draw_table(ui, &mut state);
        let database = state.id.clone();
        let editing_name = state
            .rows
            .iter()
            .any(|r| ui.memory(|m| m.has_focus(row_name_id(&r.id))));
        let typing_filter = state.typing_filter;
        self.database = Some(state);
        // The view is saved once its filter values are not being typed.
        if !typing_filter {
            let result = self.save_view();
            self.report(result);
        }
        // A name is saved once its cell is left (or scrolled away).
        if !editing_name {
            let result = self.save_row_names();
            self.report(result);
        }
        // Cells first: text typed in one is saved before anything else
        // changes the table (deleting its property, opening another page…).
        self.apply_cell_actions(cells);
        for action in actions {
            let result = self.apply_table_action(&database, action);
            self.report(result);
        }
    }

    pub(super) fn apply_table_action(&mut self, database: &str, action: TableAction) -> Result<()> {
        match action {
            TableAction::Open(row) => self.open_page(row)?,
            TableAction::AddRow => self.add_row(database)?,
            // Rows are not in the sidebar: rename in the table.
            TableAction::Row(RowAction::StartRename(id)) => {
                if let Some(state) = self.database.as_mut() {
                    state.focus = Some((id, 0));
                }
            }
            TableAction::Row(RowAction::Trash(id)) => {
                self.apply(RowAction::Trash(id.clone()))?;
                self.record_table(TableOp::TrashRow(id));
            }
            TableAction::Row(other) => self.apply(other)?,
            TableAction::AddProperty(kind, name) => {
                self.add_property(kind, &name)?;
            }
            TableAction::RenameProperty(id, name) => {
                let unchanged = self
                    .database
                    .as_ref()
                    .and_then(|d| d.properties.iter().find(|p| p.id == id))
                    .is_none_or(|p| p.name == name.trim());
                // An empty name keeps the old one.
                if !unchanged && !name.trim().is_empty() {
                    self.db.rename_property(&id, name.trim())?;
                    self.reload_properties()?;
                }
            }
            TableAction::MoveProperty(id, shift) => {
                self.db.move_property(&id, shift)?;
                self.reload_properties()?;
            }
            TableAction::DeleteProperty(id) => self.delete_property(&id)?,
            TableAction::RenameOption(id, name) => {
                let unchanged = self
                    .database
                    .iter()
                    .flat_map(|d| d.properties.iter())
                    .flat_map(|p| p.options.iter())
                    .find(|o| o.id == id)
                    .is_none_or(|o| o.name == name.trim());
                if !unchanged && !name.trim().is_empty() {
                    self.db.rename_select_option(&id, name.trim())?;
                    self.reload_properties()?;
                }
            }
            TableAction::SetOptionColor(id, color) => {
                self.db.set_option_color(&id, color)?;
                self.reload_properties()?;
            }
            TableAction::MoveOption(id, shift) => {
                self.db.move_select_option(&id, shift)?;
                self.reload_properties()?;
            }
            TableAction::DeleteOption(id) => {
                self.db.delete_select_option(&id)?;
                self.reload_properties()?;
            }
            TableAction::SwitchView(index) => self.switch_view(index)?,
            TableAction::AddView(kind) => self.add_view(kind)?,
            TableAction::DuplicateView(index) => self.duplicate_view(index)?,
            TableAction::RenameView(id, name) => self.rename_view(&id, &name)?,
            TableAction::MoveView(id, shift) => self.move_view(&id, shift)?,
            TableAction::DeleteView(id) => self.delete_view(&id)?,
            TableAction::AddCard(option) => self.add_card(database, option)?,
            TableAction::AddOption(property, name) => {
                let existing = self
                    .database
                    .iter()
                    .flat_map(|d| d.properties.iter())
                    .find(|p| p.id == property)
                    .map_or(0, |p| p.options.len());
                if !name.trim().is_empty() {
                    self.db.add_select_option(
                        &property,
                        name.trim(),
                        OptionColor::for_new_option(existing),
                    )?;
                    self.reload_properties()?;
                }
            }
            TableAction::HideProperty(id) => {
                if let Some(state) = self.database.as_mut()
                    && let Some(view) = state.views.get_mut(state.view)
                {
                    view.config.set_hidden(&id, true);
                    state.view_changed();
                }
            }
        }
        Ok(())
    }

    /// The "…" menu of a database: its deleted properties, to restore.
    /// Returns the one chosen.
    pub(super) fn deleted_properties_menu(&self, ui: &mut egui::Ui) -> Option<String> {
        let deleted = &self.database.as_ref()?.deleted;
        if deleted.is_empty() {
            return None;
        }
        ui.separator();
        ui.menu_button((icons::icon(icons::UNDO), " Propiedades borradas"), |ui| {
            let mut chosen = None;
            for property in deleted {
                if ui
                    .button((
                        icons::icon(property_icon(property.kind)),
                        format!(" Restaurar «{}»", property.name),
                    ))
                    .clicked()
                {
                    chosen = Some(property.id.clone());
                    ui.close();
                }
            }
            chosen
        })
        .inner
        .flatten()
    }

    fn draw_table(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
    ) -> (Vec<TableAction>, Vec<CellAction>) {
        let p = theme::current(ui.ctx());
        let mut actions = Vec::new();
        let mut cells = Vec::new();

        // View tabs (H6.3b) and, on the right, the row count and the view's
        // menus. Their content lines up with the cells' text, at CELL_PAD
        // from each edge.
        let mut selected_tab = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.x = CELL_PAD;
            selected_tab = view_tabs(ui, state, &mut actions);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Also at the end of the header, which may be out of sight
                // in a wide table. A board has one more menu: just «+».
                let board = state.is_board();
                let label = if board { "" } else { " Propiedad" };
                let add = ui
                    .add(
                        egui::Button::new((
                            icons::icon(icons::PLUS).color(p.text_muted),
                            egui::RichText::new(label).small().color(p.text_muted),
                        ))
                        .frame_when_inactive(false),
                    )
                    .on_hover_text("Añadir una propiedad")
                    .accessible_name("Añadir una propiedad");
                add_property_menu(
                    ui,
                    &add,
                    egui::Id::unique("silex-toolbar-add-property-menu"),
                    &mut state.new_property,
                    &mut actions,
                );
                if view_menus(ui, state) {
                    state.view_changed();
                }
                // Each column of a board counts its own cards.
                if board {
                    return;
                }
                let total = state.rows.len();
                let count = match (state.order.len(), total) {
                    (_, 1) if state.order.len() == 1 => "1 fila".to_owned(),
                    (shown, total) if shown == total => format!("{total} filas"),
                    (shown, 1) => format!("{shown} de 1 fila"),
                    (shown, total) => format!("{shown} de {total} filas"),
                };
                ui.label(egui::RichText::new(count).small().color(p.text_muted));
            });
        });
        if state.is_board() {
            self.draw_board(ui, state, selected_tab, &mut actions, &mut cells);
            return (actions, cells);
        }

        let columns: f32 = state
            .columns
            .iter()
            .map(|&i| column_width(state.properties[i].kind))
            .sum();
        let visible = ui.available_width();
        let name_width = (visible - columns - ADD_COLUMN_WIDTH).max(NAME_MIN_WIDTH);
        let width = name_width + columns + ADD_COLUMN_WIDTH;

        // Lines, header and rows touch: the lines are the only separation.
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        // The header stays on top and follows the rows sideways (it uses
        // their scroll from the last frame, and asks for one more frame
        // when it changes).
        let top = line(ui, visible, p.border);
        // The selected tab is underlined on the table's top line.
        if let Some(tab) = selected_tab {
            ui.painter()
                .hline(tab.x_range(), top, egui::Stroke::new(2.0, p.accent));
        }
        let (header, _) =
            ui.allocate_exact_size(egui::vec2(visible, HEADER_HEIGHT), egui::Sense::hover());
        {
            let ui = &mut inside(ui, header, egui::Layout::top_down(egui::Align::Min));
            ui.set_clip_rect(header.intersect(ui.clip_rect()));
            let left = header.left() - state.scroll.x;
            let rect = egui::Rect::from_min_size(
                egui::pos2(left, header.top()),
                egui::vec2(width, HEADER_HEIGHT),
            );
            self.table_header(ui, state, rect, name_width, &mut actions);
        }
        line(ui, visible, p.border);

        // The rows scroll in their own area, both ways, drawing only the
        // ones in sight (thousands of rows). Room is left for «Nueva fila».
        let max_height = (ui.available_height() - 48.0).max(3.0 * ROW_HEIGHT);
        let wanted = state.focus.as_ref().and_then(|(id, _)| {
            state
                .order
                .iter()
                .position(|&i| state.rows.get(i).is_some_and(|r| &r.id == id))
        });
        let mut area = egui::ScrollArea::both()
            .id_salt(("silex-table", &state.id))
            .auto_shrink([false, true])
            .max_height(max_height);
        if let Some(index) = wanted {
            let top = index as f32 * ROW_HEIGHT;
            let current = state.scroll.y;
            if top < current {
                area = area.vertical_scroll_offset(top);
            } else if top + ROW_HEIGHT > current + max_height {
                area = area.vertical_scroll_offset(top + ROW_HEIGHT - max_height);
            }
        }
        let output = ui
            .scope(|ui| {
                visible_scroll_bars(ui);
                area.show_rows(ui, ROW_HEIGHT, state.order.len(), |ui, range| {
                    ui.set_min_width(width);
                    let columns = Columns { name_width, width };
                    self.table_rows(ui, state, range.clone(), columns, &mut actions, &mut cells);
                    range
                })
            })
            .inner;
        ui.spacing_mut().item_spacing.y = spacing;
        if output.state.unclamped_offset() != state.scroll {
            state.scroll = output.state.unclamped_offset();
            ui.ctx().request_repaint();
        }
        // A cell scrolled out of sight stops being typed in: save it.
        let drawn = output.inner;
        if let Some(edit) = &state.editing
            && !state.order[drawn]
                .iter()
                .any(|&i| state.rows.get(i).is_some_and(|r| r.id == edit.row))
            && let Some(edit) = state.editing.take()
        {
            cells.push(CellAction::Commit(edit));
        }

        if state.order.is_empty() {
            let empty = if state.rows.is_empty() {
                "Todavía no hay filas."
            } else {
                "Ninguna fila coincide con los filtros."
            };
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(visible, ROW_HEIGHT), egui::Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, (1.0, p.border));
            paint_cell_text(ui, rect, empty, egui::Align2::LEFT_CENTER, p.text_muted);
        }
        ui.add_space(theme::space::XS);
        if new_row_button(ui, visible).clicked() {
            actions.push(TableAction::AddRow);
        }
        (actions, cells)
    }

    /// «Nombre», one button per property (opens its menu) and «+», in
    /// `rect` (the whole table width; the `Ui` clips it to what is in sight).
    fn table_header(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        rect: egui::Rect,
        name_width: f32,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        let name = egui::Rect::from_min_size(rect.min, egui::vec2(name_width, rect.height()));
        header_label(ui, name, icons::TEXT_AA, "Nombre");

        let DatabaseState {
            properties,
            columns,
            values,
            renaming,
            new_property,
            option_names,
            option_colors,
            option_delete,
            ..
        } = state;
        let count = properties.len();
        let mut x = name.right();
        for &index in columns.iter() {
            let Some(property) = properties.get(index) else {
                continue;
            };
            let cell = egui::Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(column_width(property.kind), rect.height()),
            );
            x = cell.right();
            ui.painter()
                .vline(cell.left(), cell.y_range(), (1.0, p.border));
            let button = ui
                .interact(cell, property_header_id(&property.id), egui::Sense::click())
                .on_hover_text("Opciones de la propiedad");
            let label = format!("{}, {}", property.name, property.kind.label());
            button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            if button.hovered() || button.has_focus() {
                ui.painter().rect_filled(cell, 0.0, p.subtle);
            }
            header_label(ui, cell, property_icon(property.kind), &property.name);
            focus_ring(ui, cell, &button);

            let menu = property_menu_id(&property.id);
            let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
            if opening {
                *renaming = Some((property.id.clone(), property.name.clone()));
                *option_names = property
                    .options
                    .iter()
                    .map(|o| (o.id.clone(), o.name.clone()))
                    .collect();
                *option_colors = None;
                *option_delete = None;
            }
            let width = if property.kind == PropertyKind::Select {
                280.0
            } else {
                220.0
            };
            egui::Popup::menu(&button)
                .id(menu)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(width)
                .show(|ui| {
                    let mut menu = PropertyMenu {
                        renaming: &mut *renaming,
                        option_names: &mut *option_names,
                        option_colors: &mut *option_colors,
                        option_delete: &mut *option_delete,
                        values,
                    };
                    menu.show(ui, property, (index, count), opening, actions);
                });
        }
        // New names are saved when their menu closes; Escape drops them.
        if let Some((id, _)) = renaming
            && !egui::Popup::is_id_open(ui.ctx(), property_menu_id(id))
            && let Some((id, name)) = renaming.take()
        {
            let names = std::mem::take(option_names);
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                actions.push(TableAction::RenameProperty(id, name));
                for (option, name) in names {
                    actions.push(TableAction::RenameOption(option, name));
                }
            }
        }

        let add = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            egui::vec2(ADD_COLUMN_WIDTH, rect.height()),
        );
        ui.painter()
            .vline(add.left(), add.y_range(), (1.0, p.border));
        let button = ui
            .interact(
                add,
                egui::Id::unique("silex-add-property"),
                egui::Sense::click(),
            )
            .on_hover_text("Añadir una propiedad");
        button.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "Añadir una propiedad")
        });
        if button.hovered() || button.has_focus() {
            ui.painter().rect_filled(add, 0.0, p.subtle);
        }
        ui.painter().text(
            add.center(),
            egui::Align2::CENTER_CENTER,
            icons::PLUS,
            icons::font(theme::icon::MD),
            p.text_muted,
        );
        focus_ring(ui, add, &button);
        add_property_menu(
            ui,
            &button,
            egui::Id::unique("silex-add-property-menu"),
            new_property,
            actions,
        );
    }

    /// The rows in `range` (the ones in sight): name and «Abrir», then a
    /// cell per property. Notes which cell has the keyboard focus.
    fn table_rows(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        range: std::ops::Range<usize>,
        columns: Columns,
        actions: &mut Vec<TableAction>,
        cells: &mut Vec<CellAction>,
    ) {
        let p = theme::current(ui.ctx());
        let DatabaseState {
            rows,
            order,
            properties,
            columns: shown,
            values,
            dirty,
            focus,
            cursor,
            editing,
            option_filter,
            ..
        } = state;
        *cursor = None;
        let first = range.start;
        for (offset, &row_index) in order[range].iter().enumerate() {
            // `index` is the row's place in the view; `row_index` in `rows`.
            let index = first + offset;
            let Some(row) = rows.get_mut(row_index) else {
                continue;
            };
            let name_id = row_name_id(&row.id);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(columns.width, ROW_HEIGHT), egui::Sense::hover());
            let focused = ui.memory(|m| m.has_focus(name_id));
            if focused {
                *cursor = Some((index, 0));
            }
            let wanted = |column: usize| {
                focus
                    .as_ref()
                    .is_some_and(|(id, c)| *id == row.id && *c == column)
            };
            if ui.rect_contains_pointer(rect) || focused {
                ui.painter().rect_filled(rect, 0.0, p.subtle);
            }
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, (1.0, p.border));

            let name_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(columns.name_width, rect.height()));
            {
                // A frameless text field has no margin: the rect sets where
                // the text starts.
                let inner = egui::Rect::from_x_y_ranges(
                    name_rect.left() + CELL_PAD..=name_rect.right() - theme::space::XS,
                    name_rect.y_range(),
                );
                let ui = &mut inside(ui, inner, egui::Layout::left_to_right(egui::Align::Center));
                let open_width = 72.0;
                let name = ui.add(
                    TextEdit::singleline(&mut row.title)
                        .id(name_id)
                        .frame(egui::Frame::NONE)
                        .align(egui::Align2::LEFT_CENTER)
                        .min_size(egui::vec2(0.0, ROW_HEIGHT - 4.0))
                        .hint_text(egui::RichText::new("Sin título").color(p.text_faint))
                        .desired_width(ui.available_width() - open_width),
                );
                if wanted(0) {
                    name.request_focus();
                }
                if name.gained_focus() {
                    name.scroll_to_me(None);
                }
                if name.changed() {
                    dirty.insert(row.id.clone());
                }
                name.context_menu(|ui| {
                    if let Some(chosen) = self.page_menu(ui, &row.id) {
                        actions.push(TableAction::Row(chosen));
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
                    actions.push(TableAction::Open(row.id.clone()));
                }
            }

            let mut x = name_rect.right();
            for (column, property) in (1..).zip(shown.iter().filter_map(|&i| properties.get(i))) {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(column_width(property.kind), rect.height()),
                );
                x = rect.right();
                ui.painter()
                    .vline(rect.left(), rect.y_range(), (1.0, p.border));
                let cell = Cell {
                    rect,
                    row,
                    property,
                    value: values
                        .get(&row.id)
                        .and_then(|cells| cells.get(&property.id)),
                    wanted: wanted(column),
                    place: CellPlace::Table,
                };
                property_cell(ui, &cell, editing, option_filter, cells);
                if ui.memory(|m| m.has_focus(cell.id())) {
                    *cursor = Some((index, column));
                }
            }
            ui.painter().vline(x, rect.y_range(), (1.0, p.border));
            if focus.as_ref().is_some_and(|(id, _)| *id == row.id) {
                *focus = None;
            }
        }
    }
}

/// Widths of the table's columns this frame.
#[derive(Clone, Copy)]
struct Columns {
    name_width: f32,
    width: f32,
}

/// The arrow keys move the keyboard focus between cells, like a
/// spreadsheet: ↑/↓ change row (scrolling to rows out of sight), ←/→
/// change column. In a name, ← and → move in its text, and → at its end
/// goes on to the first property. While typing in a cell or with a menu
/// open, the keys belong to them. Runs before the table is drawn, so the
/// widgets never see the keys it takes.
fn table_keys(ctx: &egui::Context, state: &mut DatabaseState) {
    use egui::{Key, Modifiers};
    let Some((index, column)) = state.cursor else {
        return;
    };
    let row_at = |index: usize| state.order.get(index).and_then(|&i| state.rows.get(i));
    let Some(row) = row_at(index) else {
        return;
    };
    let property_at = |column: usize| {
        state
            .columns
            .get(column.checked_sub(1)?)
            .and_then(|&i| state.properties.get(i))
    };
    let id = match column {
        0 => row_name_id(&row.id),
        n => match property_at(n) {
            Some(property) => cell_id(&row.id, &property.id),
            None => return,
        },
    };
    if !ctx.memory(|m| m.has_focus(id)) {
        return;
    }
    let last_row = state.order.len().saturating_sub(1);
    let last_column = state.columns.len();
    // The caret at the end of the name, with nothing selected.
    let name_end = column == 0
        && egui::TextEdit::load_state(ctx, id)
            .and_then(|s| s.cursor.char_range())
            .is_some_and(|r| r.is_empty() && r.primary.index.0 >= row.title.chars().count());
    let take = |key| take_exact_key(ctx, key, Modifiers::NONE);
    let target = if take(Key::ArrowUp) {
        (index.saturating_sub(1), column)
    } else if take(Key::ArrowDown) {
        ((index + 1).min(last_row), column)
    } else if column > 0 && take(Key::ArrowLeft) {
        (index, column - 1)
    } else if (column > 0 || name_end) && take(Key::ArrowRight) {
        (index, (column + 1).min(last_column))
    } else {
        return;
    };
    // egui would also move the focus to the nearest widget on its own.
    ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    if target != (index, column)
        && let Some(row) = row_at(target.0)
    {
        state.focus = Some((row.id.clone(), target.1));
    }
}

/// The menu of a «+» button that adds a property: an optional name, then
/// the type, which creates it.
fn add_property_menu(
    ui: &egui::Ui,
    button: &egui::Response,
    menu: egui::Id,
    name: &mut String,
    actions: &mut Vec<TableAction>,
) {
    let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
    if opening {
        name.clear();
    }
    egui::Popup::menu(button)
        .id(menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(220.0)
        .show(|ui| {
            menu_spacing(ui);
            let field = menu_field(
                ui,
                name,
                "Nombre de la propiedad",
                "Nombre de la nueva propiedad",
            );
            if opening {
                field.request_focus();
            }
            ui.add_space(theme::space::XS);
            menu_caption(ui, "Tipo");
            for kind in PropertyKind::ALL {
                if ui
                    .button((
                        icons::icon(property_icon(kind)),
                        format!(" {}", kind.label()),
                    ))
                    .clicked()
                {
                    actions.push(TableAction::AddProperty(kind, std::mem::take(name)));
                    ui.close();
                }
            }
        });
}

/// What a property's menu edits: its name, and for a select property the
/// names, colors and deletion of its options.
struct PropertyMenu<'a> {
    renaming: &'a mut Option<(String, String)>,
    option_names: &'a mut HashMap<String, String>,
    option_colors: &'a mut Option<String>,
    option_delete: &'a mut Option<String>,
    /// To say how many cells deleting an option empties.
    values: &'a TableValues,
}

impl PropertyMenu<'_> {
    /// Name (editable), type, its options if it is a select, moving and
    /// deleting.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        property: &Property,
        (index, count): (usize, usize),
        opening: bool,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        menu_spacing(ui);
        if let Some((_, name)) = self.renaming.as_mut().filter(|(id, _)| *id == property.id) {
            let field = menu_field(ui, name, "Nombre de la propiedad", "Nombre de la propiedad");
            if opening {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ui.close();
            }
        }
        ui.add_space(theme::space::XS);
        menu_caption(ui, &format!("Tipo: {}", property.kind.label()));
        if property.kind == PropertyKind::Select {
            ui.separator();
            self.options(ui, property, actions);
        }
        ui.separator();
        if ui
            .button((icons::icon(icons::EYE_SLASH), " Ocultar en esta vista"))
            .clicked()
        {
            actions.push(TableAction::HideProperty(property.id.clone()));
            ui.close();
        }
        let moves = [
            (
                Shift::Up,
                icons::ARROW_LEFT,
                " Mover a la izquierda",
                index > 0,
            ),
            (
                Shift::Down,
                icons::ARROW_RIGHT,
                " Mover a la derecha",
                index + 1 < count,
            ),
        ];
        for (shift, icon, label, enabled) in moves {
            if ui
                .add_enabled(enabled, egui::Button::new((icons::icon(icon), label)))
                .clicked()
            {
                actions.push(TableAction::MoveProperty(property.id.clone(), shift));
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button((
                icons::icon(icons::TRASH).color(p.danger),
                egui::RichText::new(" Borrar la propiedad").color(p.danger),
            ))
            .clicked()
        {
            actions.push(TableAction::DeleteProperty(property.id.clone()));
            ui.close();
        }
    }

    /// One line per option: its color (a click shows the eight), its name
    /// and a button to delete it, which asks first.
    fn options(&mut self, ui: &mut egui::Ui, property: &Property, actions: &mut Vec<TableAction>) {
        let p = theme::current(ui.ctx());
        menu_caption(ui, "Opciones");
        if property.options.is_empty() {
            menu_caption(ui, "Se crean escribiendo en una celda.");
        }
        for (number, option) in (1..).zip(property.options.iter()) {
            if self.option_delete.as_deref() == Some(option.id.as_str()) {
                self.confirm_delete(ui, option, actions);
                continue;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::XS;
                // Lined up with the captions, like the colors below.
                ui.add_space(MENU_PAD);
                let (bg, fg) = theme::option_colors(ui.ctx(), option.color);
                let name = format!("Color de «{}»: {}", option.name, option.color.label());
                let swatch =
                    color_swatch(ui, bg, fg, false, &name).on_hover_text("Cambiar el color");
                if swatch.clicked() {
                    *self.option_colors = match self.option_colors.take() {
                        Some(open) if open == option.id => None,
                        _ => Some(option.id.clone()),
                    };
                }
                // Delete on the right first; the name takes exactly the room
                // left. (Sizing it from the menu's width made the menu grow
                // every frame.)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let delete = ui
                        .add(
                            egui::Button::new(icons::icon(icons::TRASH).color(p.text_muted))
                                .frame_when_inactive(false),
                        )
                        .on_hover_text("Borrar la opción")
                        .accessible_name(format!("Borrar la opción «{}»", option.name));
                    if delete.clicked() {
                        *self.option_delete = Some(option.id.clone());
                    }
                    if let Some(name) = self.option_names.get_mut(&option.id) {
                        let field = framed(
                            ui,
                            TextEdit::singleline(name)
                                .margin(egui::Margin::symmetric(MENU_PAD as i8, 3))
                                .desired_width(f32::INFINITY),
                        )
                        .accessible_name(format!("Nombre de la opción {number}"));
                        if field.lost_focus() {
                            actions
                                .push(TableAction::RenameOption(option.id.clone(), name.clone()));
                        }
                    }
                });
            });
            if self.option_colors.as_deref() == Some(option.id.as_str()) {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::space::SM, theme::space::SM);
                    ui.add_space(MENU_PAD);
                    for color in OptionColor::ALL {
                        let (bg, fg) = theme::option_colors(ui.ctx(), color);
                        let current = color == option.color;
                        let swatch = color_swatch(ui, bg, fg, current, color.label())
                            .on_hover_text(color.label());
                        if swatch.clicked() {
                            actions.push(TableAction::SetOptionColor(option.id.clone(), color));
                            *self.option_colors = None;
                        }
                    }
                });
            }
        }
    }

    /// In place of an option's line: what deleting it does, and the choice.
    fn confirm_delete(
        &mut self,
        ui: &mut egui::Ui,
        option: &SelectOption,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        let uses = self
            .values
            .values()
            .flat_map(|cells| cells.values())
            .filter(|v| matches!(v, CellValue::Select(id) if *id == option.id))
            .count();
        let effect = match uses {
            0 => "Ninguna fila la usa.".to_owned(),
            1 => "Se vaciará en 1 fila.".to_owned(),
            n => format!("Se vaciará en {n} filas."),
        };
        menu_caption(ui, &format!("¿Borrar «{}»? {effect}", option.name));
        // The buttons' padding lines their text up with the question.
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("Borrar").color(p.danger))
                .on_hover_text("No se puede deshacer")
                .clicked()
            {
                actions.push(TableAction::DeleteOption(option.id.clone()));
                *self.option_delete = None;
            }
            if ui.button("Cancelar").clicked() {
                *self.option_delete = None;
            }
        });
    }
}

/// A small square in an option's colors, a button named `name` (ticked
/// when it is the current color). Drawn by hand: an `egui::Button` is at
/// least as tall as a row.
pub(super) fn color_swatch(
    ui: &mut egui::Ui,
    bg: egui::Color32,
    fg: egui::Color32,
    current: bool,
    name: &str,
) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (swatch, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, name));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    ui.painter().rect_filled(swatch, radius, bg);
    ui.painter().rect_stroke(
        swatch,
        radius,
        egui::Stroke::new(1.0, fg),
        egui::StrokeKind::Inside,
    );
    if current {
        ui.painter().text(
            swatch.center(),
            egui::Align2::CENTER_CENTER,
            icons::CHECK,
            icons::font(12.0),
            fg,
        );
    }
    if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            swatch.expand(2.0),
            egui::CornerRadius::same(theme::radius::MD),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Outside,
        );
    }
    response
}

/// «Nombre» or a property's icon and name in the header, muted.
fn header_label(ui: &egui::Ui, rect: egui::Rect, icon: &str, name: &str) {
    let p = theme::current(ui.ctx());
    let painter = ui.painter();
    let icon_at = egui::pos2(rect.left() + CELL_PAD, rect.center().y);
    let icon_rect = painter.text(
        icon_at,
        egui::Align2::LEFT_CENTER,
        icon,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(icon_rect.right() + theme::space::XS, rect.top()),
        egui::pos2(rect.right() - CELL_PAD, rect.bottom()),
    );
    let galley = egui::WidgetText::from(egui::RichText::new(name).small()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        text_rect.width().max(0.0),
        egui::TextStyle::Body,
    );
    let at = egui::Align2::LEFT_CENTER
        .align_size_within_rect(galley.size(), text_rect)
        .min;
    painter.galley(at, galley, p.text_muted);
}

/// Thin scroll bars that are always in sight: columns that do not fit
/// must not go unnoticed.
pub(super) fn visible_scroll_bars(ui: &mut egui::Ui) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        floating_width: 4.0,
        foreground_color: true,
        dormant_handle_opacity: 0.35,
        active_handle_opacity: 0.5,
        dormant_background_opacity: 0.0,
        ..egui::style::ScrollStyle::thin()
    };
}

/// A 1 px line `width` wide. Returns its y.
pub(super) fn line(ui: &mut egui::Ui, width: f32, color: egui::Color32) -> f32 {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, (1.0, color));
    rect.center().y
}

/// «Nueva fila» under the table: a row-wide button, its icon where the
/// names start.
fn new_row_button(ui: &mut egui::Ui, width: f32) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Nueva fila"));
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.subtle);
    }
    let icon = ui.painter().text(
        egui::pos2(rect.left() + CELL_PAD, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icons::PLUS,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    ui.painter().text(
        egui::pos2(icon.right() + theme::space::XS, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Nueva fila",
        egui::TextStyle::Body.resolve(ui.style()),
        p.text_muted,
    );
    focus_ring(ui, rect, &response);
    response
}
