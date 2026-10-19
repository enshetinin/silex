//! A database page (ADR 0006): its views and a table of its rows.
//! Rows are pages (H6.1): the name cell is the page title, «Abrir» opens the
//! row as a page, and right-click shows the usual page menu. Typed
//! properties (H6.2) are the other columns; each cell is edited by its type
//! and saved at once in its own transaction.
use std::collections::HashSet;

use anyhow::{Result, anyhow};
use eframe::egui::{self, TextEdit};
use silex_desktop::db::Shift;
use silex_desktop::model::{DatabaseView, Page};
use silex_desktop::property::{
    CellValue, OptionColor, Property, PropertyKind, SelectOption, TableValues, parse_input,
};

use super::*;
use super::{blocks::paint_check_box, icons, theme};

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
    /// Focus this row's name on the next frame (a new row, «Renombrar»).
    pub(super) focus: Option<String>,
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
}

/// A cell being typed in: its row, its property and the text so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CellEdit {
    pub(super) row: String,
    pub(super) property: String,
    pub(super) text: String,
    /// Put the cursor in the field on the next frame.
    pub(super) focus: bool,
}

/// Height of a table row.
const ROW_HEIGHT: f32 = 34.0;
/// Height of the header row.
const HEADER_HEIGHT: f32 = 30.0;
/// The name column takes the room left by the others, but not less.
const NAME_MIN_WIDTH: f32 = 240.0;
/// The «+» column that adds a property.
const ADD_COLUMN_WIDTH: f32 = 40.0;
/// Space between a cell's edge and its content.
const CELL_PAD: f32 = 8.0;

fn column_width(kind: PropertyKind) -> f32 {
    match kind {
        PropertyKind::Text => 180.0,
        PropertyKind::Number => 110.0,
        PropertyKind::Checkbox => 96.0,
        PropertyKind::Date => 124.0,
        PropertyKind::Select => 150.0,
    }
}

/// Icon of a property type (header and menus).
pub(super) fn property_icon(kind: PropertyKind) -> &'static str {
    match kind {
        PropertyKind::Text => icons::TEXT_ALIGN_LEFT,
        PropertyKind::Number => icons::HASH,
        PropertyKind::Checkbox => icons::TODO,
        PropertyKind::Date => icons::CALENDAR,
        PropertyKind::Select => icons::CARET_CIRCLE_DOWN,
    }
}

pub(super) fn row_name_id(row_id: &str) -> egui::Id {
    egui::Id::unique(("silex-row-name", row_id))
}

/// The cell of `row` in the column of `property` (when not typing in it).
pub(super) fn cell_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-cell", row, property))
}

/// The text field of a cell being typed in.
pub(super) fn cell_edit_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-cell-edit", row, property))
}

/// The header button of a property (opens its menu).
pub(super) fn property_header_id(property: &str) -> egui::Id {
    egui::Id::unique(("silex-property", property))
}

fn property_menu_id(property: &str) -> egui::Id {
    egui::Id::unique(("silex-property-menu", property))
}

pub(super) fn select_menu_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-select-menu", row, property))
}

