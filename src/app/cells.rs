//! Database cells (H6.2): how a value is shown and edited by its type, in
//! the table and on a row's page. The widgets only record what the user did
//! (`CellAction`); `SilexApp` applies it after drawing, to whichever of the
//! two holds the cell.
use anyhow::{Result, anyhow};
use eframe::egui::{self, TextEdit};
use silex_desktop::model::Page;
use silex_desktop::property::{
    CellValue, OptionColor, Property, PropertyKind, SelectOption, parse_input,
};

use super::*;
use super::{blocks::paint_check_box, icons, theme};

/// Space between a cell's edge and its content.
pub(super) const CELL_PAD: f32 = 8.0;

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

/// The cell of `row` in the column of `property` (when not typing in it).
pub(super) fn cell_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-cell", row, property))
}

/// The text field of a cell being typed in.
pub(super) fn cell_edit_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-cell-edit", row, property))
}

pub(super) fn select_menu_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-select-menu", row, property))
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

/// What the user did to a cell this frame, applied after drawing.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CellAction {
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

/// Where a cell is drawn and what it shows.
pub(super) struct Cell<'a> {
    pub(super) rect: egui::Rect,
    pub(super) row: &'a Page,
    pub(super) property: &'a Property,
    pub(super) value: Option<&'a CellValue>,
    /// Take the keyboard focus this frame (arrow keys in the table).
    pub(super) wanted: bool,
    pub(super) place: CellPlace,
}

/// Where a cell is shown, which changes a little how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CellPlace {
    /// A column of the table: numbers to the right, empty cells blank.
    Table,
    /// A row's page: everything to the left, empty values say «Vacío».
    Page,
}

impl Cell<'_> {
    pub(super) fn id(&self) -> egui::Id {
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

/// A cell of any type: its value, edited the way its type is.
pub(super) fn property_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    option_filter: &mut String,
    actions: &mut Vec<CellAction>,
) {
    match cell.property.kind {
        PropertyKind::Text | PropertyKind::Number | PropertyKind::Date => {
            text_cell(ui, cell, editing, actions);
        }
        PropertyKind::Checkbox => checkbox_cell(ui, cell, actions),
        PropertyKind::Select => select_cell(ui, cell, editing, option_filter, actions),
    }
}

/// Starts typing in a cell. A cell still being typed in is saved first, so
/// its text is not lost when the new one takes its place.
pub(super) fn start_typing(
    editing: &mut Option<CellEdit>,
    actions: &mut Vec<CellAction>,
    edit: CellEdit,
) {
    if let Some(old) = editing.replace(edit) {
        actions.push(CellAction::Commit(old));
    }
}

/// A text, number or date cell: its value, or a text field while typing.
/// Enter or leaving the cell saves; Escape drops what was typed.
pub(super) fn text_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    actions: &mut Vec<CellAction>,
) {
    let p = theme::current(ui.ctx());
    let number = cell.property.kind == PropertyKind::Number;
    let align = if number && cell.place == CellPlace::Table {
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
                actions.push(CellAction::Commit(edit));
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
    if shown.is_empty() && cell.place == CellPlace::Page {
        paint_cell_text(
            ui,
            cell.rect,
            "Vacío",
            egui::Align2::LEFT_CENTER,
            p.text_faint,
        );
    } else {
        paint_cell_text(ui, cell.rect, &shown, align, p.text);
    }
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
pub(super) fn checkbox_cell(ui: &mut egui::Ui, cell: &Cell<'_>, actions: &mut Vec<CellAction>) {
    let on = matches!(cell.value, Some(CellValue::Checkbox(true)));
    let response = ui.interact(cell.rect, cell.id(), egui::Sense::click());
    let name = cell.name();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &name));
    follow_focus(cell, &response);
    paint_check_box(
        ui,
        egui::pos2(cell.rect.left() + CELL_PAD + 8.0, cell.rect.center().y),
        on,
        &response,
    );
    if response.clicked() {
        actions.push(CellAction::SetValue {
            row: cell.row.id.clone(),
            property: cell.property.id.clone(),
            value: Some(CellValue::Checkbox(!on)),
        });
    }
}

