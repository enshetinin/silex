//! Editable linked tables. Each block owns its view settings and transient UI
//! state; rows and properties always belong to the source database.
use super::*;
use anyhow::Result;
use eframe::egui;
use silex_desktop::{
    model::DatabaseView,
    property::{CellValue, Property, PropertyKind, parse_input},
    view::{Direction, Filter, FilterOp, NAME, Sort, ViewConfig},
};
use std::collections::HashSet;

impl SilexApp {
    pub(super) fn prepare_inline_tables(&mut self) -> Result<()> {
        let links: Vec<(String, String)> = self
            .blocks
            .iter()
            .filter_map(|b| {
                let target = b.link.as_ref()?;
                (b.kind == BlockKind::PageLink
                    && self
                        .pages
                        .iter()
                        .any(|p| p.id == *target && p.kind == PageKind::Database))
                .then(|| (b.id.clone(), target.clone()))
            })
            .collect();
        let live: HashSet<_> = links.iter().map(|(block, _)| block.clone()).collect();
        self.inline_tables.retain(|id, _| live.contains(id));
        for (block, target) in links {
            if self.inline_tables.contains_key(&block) {
                continue;
            }
            let previous = self.database.take();
            let loaded = self.load_database(&target);
            let state = self.database.take();
            self.database = previous;
            loaded?;
            if let Some(mut state) = state {
                let config = self.db.ensure_linked_view_config(&block, &target)?;
                state.views = vec![DatabaseView {
                    id: block.clone(),
                    name: "Vista enlazada".into(),
                    kind: silex_desktop::model::ViewKind::Table,
                    config,
                }];
                state.view = 0;
                state.linked_block = Some(block.clone());
                state.apply_view();
                self.inline_tables.insert(block, state);
            }
        }
        Ok(())
    }

    pub(super) fn refresh_inline_tables(&mut self) -> Result<()> {
        for state in self.inline_tables.values_mut() {
            let dirty: Vec<_> = state
                .rows
                .iter()
                .filter(|r| state.dirty.contains(&r.id))
                .cloned()
                .collect();
            state.rows = self.db.database_rows(&state.id)?;
            for row in dirty {
                if let Some(current) = state.rows.iter_mut().find(|r| r.id == row.id) {
                    current.title = row.title;
                }
            }
            if !state.inline_title_dirty {
                state.inline_title = self
                    .db
                    .get_page(&state.id)?
                    .map(|p| p.title)
                    .unwrap_or_default();
            }
            state.properties = self.db.database_properties(&state.id)?;
            state.values = self.db.database_values(&state.id)?;
            state.apply_view();
        }
        Ok(())
    }

    /// Flush all buffers before navigation/closing; retain invalid input on error.
    pub(super) fn save_inline_tables(&mut self) -> Result<()> {
        let ids: Vec<_> = self.inline_tables.keys().cloned().collect();
        for id in ids {
            let Some(state) = self.inline_tables.remove(&id) else {
                continue;
            };
            let previous = self.database.replace(state);
            let result = self
                .commit_inline_buffers()
                .and_then(|()| self.save_table_edits());
            if let Some(state) = self.database.take() {
                self.inline_tables.insert(id, state);
            }
            self.database = previous;
            result?;
        }
        Ok(())
    }

    pub(super) fn commit_inline_buffers(&mut self) -> Result<()> {
        if let Some(state) = self.database.as_mut()
            && state.inline_title_dirty
        {
            self.db.rename_page(&state.id, &state.inline_title)?;
            state.inline_title_dirty = false;
        }
        let edits = self
            .database
            .as_ref()
            .map(|s| s.inline_edits.clone())
            .unwrap_or_default();
        for ((row, property), text) in edits {
            self.commit_cell_edit(&CellEdit {
                row: row.clone(),
                property: property.clone(),
                text,
                focus: false,
            })?;
            if let Some(state) = self.database.as_mut() {
                state.inline_edits.remove(&(row, property));
            }
        }
        Ok(())
    }