/// What the user did in the table this frame, applied after drawing.
enum TableAction {
    Open(String),
    AddRow,
    Row(RowAction),
    AddProperty(PropertyKind, String),
    RenameProperty(String, String),
    MoveProperty(String, Shift),
    DeleteProperty(String),
    SetValue {
        row: String,
        property: String,
        value: Option<CellValue>,
    },
    CreateOption {
        row: String,
        property: String,
        name: String,
    },
    /// Save what was typed in a cell that is no longer being edited.
    Commit(CellEdit),
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
            editing: None,
            renaming: None,
            new_property: String::new(),
            option_filter: String::new(),
            scroll: egui::Vec2::ZERO,
        });
        Ok(())
    }

    /// Saves the table's pending edits, then reloads the rows (they may
    /// have been added, renamed, moved or trashed from elsewhere). Values
    /// are not reloaded (~80 ms with 10 000 rows): they only change from the
    /// table, and a row restored from the trash opens as a page, so its
    /// values are read when the database opens again.
    pub(super) fn refresh_database(&mut self) -> Result<()> {
        self.save_table_edits()?;
        if let Some(state) = self.database.as_mut() {
            state.rows = self.db.database_rows(&state.id)?;
        }
        Ok(())
    }

    /// Reloads the columns after a change to them (and the values, which
    /// follow their properties out and back).
    fn reload_properties(&mut self) -> Result<()> {
        if let Some(state) = self.database.as_mut() {
            state.properties = self.db.database_properties(&state.id)?;
            state.deleted = self.db.deleted_properties(&state.id)?;
            state.values = self.db.database_values(&state.id)?;
        }
        Ok(())
    }

    /// Saves everything the table has pending: edited row names and the
    /// text typed in the open cell (which stays open).
    pub(super) fn save_table_edits(&mut self) -> Result<()> {
        let names = self.save_row_names();
        let cell = match self.database.as_ref().and_then(|d| d.editing.clone()) {
            Some(edit) => self.commit_cell_edit(&edit),
            None => Ok(()),
        };
        names.and(cell)
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

    /// Saves the text typed in a cell. Text that does not fit the property
    /// (a number, a date) is reported and the cell keeps its value.
    pub(super) fn commit_cell_edit(&mut self, edit: &CellEdit) -> Result<()> {
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        // Deleted meanwhile: there is nothing to save into.
        let Some(property) = state.properties.iter().find(|p| p.id == edit.property) else {
            return Ok(());
        };
        let value = parse_input(property.kind, &edit.text)
            .map_err(|message| anyhow!("{message} No se ha guardado."))?;
        let current = state
            .values
            .get(&edit.row)
            .and_then(|cells| cells.get(&edit.property));
        if value.as_ref() == current {
            return Ok(());
        }
        self.set_cell(&edit.row, &edit.property, value)
    }

    /// Stores a cell's value (`None` empties it) and updates the table.
    pub(super) fn set_cell(
        &mut self,
        row: &str,
        property: &str,
        value: Option<CellValue>,
    ) -> Result<()> {
        self.db.set_value(row, property, value.as_ref())?;
        if let Some(state) = self.database.as_mut() {
            let cells = state.values.entry(row.to_owned()).or_default();
            match value.filter(|v| !v.is_empty()) {
                Some(value) => cells.insert(property.to_owned(), value),
                None => cells.remove(property),
            };
        }
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
        self.notice = Some(format!(
            "Se borró la propiedad «{name}». Puedes restaurarla con sus valores desde «…» → «Propiedades borradas»."
        ));
        Ok(())
    }

    pub(super) fn restore_property(&mut self, id: &str) -> Result<()> {
        self.db.restore_property(id)?;
        self.reload_properties()
    }

    /// Creates an option named `name` in a select property (next color in
    /// turn) and puts it in the cell of `row`.
    pub(super) fn create_option(&mut self, row: &str, property: &str, name: &str) -> Result<()> {
        let existing = self
            .database
            .as_ref()
            .and_then(|d| d.properties.iter().find(|p| p.id == property))
            .map_or(0, |p| p.options.len());
        let option = self.db.add_select_option(
            property,
            name.trim(),
            OptionColor::for_new_option(existing),
        )?;
        self.reload_properties()?;
        self.set_cell(row, property, Some(CellValue::Select(option)))
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
        let mut actions = self.draw_table(ui, &mut state);
        // Text typed in a cell is saved before anything else changes the
        // table (deleting its property, opening another page…).
        actions.sort_by_key(|action| !matches!(action, TableAction::Commit(_)));
        let database = state.id.clone();
        let editing_name = state
            .rows
            .iter()
            .any(|r| ui.memory(|m| m.has_focus(row_name_id(&r.id))));
        self.database = Some(state);
        // A name is saved once its cell is left (or scrolled away).
        if !editing_name {
            let result = self.save_row_names();
            self.report(result);
        }
        for action in actions {
            let result = self.apply_table_action(&database, action);
            self.report(result);
        }
    }

    fn apply_table_action(&mut self, database: &str, action: TableAction) -> Result<()> {
        match action {
            TableAction::Open(row) => self.open_page(row)?,
            TableAction::AddRow => self.add_row(database)?,
            // Rows are not in the sidebar: rename in the table.
            TableAction::Row(RowAction::StartRename(id)) => {
                if let Some(state) = self.database.as_mut() {
                    state.focus = Some(id);
                }
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
            TableAction::SetValue {
                row,
                property,
                value,
            } => self.set_cell(&row, &property, value)?,
            TableAction::CreateOption {
                row,
                property,
                name,
            } => self.create_option(&row, &property, &name)?,
            TableAction::Commit(edit) => self.commit_cell_edit(&edit)?,
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

    fn draw_table(&self, ui: &mut egui::Ui, state: &mut DatabaseState) -> Vec<TableAction> {
        let p = theme::current(ui.ctx());
        let mut actions = Vec::new();

        // View tabs (one «Tabla» view until H6.3) and the row count. Their
        // content lines up with the cells' text, at CELL_PAD from each edge.
        let mut selected_tab = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.x = CELL_PAD;
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
                    selected_tab = Some(tab.rect);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Also at the end of the header, which may be out of sight
                // in a wide table.
                let add = ui
                    .add(
                        egui::Button::new((
                            icons::icon(icons::PLUS).color(p.text_muted),
                            egui::RichText::new(" Propiedad")
                                .small()
                                .color(p.text_muted),
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
                let count = match state.rows.len() {
                    1 => "1 fila".to_owned(),
                    n => format!("{n} filas"),
                };
                ui.label(egui::RichText::new(count).small().color(p.text_muted));
            });
        });

        let columns: f32 = state.properties.iter().map(|p| column_width(p.kind)).sum();
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
        let wanted = state
            .focus
            .as_ref()
            .and_then(|id| state.rows.iter().position(|r| &r.id == id));
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
                // Thin bars that are always in sight: the columns that do
                // not fit must not go unnoticed.
                ui.spacing_mut().scroll = egui::style::ScrollStyle {
                    floating_width: 4.0,
                    foreground_color: true,
                    dormant_handle_opacity: 0.35,
                    active_handle_opacity: 0.5,
                    dormant_background_opacity: 0.0,
                    ..egui::style::ScrollStyle::thin()
                };
                area.show_rows(ui, ROW_HEIGHT, state.rows.len(), |ui, range| {
                    ui.set_min_width(width);
                    self.table_rows(ui, state, range.clone(), name_width, width, &mut actions);
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
            && !state.rows[drawn].iter().any(|r| r.id == edit.row)
            && let Some(edit) = state.editing.take()
        {
            actions.push(TableAction::Commit(edit));
        }

        if state.rows.is_empty() {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(visible, ROW_HEIGHT), egui::Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, (1.0, p.border));
            paint_cell_text(
                ui,
                rect,
                "Todavía no hay filas.",
                egui::Align2::LEFT_CENTER,
                p.text_muted,
            );
        }
        ui.add_space(theme::space::XS);
        if new_row_button(ui, visible).clicked() {
            actions.push(TableAction::AddRow);
        }
        actions
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
            renaming,
            new_property,
            ..
        } = state;
        let count = properties.len();
        let mut x = name.right();
        for (index, property) in properties.iter().enumerate() {
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
            }
            egui::Popup::menu(&button)
                .id(menu)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(220.0)
                .show(|ui| {
                    property_menu(ui, renaming, property, (index, count), opening, actions);
                });
        }
        // A new name is saved when its menu closes; Escape drops it.
        if let Some((id, _)) = renaming
            && !egui::Popup::is_id_open(ui.ctx(), property_menu_id(id))
            && let Some((id, name)) = renaming.take()
            && !ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            actions.push(TableAction::RenameProperty(id, name));
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
    /// cell per property.
    fn table_rows(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        range: std::ops::Range<usize>,
        name_width: f32,
        width: f32,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        let DatabaseState {
            rows,
            properties,
            values,
            dirty,
            focus,
            editing,
            option_filter,
            ..
        } = state;
        for row in rows[range].iter_mut() {
            let name_id = row_name_id(&row.id);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
            let focused = ui.memory(|m| m.has_focus(name_id));
            let wanted = focus.as_deref() == Some(row.id.as_str());
            if ui.rect_contains_pointer(rect) || focused {
                ui.painter().rect_filled(rect, 0.0, p.subtle);
            }
            ui.painter()
                .hline(rect.x_range(), rect.bottom() - 0.5, (1.0, p.border));

            let name_rect =
                egui::Rect::from_min_size(rect.min, egui::vec2(name_width, rect.height()));
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
                if wanted {
                    name.request_focus();
                    *focus = None;
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
            for property in properties.iter() {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(column_width(property.kind), rect.height()),
                );
                x = cell.right();
                ui.painter()
                    .vline(cell.left(), cell.y_range(), (1.0, p.border));
                let value = values
                    .get(&row.id)
                    .and_then(|cells| cells.get(&property.id));
                let place = Cell {
                    rect: cell,
                    row,
                    property,
                    value,
                };
                match property.kind {
                    PropertyKind::Text | PropertyKind::Number | PropertyKind::Date => {
                        text_cell(ui, &place, editing, actions);
                    }
                    PropertyKind::Checkbox => checkbox_cell(ui, &place, actions),
                    PropertyKind::Select => {
                        select_cell(ui, &place, editing, option_filter, actions);
                    }
                }
            }
            ui.painter().vline(x, rect.y_range(), (1.0, p.border));
        }
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

/// Inner padding of the table's menus: the text of their field, their
/// captions, the icons of their items and the option tags start here.
const MENU_PAD: f32 = 6.0;

/// Gives the menu's items the same padding as its field and captions
/// (egui menus use 2 px, too tight next to a framed field).
fn menu_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().button_padding = egui::vec2(MENU_PAD, 4.0);
}

/// A text field at the top of a menu, its text at MENU_PAD.
fn menu_field(ui: &mut egui::Ui, text: &mut String, hint: &str, name: &str) -> egui::Response {
    let p = theme::current(ui.ctx());
    ui.add(
        TextEdit::singleline(text)
            .margin(egui::Margin::symmetric(MENU_PAD as i8, 5))
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(egui::RichText::new(hint).color(p.text_faint))
            .desired_width(f32::INFINITY),
    )
    .accessible_name(name)
}

/// A small muted line in a menu («Tipo»), lined up with the items.
fn menu_caption(ui: &mut egui::Ui, text: &str) {
    let p = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(MENU_PAD);
        ui.label(egui::RichText::new(text).small().color(p.text_muted));
    });
}

/// A property's menu: its name (editable), type, moving and deleting.
fn property_menu(
    ui: &mut egui::Ui,
    renaming: &mut Option<(String, String)>,
    property: &Property,
    (index, count): (usize, usize),
    opening: bool,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    menu_spacing(ui);
    if let Some((_, name)) = renaming.as_mut().filter(|(id, _)| *id == property.id) {
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
    ui.separator();
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

/// Where a cell is drawn and what it shows.
struct Cell<'a> {
    rect: egui::Rect,
    row: &'a Page,
    property: &'a Property,
    value: Option<&'a CellValue>,
}

impl Cell<'_> {
    fn id(&self) -> egui::Id {
        cell_id(&self.row.id, &self.property.id)
    }

    /// "Propiedad, fila" (what a screen reader says before the value).
    fn name(&self) -> String {
        format!("{}, {}", self.property.name, display_title(&self.row.title))
    }

    fn shown(&self) -> String {
        self.value
            .map(|v| v.display(self.property))
            .unwrap_or_default()
    }

    fn is_typing(&self, editing: &Option<CellEdit>) -> bool {
        editing
            .as_ref()
            .is_some_and(|e| e.row == self.row.id && e.property == self.property.id)
    }
}

/// Starts typing in a cell. A cell still being typed in is saved first, so
/// its text is not lost when the new one takes its place.
fn start_typing(editing: &mut Option<CellEdit>, actions: &mut Vec<TableAction>, edit: CellEdit) {
    if let Some(old) = editing.replace(edit) {
        actions.push(TableAction::Commit(old));
    }
}

/// A text, number or date cell: its value, or a text field while typing.
/// Enter or leaving the cell saves; Escape drops what was typed.
fn text_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    let number = cell.property.kind == PropertyKind::Number;
    let align = if number {
        egui::Align2::RIGHT_CENTER
    } else {
        egui::Align2::LEFT_CENTER
    };
    if cell.is_typing(editing) {
        let Some(edit) = editing.as_mut() else {
            return;
        };
        let hint = match cell.property.kind {
            PropertyKind::Date => "AAAA-MM-DD",
            PropertyKind::Number => "0",
            _ => "Vacío",
        };
        // Frameless, in the same rect as the shown value: the text does not
        // move when typing starts.
        let field = inside(
            ui,
            cell.rect.shrink2(egui::vec2(CELL_PAD, 0.0)),
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
        )
        .add(
            TextEdit::singleline(&mut edit.text)
                .id(cell_edit_id(&cell.row.id, &cell.property.id))
                .frame(egui::Frame::NONE)
                .align(align)
                .hint_text(egui::RichText::new(hint).color(p.text_faint))
                .desired_width(f32::INFINITY),
        )
        .accessible_name(cell.name());
        ui.painter().rect_stroke(
            cell.rect.shrink(1.5),
            egui::CornerRadius::same(theme::radius::SM),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
        if std::mem::take(&mut edit.focus) {
            field.request_focus();
        }
        if field.lost_focus() {
            let (enter, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if let Some(edit) = editing.take()
                && !escape
            {
                actions.push(TableAction::Commit(edit));
            }
            // From the keyboard, the cell keeps the focus.
            if enter || escape {
                ui.memory_mut(|m| m.request_focus(cell.id()));
            }
        }
        return;
    }
    let shown = cell.shown();
    let button = cell_button(ui, cell, &shown);
    paint_cell_text(ui, cell.rect, &shown, align, p.text);
    if button.clicked() {
        start_typing(
            editing,
            actions,
            CellEdit {
                row: cell.row.id.clone(),
                property: cell.property.id.clone(),
                text: shown,
                focus: true,
            },
        );
    }
}

/// A checkbox cell: a click (or Space/Enter) ticks or unticks it.
fn checkbox_cell(ui: &mut egui::Ui, cell: &Cell<'_>, actions: &mut Vec<TableAction>) {
    let on = matches!(cell.value, Some(CellValue::Checkbox(true)));
    let response = ui.interact(cell.rect, cell.id(), egui::Sense::click());
    let name = cell.name();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &name));
    paint_check_box(
        ui,
        egui::pos2(cell.rect.left() + CELL_PAD + 8.0, cell.rect.center().y),
        on,
        &response,
    );
    if response.clicked() {
        actions.push(TableAction::SetValue {
            row: cell.row.id.clone(),
            property: cell.property.id.clone(),
            value: Some(CellValue::Checkbox(!on)),
        });
    }
}

