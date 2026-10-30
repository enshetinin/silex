//! Several rows at once (H11.2): a box in front of each row chooses it
//! (Shift+click chooses the range from the last box clicked), the header's
//! box chooses every row in sight, and while rows are chosen the toolbar
//! becomes a bar to act on all of them: duplicate them (H11.4), send them
//! to the trash or put a value in one property. Each action is one transaction and one step of
//! Cmd+Z. Escape, «×», changing the view or its filters empty the
//! selection; rows that leave the table leave it too. Only in the table
//! view: a board has no boxes.
use std::collections::HashSet;

use anyhow::Result;
use eframe::egui;
use silex_desktop::model::Page;
use silex_desktop::property::{CellValue, Date, PropertyKind, parse_input};

use super::*;
use super::{icons, theme};

/// Width of the column of boxes, in front of «Nombre».
pub(super) const SELECT_COLUMN_WIDTH: f32 = 32.0;

/// The box of a row.
pub(super) fn select_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-row-select", row))
}

fn select_all_id(database: &str) -> egui::Id {
    egui::Id::unique(("silex-rows-select-all", database))
}

/// What «Poner valor…» is showing: the property chosen (none: the list of
/// properties) and what is typed for it.
#[derive(Debug, Default)]
pub(super) struct BulkValue {
    pub(super) property: Option<String>,
    pub(super) text: String,
    /// Why the typed text is not a value of the property.
    pub(super) error: Option<String>,
}

impl DatabaseState {
    /// The chosen rows, in the view's order.
    pub(super) fn chosen(&self) -> Vec<String> {
        self.order
            .iter()
            .filter_map(|&i| self.rows.get(i))
            .filter(|r| self.selection.contains(&r.id))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Empties the selection (and closes «Poner valor…»).
    pub(super) fn clear_selection(&mut self) {
        self.selection.clear();
        self.select_anchor = None;
        self.bulk = BulkValue::default();
    }

    /// Rows no longer in the table leave the selection.
    pub(super) fn prune_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let live: HashSet<&str> = self.rows.iter().map(|r| r.id.as_str()).collect();
        self.selection.retain(|id| live.contains(id.as_str()));
    }
}

/// A click on the box of the row at `index` (in the view's order): it
/// joins or leaves the selection; with Shift, the rows from the last box
/// clicked to this one join it.
pub(super) fn click_box(
    rows: &[Page],
    order: &[usize],
    selection: &mut HashSet<String>,
    anchor: &mut Option<String>,
    index: usize,
    shift: bool,
) {
    let id_at = |i: usize| {
        order
            .get(i)
            .and_then(|&r| rows.get(r))
            .map(|r| r.id.clone())
    };
    let Some(id) = id_at(index) else {
        return;
    };
    let from = anchor.as_ref().filter(|_| shift).and_then(|a| {
        order
            .iter()
            .position(|&r| rows.get(r).is_some_and(|r| &r.id == a))
    });
    match from {
        Some(from) => {
            let (low, high) = (from.min(index), from.max(index));
            selection.extend((low..=high).filter_map(id_at));
        }
        None => {
            if !selection.remove(&id) {
                selection.insert(id.clone());
            }
        }
    }
    *anchor = Some(id);
}

/// A check box at the centre of `rect`, seen while `shown` (or hovered,
/// focused or ticked). `mixed` draws a dash: some rows, not all.
pub(super) fn select_box(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    on: bool,
    mixed: bool,
    shown: bool,
    name: &str,
) -> egui::Response {
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, name));
    let seen = shown || on || mixed || response.hovered() || response.has_focus();
    if seen && ui.is_rect_visible(rect) {
        let center = rect.center();
        paint_check_box(ui, center, on, &response);
        if mixed && !on {
            let p = theme::current(ui.ctx());
            let b = egui::Rect::from_center_size(center, egui::vec2(16.0, 16.0));
            ui.painter()
                .rect_filled(b, egui::CornerRadius::same(theme::radius::SM), p.accent);
            ui.painter().hline(
                center.x - 4.0..=center.x + 4.0,
                center.y,
                egui::Stroke::new(1.8, p.on_accent),
            );
        }
    }
    response.on_hover_text(name)
}

/// The header's box: chooses every row in sight, or none if all are.
pub(super) fn header_box(ui: &mut egui::Ui, rect: egui::Rect, state: &mut DatabaseState) {
    let visible: Vec<String> = state
        .order
        .iter()
        .filter_map(|&i| state.rows.get(i))
        .map(|r| r.id.clone())
        .collect();
    let chosen = visible
        .iter()
        .filter(|id| state.selection.contains(*id))
        .count();
    let all = !visible.is_empty() && chosen == visible.len();
    let hot = ui.rect_contains_pointer(rect) || !state.selection.is_empty();
    let response = select_box(
        ui,
        rect,
        select_all_id(&state.id),
        all,
        chosen > 0,
        hot,
        "Seleccionar todas las filas visibles",
    );
    if response.clicked() {
        if all {
            state.clear_selection();
        } else {
            state.selection.extend(visible);
        }
    }
}