    pub(super) fn apply_inline_actions(&mut self) {
        let mut open = None;
        let mut changed = false;
        for (block, actions, cells) in std::mem::take(&mut self.inline_actions) {
            let Some(state) = self.inline_tables.remove(&block) else {
                continue;
            };
            let id = state.id.clone();
            let previous = self.database.replace(state);
            changed |= actions
                .iter()
                .any(|a| !matches!(a, TableAction::ResizeColumn(..)))
                || !cells.is_empty();
            for cell in cells {
                if let CellAction::Commit(edit) = &cell {
                    let result = self.commit_cell_edit(edit);
                    if result.is_ok()
                        && let Some(state) = self.database.as_mut()
                    {
                        state
                            .inline_edits
                            .remove(&(edit.row.clone(), edit.property.clone()));
                    }
                    self.report(result);
                } else {
                    self.apply_cell_actions(vec![cell]);
                }
            }
            for action in actions {
                if let TableAction::Open(target) = action {
                    open = Some(target);
                } else {
                    let result = self.apply_table_action(&id, action);
                    self.report(result);
                }
            }
            let saved = self.save_view();
            self.report(saved);
            if let Some(state) = self.database.take() {
                self.inline_tables.insert(block, state);
            }
            self.database = previous;
        }
        if changed {
            let result = self
                .reload_pages()
                .and_then(|()| self.refresh_inline_tables());
            self.report(result);
        }
        if let Some(id) = open {
            let result = self.open_page(id);
            self.report(result);
        }
    }
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut DatabaseState,
    block: &str,
    selected: bool,
) -> (egui::Rect, Vec<TableAction>, Vec<CellAction>) {
    let mut actions = Vec::new();
    let mut cells = Vec::new();
    let reveal_new_row = state.focus.is_some();
    let response = ui
        .push_id(("linked-table", block), |ui| {
            let p = theme::current(ui.ctx());
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(12, 8))
                .corner_radius(theme::radius::LG)
                .fill(p.bg)
                .stroke(egui::Stroke::new(
                    1.0,
                    if selected { p.accent } else { p.border },
                ))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        icons::decoration(ui, icons::TABLE, 19.0, p.accent_strong);
                        let title_width = (ui.available_width() - 65.0).clamp(60.0, 450.0);
                        let title = ui
                            .add(
                                egui::TextEdit::singleline(&mut state.inline_title)
                                    .font(egui::FontId::new(
                                        19.0,
                                        egui::FontFamily::Name(theme::SEMIBOLD.into()),
                                    ))
                                    .hint_text("Nombre de la base de datos")
                                    .desired_width(title_width),
                            )
                            .accessible_name("Nombre de la base de datos enlazada");
                        state.inline_title_dirty |= title.changed();
                        if title.lost_focus() {
                            actions.push(TableAction::CommitInline);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let more = icons::button(
                                ui,
                                true,
                                icons::DOTS_THREE,
                                p.text_muted,
                                "Opciones de la vista",
                                None,
                            );
                            egui::Popup::menu(&more).show(|ui| {
                                ui.menu_button("Añadir propiedad", |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut state.new_property)
                                            .hint_text("Nombre de la propiedad"),
                                    )
                                    .accessible_name("Nombre de la nueva propiedad");
                                    for kind in PropertyKind::ALL {
                                        if ui.button(kind.label()).clicked() {
                                            actions.push(TableAction::AddProperty(
                                                kind,
                                                std::mem::take(&mut state.new_property),
                                            ));
                                            ui.close();
                                        }
                                    }
                                });

                                for (name, enabled, action) in [
                                    ("Deshacer", state.history.can_undo(), TableAction::Undo),
                                    ("Rehacer", state.history.can_redo(), TableAction::Redo),
                                ] {
                                    if ui.add_enabled(enabled, egui::Button::new(name)).clicked() {
                                        actions.push(action);
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button("Quitar vista de esta página").clicked() {
                                    actions.push(TableAction::RemoveLinkedView(block.to_owned()));
                                    ui.close();
                                }
                            });
                            if icons::button(
                                ui,
                                true,
                                icons::OPEN_EXTERNAL,
                                p.text_muted,
                                "Abrir base de datos",
                                None,
                            )
                            .clicked()
                            {
                                actions.push(TableAction::Open(state.id.clone()));
                            }
                        });
                    });
                    let mut config = state.config().cloned().unwrap_or_default();
                    let old = config.clone();
                    view_controls(ui, &mut config, &state.properties);
                    if config != old {
                        if let Some(view) = state.views.first_mut() {
                            view.config = config.clone();
                        }
                        state.view_changed();
                    }
                    let shown: Vec<_> = state
                        .properties
                        .iter()
                        .filter(|p| !config.is_hidden(&p.id))
                        .cloned()
                        .collect();
                    let name_width = config.column_width(NAME, 206.0);
                    let widths: Vec<_> = shown
                        .iter()
                        .map(|p| config.column_width(&p.id, column_width(p.kind) * 0.9))
                        .collect();
                    let width = name_width + widths.iter().sum::<f32>() + 28.0;
                    egui::ScrollArea::horizontal()
                        .id_salt("columns")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_min_width(width);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                header(
                                    ui,
                                    "Nombre",
                                    icons::TEXT_AA,
                                    NAME,
                                    name_width,
                                    &mut actions,
                                );
                                for (property, width) in shown.iter().zip(&widths) {
                                    header(
                                        ui,
                                        &property.name,
                                        property_icon(property.kind),
                                        &property.id,
                                        *width,
                                        &mut actions,
                                    );
                                }
                            });
                            ui.separator();
                            let row_height = 32.0 + ui.spacing().item_spacing.y;
                            let height = (state.order.len() as f32 * row_height).clamp(40.0, 320.0);
                            let mut rows_area = egui::ScrollArea::vertical()
                                .id_salt("rows")
                                .min_scrolled_height(height)
                                .max_height(height)
                                .auto_shrink([false, true]);
                            if let Some((wanted, _)) = &state.focus
                                && let Some(index) = state
                                    .order
                                    .iter()
                                    .position(|&i| state.rows[i].id == *wanted)
                            {
                                rows_area =
                                    rows_area.vertical_scroll_offset(index as f32 * row_height);
                            }
                            rows_area.show_rows(ui, 32.0, state.order.len(), |ui, range| {
                                for index in range {
                                    let row_index = state.order[index];
                                    let row = &mut state.rows[row_index];
                                    ui.push_id(row.id.clone(), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 0.0;
                                            let response = ui.add_sized(
                                                [name_width, 32.0],
                                                egui::TextEdit::singleline(&mut row.title)
                                                    .id_salt("name")
                                                    .hint_text("Sin título"),
                                            );
                                            response.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::Role::TextInput,
                                                    true,
                                                    "Nombre de la fila",
                                                )
                                            });
                                            if state
                                                .focus
                                                .as_ref()
                                                .is_some_and(|(id, _)| id == &row.id)
                                            {
                                                response.request_focus();
                                                state.focus = None;
                                            }
                                            if response.changed() {
                                                state.dirty.insert(row.id.clone());
                                            }
                                            if response.lost_focus() {
                                                actions.push(TableAction::CommitNames);
                                            }
                                            for (property, width) in shown.iter().zip(&widths) {
                                                let value = state
                                                    .values
                                                    .get(&row.id)
                                                    .and_then(|v| v.get(&property.id));
                                                inline_cell(
                                                    ui,
                                                    row,
                                                    property,
                                                    value,
                                                    *width,
                                                    &mut state.inline_edits,
                                                    &mut cells,
                                                    &mut actions,
                                                );
                                            }
                                            let (rect, _) = ui.allocate_exact_size(
                                                egui::Vec2::splat(28.0),
                                                egui::Sense::hover(),
                                            );
                                            let hovered =
                                                ui.rect_contains_pointer(egui::Rect::from_min_max(
                                                    response.rect.min,
                                                    rect.max,
                                                ));
                                            let menu = icons::more_button(
                                                ui,
                                                rect,
                                                ui.make_persistent_id("row-menu"),
                                                hovered,
                                                p.bg,
                                                &format!(
                                                    "Acciones de {}",
                                                    display_title(&row.title)
                                                ),
                                            );
                                            egui::Popup::menu(&menu).show(|ui| {
                                                if ui.button("Abrir fila").clicked() {
                                                    actions.push(TableAction::Open(row.id.clone()));
                                                    ui.close();
                                                }
                                                if ui.button("Enviar fila a la papelera").clicked()
                                                {
                                                    actions.push(TableAction::Row(
                                                        RowAction::Trash(row.id.clone()),
                                                    ));
                                                    ui.close();
                                                }
                                            });
                                        });
                                    });
                                }
                            });
                        });
                    if state.order.is_empty() {
                        ui.label(if state.rows.is_empty() {
                            "Todavía no hay filas."
                        } else {
                            "Ninguna fila coincide con los filtros."
                        });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new((icons::icon(icons::PLUS), " Nueva fila"))
                                    .frame_when_inactive(false),
                            )
                            .accessible_name("+ Nueva fila")
                            .clicked()
                        {
                            actions.push(TableAction::AddRow);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let count = state.order.len();
                            let text = if count == 1 {
                                "1 fila".into()
                            } else {
                                format!("{count} filas")
                            };
                            ui.label(egui::RichText::new(text).small().color(p.text_muted));
                        });
                    });
                })
                .response
        })
        .inner;
    if reveal_new_row {
        response.scroll_to_me(Some(egui::Align::BOTTOM));
        ui.ctx().request_repaint();
    }
    (response.rect, actions, cells)
}