/// A select cell: its option as a colored tag; a click opens a menu to
/// search the options, pick one, create one or empty the cell.
fn select_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    filter: &mut String,
    actions: &mut Vec<TableAction>,
) {
    let current = match cell.value {
        Some(CellValue::Select(id)) => cell.property.option(id),
        _ => None,
    };
    let button = cell_button(ui, cell, &cell.shown());
    if let Some(option) = current {
        paint_tag(
            ui,
            egui::pos2(cell.rect.left() + CELL_PAD, cell.rect.center().y),
            option,
            cell.rect.width() - 2.0 * CELL_PAD,
        );
    }
    let menu = select_menu_id(&cell.row.id, &cell.property.id);
    let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
    if opening {
        filter.clear();
        if let Some(old) = editing.take() {
            actions.push(TableAction::Commit(old));
        }
    }
    egui::Popup::menu(&button)
        .id(menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(240.0)
        .show(|ui| {
            menu_spacing(ui);
            let field = menu_field(
                ui,
                filter,
                "Busca o crea una opción",
                &format!("Opción de {}", cell.name()),
            );
            if opening {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let typed = filter.trim().to_owned();
            let (matches, exact) = cell.property.matching_options(&typed);
            let set = |option: Option<&SelectOption>| TableAction::SetValue {
                row: cell.row.id.clone(),
                property: cell.property.id.clone(),
                value: option.map(|o| CellValue::Select(o.id.clone())),
            };
            let create = || TableAction::CreateOption {
                row: cell.row.id.clone(),
                property: cell.property.id.clone(),
                name: typed.clone(),
            };
            let mut chosen = None;
            ui.add_space(theme::space::XS);
            for option in &matches {
                let selected = current.is_some_and(|c| c.id == option.id);
                if option_row(ui, option, selected).clicked() {
                    chosen = Some(set(Some(option)));
                }
            }
            if !typed.is_empty() && exact.is_none() {
                let label = format!("Crear «{typed}»");
                if ui
                    .button((icons::icon(icons::PLUS), format!(" {label}")))
                    .clicked()
                {
                    chosen = Some(create());
                }
            }
            if cell.property.options.is_empty() && typed.is_empty() {
                menu_caption(ui, "Escribe para crear la primera opción.");
            }
            // Enter takes the option with that name, else the first match,
            // else creates it.
            if enter && chosen.is_none() && !typed.is_empty() {
                chosen = Some(match exact.or(matches.first().copied()) {
                    Some(option) => set(Some(option)),
                    None => create(),
                });
            }
            if current.is_some() {
                ui.separator();
                if ui
                    .button((icons::icon(icons::CLOSE), " Quitar la opción"))
                    .clicked()
                {
                    chosen = Some(set(None));
                }
            }
            if let Some(action) = chosen {
                actions.push(action);
                ui.close();
            }
        });
}

/// One option in a select menu: its tag, and a tick on the current one.
fn option_row(ui: &mut egui::Ui, option: &SelectOption, selected: bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, selected, &option.name)
    });
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.subtle);
    }
    paint_tag(
        ui,
        egui::pos2(rect.left() + MENU_PAD, rect.center().y),
        option,
        rect.width() - 40.0,
    );
    if selected {
        ui.painter().text(
            egui::pos2(rect.right() - MENU_PAD, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            icons::CHECK,
            icons::font(theme::icon::MD),
            p.accent,
        );
    }
    response
}