/// Escape empties the selection, unless it is for something else: a menu,
/// a cell or a name being typed in.
pub(super) fn selection_keys(ctx: &egui::Context, state: &mut DatabaseState) {
    let typing = state.editing.is_some()
        || state
            .rows
            .iter()
            .any(|r| ctx.memory(|m| m.has_focus(row_name_id(&r.id))));
    if state.selection.is_empty() || typing || egui::Popup::is_any_open(ctx) {
        return;
    }
    if take_exact_key(ctx, egui::Key::Escape, egui::Modifiers::NONE) {
        state.clear_selection();
    }
}

/// The toolbar while rows are chosen: how many, and what to do with them.
pub(super) fn selection_bar(
    ui: &mut egui::Ui,
    state: &mut DatabaseState,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    let id = egui::Id::unique(("silex-db-selection-bar", &state.id));
    right_aligned(ui, id, |ui| {
        let count = state.chosen().len();
        let label = if count == 1 {
            "1 fila seleccionada".to_owned()
        } else {
            format!("{count} filas seleccionadas")
        };
        ui.label(egui::RichText::new(label).small().color(p.text));
        let small = |icon: &str, text: &str| {
            egui::Button::new((
                icons::icon(icon).color(p.text_muted),
                egui::RichText::new(text).small().color(p.text_muted),
            ))
            .frame_when_inactive(false)
        };
        let set = ui
            .add(small(icons::TEXT, " Poner valor…"))
            .accessible_name("Poner valor…");
        // It opens on the list of properties.
        if set.clicked()
            && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&set))
        {
            state.bulk = BulkValue::default();
        }
        egui::Popup::menu(&set)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(240.0)
            .show(|ui| bulk_menu(ui, state, actions));
        if ui
            .add(small(icons::COPY, " Duplicar"))
            .accessible_name("Duplicar")
            .clicked()
        {
            actions.push(TableAction::DuplicateSelected);
        }
        if ui
            .add(small(icons::TRASH, " Enviar a la papelera"))
            .accessible_name("Enviar a la papelera")
            .clicked()
        {
            actions.push(TableAction::TrashSelected);
        }
        if icons::button(
            ui,
            true,
            icons::CLOSE,
            p.text_muted,
            "Quitar la selección",
            Some(egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Escape,
            )),
        )
        .clicked()
        {
            state.clear_selection();
        }
    });
}