/// A select cell: its option as a colored tag; a click opens a menu to
/// search the options, pick one, create one or empty the cell.
pub(super) fn select_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    filter: &mut String,
    actions: &mut Vec<CellAction>,
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
    } else if cell.place == CellPlace::Page {
        let p = theme::current(ui.ctx());
        paint_cell_text(
            ui,
            cell.rect,
            "Vacío",
            egui::Align2::LEFT_CENTER,
            p.text_faint,
        );
    }
    let menu = select_menu_id(&cell.row.id, &cell.property.id);
    let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
    if opening {
        filter.clear();
        if let Some(old) = editing.take() {
            actions.push(CellAction::Commit(old));
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
            let set = |option: Option<&SelectOption>| CellAction::SetValue {
                row: cell.row.id.clone(),
                property: cell.property.id.clone(),
                value: option.map(|o| CellValue::Select(o.id.clone())),
            };
            let create = || CellAction::CreateOption {
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
pub(super) fn option_row(
    ui: &mut egui::Ui,
    option: &SelectOption,
    selected: bool,
) -> egui::Response {
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
pub(super) fn cell_button(ui: &mut egui::Ui, cell: &Cell<'_>, shown: &str) -> egui::Response {
    let response = ui.interact(cell.rect, cell.id(), egui::Sense::click());
    let value = if shown.is_empty() { "vacío" } else { shown };
    let label = format!("{}: {value}", cell.name());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    follow_focus(cell, &response);
    focus_ring(ui, cell.rect, &response);
    response
}

/// Takes the focus when the arrow keys asked for this cell, and brings a
/// cell that gets the focus (arrows, Tab) into sight.
fn follow_focus(cell: &Cell<'_>, response: &egui::Response) {
    if cell.wanted {
        response.request_focus();
    }
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
}

/// Accent outline of a focused cell or header.
pub(super) fn focus_ring(ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
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
pub(super) fn paint_cell_text(
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
pub(super) fn paint_tag(
    ui: &egui::Ui,
    left_center: egui::Pos2,
    option: &SelectOption,
    max_width: f32,
) {
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

/// A `Ui` for drawing in `rect`, already allocated by the caller. Unlike
/// `scope_builder` or `put`, it never moves the parent's cursor (egui sets
/// the cursor below the last child, even if that is above where it was).
pub(super) fn inside(ui: &mut egui::Ui, rect: egui::Rect, layout: egui::Layout) -> egui::Ui {
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

/// Inner padding of the table's menus: the text of their field, their
/// captions, the icons of their items and the option tags start here.
pub(super) const MENU_PAD: f32 = 6.0;

/// Gives the menu's items the same padding as its field and captions
/// (egui menus use 2 px, too tight next to a framed field).
pub(super) fn menu_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().button_padding = egui::vec2(MENU_PAD, 4.0);
}

/// A text field at the top of a menu, its text at MENU_PAD.
pub(super) fn menu_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    name: &str,
) -> egui::Response {
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
/// Long text wraps instead of widening the menu (which would move it).
pub(super) fn menu_caption(ui: &mut egui::Ui, text: &str) {
    let p = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(MENU_PAD);
        ui.add(
            egui::Label::new(egui::RichText::new(text).small().color(p.text_muted))
                .wrap_mode(egui::TextWrapMode::Wrap),
        );
    });
}

impl SilexApp {
    /// The type, number of options and value of a cell wherever it is shown:
    /// the open table or the open row's page. `None` if its property is not
    /// shown any more (deleted meanwhile).
    fn cell_info(
        &self,
        row: &str,
        property: &str,
    ) -> Option<(PropertyKind, usize, Option<CellValue>)> {
        if let Some(state) = &self.database
            && let Some(p) = state.properties.iter().find(|p| p.id == property)
        {
            let value = state.values.get(row).and_then(|cells| cells.get(property));
            return Some((p.kind, p.options.len(), value.cloned()));
        }
        let page = self.row_page.as_ref().filter(|page| page.row == row)?;
        let p = page.properties.iter().find(|p| p.id == property)?;
        Some((p.kind, p.options.len(), page.values.get(property).cloned()))
    }

    /// Saves the text typed in a cell. Text that does not fit the property
    /// (a number, a date) is reported and the cell keeps its value.
    pub(super) fn commit_cell_edit(&mut self, edit: &CellEdit) -> Result<()> {
        let Some((kind, _, current)) = self.cell_info(&edit.row, &edit.property) else {
            return Ok(());
        };
        let value = parse_input(kind, &edit.text)
            .map_err(|message| anyhow!("{message} No se ha guardado."))?;
        if value == current {
            return Ok(());
        }
        self.set_cell(&edit.row, &edit.property, value)
    }

    /// Stores a cell's value (`None` empties it) and updates the table and
    /// the row's page, whichever shows it.
    pub(super) fn set_cell(
        &mut self,
        row: &str,
        property: &str,
        value: Option<CellValue>,
    ) -> Result<()> {
        self.db.set_value(row, property, value.as_ref())?;
        let value = value.filter(|v| !v.is_empty());
        if let Some(state) = self.database.as_mut() {
            let cells = state.values.entry(row.to_owned()).or_default();
            match &value {
                Some(value) => cells.insert(property.to_owned(), value.clone()),
                None => cells.remove(property),
            };
        }
        if let Some(page) = self.row_page.as_mut().filter(|page| page.row == row) {
            match value {
                Some(value) => page.values.insert(property.to_owned(), value),
                None => page.values.remove(property),
            };
        }
        Ok(())
    }

    /// Creates an option named `name` in a select property (next color in
    /// turn) and puts it in the cell of `row`.
    pub(super) fn create_option(&mut self, row: &str, property: &str, name: &str) -> Result<()> {
        let existing = self.cell_info(row, property).map_or(0, |(_, n, _)| n);
        let option = self.db.add_select_option(
            property,
            name.trim(),
            OptionColor::for_new_option(existing),
        )?;
        self.reload_properties()?;
        self.reload_row_page()?;
        self.set_cell(row, property, Some(CellValue::Select(option)))
    }

    /// Applies what the user did to cells this frame. Text typed in a cell
    /// is saved before anything else changes it.
    pub(super) fn apply_cell_actions(&mut self, mut actions: Vec<CellAction>) {
        actions.sort_by_key(|action| !matches!(action, CellAction::Commit(_)));
        for action in actions {
            let result = match action {
                CellAction::SetValue {
                    row,
                    property,
                    value,
                } => self.set_cell(&row, &property, value),
                CellAction::CreateOption {
                    row,
                    property,
                    name,
                } => self.create_option(&row, &property, &name),
                CellAction::Commit(edit) => self.commit_cell_edit(&edit),
            };
            self.report(result);
        }
    }
}