/// A clickable, focusable cell named "Propiedad, fila: valor".
fn cell_button(ui: &mut egui::Ui, cell: &Cell<'_>, shown: &str) -> egui::Response {
    let response = ui.interact(cell.rect, cell.id(), egui::Sense::click());
    let value = if shown.is_empty() { "vacío" } else { shown };
    let label = format!("{}: {value}", cell.name());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    focus_ring(ui, cell.rect, &response);
    response
}

/// Accent outline of a focused cell or header.
fn focus_ring(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
    if response.has_focus() {
        let p = theme::current(ui.ctx());
        ui.painter().rect_stroke(
            rect.shrink(1.5),
            egui::CornerRadius::same(theme::radius::SM),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }
}

/// One line of text inside a cell, cut with "…" if it does not fit.
fn paint_cell_text(
    ui: &egui::Ui,
    rect: egui::Rect,
    text: &str,
    align: egui::Align2,
    color: egui::Color32,
) {
    if text.is_empty() {
        return;
    }
    let inner = rect.shrink2(egui::vec2(CELL_PAD, 0.0));
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        inner.width(),
        egui::TextStyle::Body,
    );
    let at = align.align_size_within_rect(galley.size(), inner).min;
    ui.painter().galley(at, galley, color);
}

/// An option's colored tag, starting at `left_center`, at most `max_width` wide.
fn paint_tag(ui: &egui::Ui, left_center: egui::Pos2, option: &SelectOption, max_width: f32) {
    let (bg, fg) = theme::option_colors(ui.ctx(), option.color);
    let pad = egui::vec2(6.0, 2.0);
    let galley = egui::WidgetText::from(egui::RichText::new(&option.name).color(fg)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (max_width - 2.0 * pad.x).max(0.0),
        egui::TextStyle::Body,
    );
    let size = galley.size() + 2.0 * pad;
    let rect = egui::Rect::from_min_size(
        egui::pos2(left_center.x, left_center.y - size.y / 2.0),
        size,
    );
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), bg);
    painter.galley(rect.min + pad, galley, fg);
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

/// A `Ui` for drawing in `rect`, already allocated by the caller. Unlike
/// `scope_builder` or `put`, it never moves the parent's cursor (egui sets
/// the cursor below the last child, even if that is above where it was).
fn inside(ui: &mut egui::Ui, rect: egui::Rect, layout: egui::Layout) -> egui::Ui {
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

/// A 1 px line `width` wide. Returns its y.
fn line(ui: &mut egui::Ui, width: f32, color: egui::Color32) -> f32 {
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