/// «Poner valor…»: first the property, then its value (by its kind).
fn bulk_menu(ui: &mut egui::Ui, state: &mut DatabaseState, actions: &mut Vec<TableAction>) {
    menu_spacing(ui);
    let chosen = state
        .bulk
        .property
        .as_ref()
        .and_then(|id| state.properties.iter().find(|p| &p.id == id))
        .cloned();
    let Some(property) = chosen else {
        menu_caption(ui, "Propiedad");
        for property in &state.properties {
            if ui
                .button((
                    icons::icon(property_icon(property.kind)),
                    format!(" {}", property.name),
                ))
                .clicked()
            {
                state.bulk = BulkValue {
                    property: Some(property.id.clone()),
                    ..BulkValue::default()
                };
            }
        }
        return;
    };
    if ui
        .button((
            icons::icon(icons::ARROW_LEFT),
            format!(" {}", property.name),
        ))
        .on_hover_text("Elegir otra propiedad")
        .clicked()
    {
        state.bulk = BulkValue::default();
        return;
    }
    let mut set = |value: Option<CellValue>| {
        actions.push(TableAction::SetSelected(property.id.clone(), value));
    };
    match property.kind {
        PropertyKind::Checkbox => {
            if ui.button((icons::icon(icons::TODO), " Marcar")).clicked() {
                set(Some(CellValue::Checkbox(true)));
                ui.close();
            }
            if ui
                .button((icons::icon(icons::CLOSE), " Desmarcar"))
                .clicked()
            {
                set(None);
                ui.close();
            }
        }
        PropertyKind::Select => {
            for option in &property.options {
                if ui.button(option.name.as_str()).clicked() {
                    set(Some(CellValue::Select(option.id.clone())));
                    ui.close();
                }
            }
        }
        // Each chosen row gets just that option, or none (H13.2).
        PropertyKind::MultiSelect => {
            if property.options.is_empty() {
                menu_caption(ui, "Esta propiedad aún no tiene opciones.");
            }
            for option in &property.options {
                if ui.button(format!("Solo «{}»", option.name)).clicked() {
                    set(Some(CellValue::MultiSelect(vec![option.id.clone()])));
                    ui.close();
                }
            }
            if ui
                .button((icons::icon(icons::CLOSE), " Quitar todas"))
                .clicked()
            {
                set(None);
                ui.close();
            }
        }
        PropertyKind::Text | PropertyKind::Number | PropertyKind::Date => {
            let hint = match property.kind {
                PropertyKind::Number => "0",
                PropertyKind::Date => "AAAA-MM-DD",
                _ => "Texto",
            };
            let field = menu_field(
                ui,
                &mut state.bulk.text,
                hint,
                &format!("Valor de «{}»", property.name),
            );
            if field.changed() {
                state.bulk.error = None;
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(error) = &state.bulk.error {
                menu_caption(ui, error);
            }
            let mut apply = enter;
            ui.horizontal(|ui| {
                apply |= ui.button("Aplicar").clicked();
                if property.kind == PropertyKind::Date
                    && let Some(today) = Date::today_utc()
                    && ui.button("Hoy").clicked()
                {
                    set(Some(CellValue::Date(today)));
                    ui.close();
                }
            });
            if apply && !state.bulk.text.trim().is_empty() {
                match parse_input(property.kind, &state.bulk.text) {
                    Ok(value) => {
                        set(value);
                        state.bulk.text.clear();
                        ui.close();
                    }
                    Err(error) => state.bulk.error = Some(error),
                }
            }
        }
    }
    // For a box, «Desmarcar» already empties it.
    if property.kind == PropertyKind::Checkbox {
        return;
    }
    ui.separator();
    if ui.button((icons::icon(icons::CLOSE), " Vaciar")).clicked() {
        set(None);
        ui.close();
    }
}

impl SilexApp {
    /// Sends the chosen rows to the trash in one transaction, as one step
    /// of Cmd+Z, with the notice that offers «Deshacer».
    pub(super) fn trash_selected(&mut self, database: &str) -> Result<()> {
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        let rows = state.chosen();
        let first = rows
            .first()
            .and_then(|id| state.rows.iter().find(|r| &r.id == id))
            .map(|r| display_title(&r.title).to_owned());
        if rows.is_empty() {
            return Ok(());
        }
        self.save_now()?;
        self.db.trash_pages(&rows)?;
        self.reload_pages()?;
        if let Some(state) = self.database.as_mut() {
            state.clear_selection();
        }
        let text = match (rows.len(), first) {
            (1, Some(title)) => format!("Fila «{title}» enviada a la papelera."),
            (n, _) => format!("{n} filas enviadas a la papelera."),
        };
        let op = TableOp::TrashRows(rows);
        self.record_table(op.clone());
        self.trash_notice = Some(TrashNotice {
            database: database.to_owned(),
            op,
            text,
        });
        Ok(())
    }

    /// Puts `value` in `property` for every chosen row, in one transaction
    /// and as one step of Cmd+Z. Cells that already have it do not count.
    pub(super) fn set_selected(&mut self, property: &str, value: Option<CellValue>) -> Result<()> {
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        let after = value.filter(|v| !v.is_empty());
        let changes: Vec<CellChange> = state
            .chosen()
            .into_iter()
            .filter_map(|row| {
                let before = state
                    .values
                    .get(&row)
                    .and_then(|c| c.get(property))
                    .cloned();
                (before != after).then(|| CellChange {
                    row,
                    property: property.to_owned(),
                    before,
                    after: after.clone(),
                })
            })
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        self.store_cells(
            changes
                .iter()
                .map(|c| (c.row.clone(), c.property.clone(), c.after.clone()))
                .collect(),
        )?;
        self.record_table(TableOp::Cells(changes));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silex_desktop::model::PageKind;

    fn page(id: &str) -> Page {
        Page {
            id: id.into(),
            parent_id: None,
            title: id.into(),
            kind: PageKind::Page,
        }
    }

    #[test]
    fn a_click_toggles_and_shift_click_adds_the_range() {
        let rows: Vec<Page> = ["a", "b", "c", "d"].into_iter().map(page).collect();
        // The view shows them as d, c, b, a.
        let order = [3, 2, 1, 0];
        let (mut selection, mut anchor) = (HashSet::new(), None);
        click_box(&rows, &order, &mut selection, &mut anchor, 0, false);
        assert_eq!(selection, HashSet::from(["d".to_owned()]));
        click_box(&rows, &order, &mut selection, &mut anchor, 2, true);
        assert_eq!(selection, HashSet::from(["d", "c", "b"].map(String::from)));
        // Again on a chosen one, without Shift: it leaves.
        click_box(&rows, &order, &mut selection, &mut anchor, 1, false);
        assert!(!selection.contains("c"));
        // Shift without an anchor in sight is a plain click.
        let mut anchor = Some("gone".to_owned());
        click_box(&rows, &order, &mut selection, &mut anchor, 3, true);
        assert!(selection.contains("a"));
        // Out of range: nothing.
        click_box(&rows, &order, &mut selection, &mut anchor, 9, false);
        assert_eq!(anchor.as_deref(), Some("a"));
    }
}