fn header(
    ui: &mut egui::Ui,
    name: &str,
    icon: &str,
    id: &str,
    width: f32,
    actions: &mut Vec<TableAction>,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
    let p = theme::current(ui.ctx());
    ui.painter().rect_filled(rect, 0.0, p.subtle);
    ui.painter().vline(
        rect.right(),
        rect.y_range().shrink(7.0),
        egui::Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        icons::font(14.0),
        p.text_muted,
    );
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(rect.shrink(4.0)))
        .text(
            rect.left_center() + egui::vec2(28.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::TextStyle::Small.resolve(ui.style()),
            p.text_muted,
        );
    if let Some(width) = super::column_resize::handle(ui, rect, id, width) {
        actions.push(TableAction::ResizeColumn(id.into(), width, None));
    }
}

#[allow(clippy::too_many_arguments)]
fn inline_cell(
    ui: &mut egui::Ui,
    row: &silex_desktop::model::Page,
    property: &Property,
    value: Option<&CellValue>,
    width: f32,
    edits: &mut std::collections::HashMap<(String, String), String>,
    cells: &mut Vec<CellAction>,
    actions: &mut Vec<TableAction>,
) {
    let shown = value.map(|v| v.display(property)).unwrap_or_default();
    ui.push_id(&property.id, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(width, 32.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                match property.kind {
                    PropertyKind::Created | PropertyKind::Edited => {
                        ui.add_sized([width, 32.0], egui::Label::new(&shown).truncate());
                    }
                    PropertyKind::Checkbox => {
                        let mut on = matches!(value, Some(CellValue::Checkbox(true)));
                        if ui
                            .checkbox(&mut on, "")
                            .on_hover_text(&property.name)
                            .changed()
                        {
                            cells.push(CellAction::SetValue {
                                row: row.id.clone(),
                                property: property.id.clone(),
                                value: on.then_some(CellValue::Checkbox(true)),
                            });
                        }
                    }
                    PropertyKind::Select | PropertyKind::MultiSelect | PropertyKind::Relation => {
                        let label = if shown.is_empty() {
                            "Elegir…"
                        } else {
                            &shown
                        };
                        let (rect, button) =
                            ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::click());
                        let button = button.accessible_name(format!(
                            "{}, {}: {label}",
                            property.name,
                            display_title(&row.title)
                        ));
                        let p = theme::current(ui.ctx());
                        if button.hovered() {
                            ui.painter().rect_filled(rect, 4.0, p.subtle);
                        }
                        focus_ring(ui, rect, &button);
                        let chosen: Vec<_> = match value {
                            Some(CellValue::Select(id)) => {
                                property.option(id).into_iter().collect()
                            }
                            other => CellValue::option_ids(other)
                                .iter()
                                .filter_map(|id| property.option(id))
                                .collect(),
                        };
                        let pos = rect.left_center() + egui::vec2(8.0, 0.0);
                        if chosen.is_empty() {
                            ui.painter().text(
                                pos,
                                egui::Align2::LEFT_CENTER,
                                "Elegir…",
                                egui::TextStyle::Small.resolve(ui.style()),
                                p.text_muted,
                            );
                        } else {
                            paint_tags(ui, &ui.painter_at(rect), pos, &chosen, width - 16.0);
                        }
                        egui::Popup::menu(&button)
                            .id(ui.make_persistent_id("choose"))
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                let query_id = ui.make_persistent_id("query");
                                let mut query: String = ui
                                    .ctx()
                                    .data_mut(|d| d.get_temp(query_id).unwrap_or_default());
                                ui.add(egui::TextEdit::singleline(&mut query).hint_text("Buscar…"));
                                ui.ctx()
                                    .data_mut(|d| d.insert_temp(query_id, query.clone()));
                                let (options, exact) = property.matching_options(&query);
                                egui::ScrollArea::vertical()
                                    .max_height(220.0)
                                    .show(ui, |ui| {
                                        for option in options {
                                            ui.horizontal(|ui| {
                                                let mut on = match value {
                                                    Some(CellValue::Select(id)) => id == &option.id,
                                                    _ => CellValue::option_ids(value)
                                                        .contains(&option.id),
                                                };
                                                if ui.checkbox(&mut on, &option.name).changed() {
                                                    let new =
                                                        if property.kind == PropertyKind::Select {
                                                            on.then(|| {
                                                                CellValue::Select(option.id.clone())
                                                            })
                                                        } else {
                                                            CellValue::toggled(
                                                                property, value, &option.id,
                                                            )
                                                        };
                                                    cells.push(CellAction::SetValue {
                                                        row: row.id.clone(),
                                                        property: property.id.clone(),
                                                        value: new,
                                                    });
                                                }
                                                if property.kind == PropertyKind::Relation
                                                    && ui
                                                        .small_button("↗")
                                                        .on_hover_text("Abrir entrada relacionada")
                                                        .clicked()
                                                {
                                                    actions
                                                        .push(TableAction::Open(option.id.clone()));
                                                }
                                            });
                                        }
                                    });
                                if property.kind != PropertyKind::Relation
                                    && !query.trim().is_empty()
                                    && exact.is_none()
                                    && ui.button(format!("Crear «{}»", query.trim())).clicked()
                                {
                                    cells.push(CellAction::CreateOption {
                                        row: row.id.clone(),
                                        property: property.id.clone(),
                                        name: query.trim().into(),
                                    });
                                }
                                if ui.button("Quitar valor").clicked() {
                                    cells.push(CellAction::SetValue {
                                        row: row.id.clone(),
                                        property: property.id.clone(),
                                        value: None,
                                    });
                                    ui.close();
                                }
                            });
                    }
                    kind => {
                        let key = (row.id.clone(), property.id.clone());
                        let mut text = edits.get(&key).cloned().unwrap_or(shown);
                        let field = ui.add_sized(
                            [width, 32.0],
                            egui::TextEdit::singleline(&mut text).id_salt("value"),
                        );
                        field.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::TextInput,
                                true,
                                format!("{}, {}", property.name, display_title(&row.title)),
                            )
                        });
                        if field.changed() {
                            edits.insert(key.clone(), text.clone());
                        }
                        if field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            edits.remove(&key);
                            field.surrender_focus();
                        } else if field.lost_focus() && edits.contains_key(&key) {
                            cells.push(CellAction::Commit(CellEdit {
                                row: row.id.clone(),
                                property: property.id.clone(),
                                text: text.clone(),
                                focus: false,
                            }));
                        }
                        if edits.contains_key(&key)
                            && let Err(error) = parse_input(kind, &text)
                        {
                            field.on_hover_text(error);
                        }
                    }
                }
            },
        );
    });
}

fn view_controls(ui: &mut egui::Ui, config: &mut ViewConfig, properties: &[Property]) {
    ui.horizontal_wrapped(|ui| {
        ui.menu_button(
            if config.filters.is_empty() {
                "Filtrar".to_owned()
            } else {
                format!("Filtrar ({})", config.filters.len())
            },
            |ui| {
                let mut remove = None;
                for (index, filter) in config.filters.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            property_choice(ui, "property", &mut filter.property, properties);
                            let property = properties.iter().find(|p| p.id == filter.property);
                            let kind = property.map(|p| p.kind);
                            let ops = FilterOp::for_kind(kind);
                            if !ops.contains(&filter.op) {
                                filter.op = ops[0];
                            }
                            egui::ComboBox::from_id_salt("operator")
                                .selected_text(filter.op.label(kind))
                                .show_ui(ui, |ui| {
                                    for op in ops {
                                        ui.selectable_value(&mut filter.op, *op, op.label(kind));
                                    }
                                });
                            if ui.small_button("×").clicked() {
                                remove = Some(index);
                            }
                        });
                        if filter.op.takes_value() {
                            if let Some(property) = properties.iter().find(|p| {
                                p.id == filter.property
                                    && (p.kind.has_options() || p.kind == PropertyKind::Relation)
                            }) {
                                egui::ComboBox::from_id_salt("value")
                                    .selected_text(
                                        property
                                            .option(&filter.value)
                                            .map_or("Elegir…", |o| o.name.as_str()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for option in &property.options {
                                            ui.selectable_value(
                                                &mut filter.value,
                                                option.id.clone(),
                                                &option.name,
                                            );
                                        }
                                    });
                            } else {
                                ui.add(
                                    egui::TextEdit::singleline(&mut filter.value)
                                        .hint_text("Valor"),
                                )
                                .accessible_name(format!("Valor del filtro {}", index + 1));
                            }
                        }
                        if !filter.is_complete(properties) {
                            ui.label("Completa el filtro para aplicarlo.");
                        }
                    });
                }
                if let Some(index) = remove {
                    config.filters.remove(index);
                }
                if ui.button("+ Filtro").clicked() {
                    config.filters.push(Filter {
                        property: NAME.into(),
                        op: FilterOp::Contains,
                        value: String::new(),
                    });
                }
            },
        );
        ui.menu_button("Ordenar", |ui| {
            let mut remove = None;
            for (index, sort) in config.sorts.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    property_choice(ui, "sort", &mut sort.property, properties);
                    ui.selectable_value(&mut sort.direction, Direction::Ascending, "Ascendente");
                    ui.selectable_value(&mut sort.direction, Direction::Descending, "Descendente");
                    if ui.button("Quitar orden").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                config.sorts.remove(index);
            }
            if ui.button("+ Orden").clicked() {
                config.sorts.push(Sort {
                    property: NAME.into(),
                    direction: Direction::Ascending,
                });
            }
        });
        ui.menu_button("Columnas", |ui| {
            for property in properties {
                let mut shown = !config.is_hidden(&property.id);
                if ui.checkbox(&mut shown, &property.name).changed() {
                    config.set_hidden(&property.id, !shown);
                }
            }
        });
    });
}

fn property_choice(ui: &mut egui::Ui, salt: &str, current: &mut String, properties: &[Property]) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(
            properties
                .iter()
                .find(|p| p.id == *current)
                .map_or("Nombre", |p| p.name.as_str()),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(current, NAME.into(), "Nombre");
            for property in properties {
                ui.selectable_value(current, property.id.clone(), &property.name);
            }
        });
}
