//! A database's board view (H6.5, ADR 0006): the rows as cards, in one
//! column per option of a select property, after a column for the rows
//! without one. Dragging a card to another column sets that value (one
//! change in the table's undo); the card's context menu (or its «⋯»,
//! H11.1) does the same with «Mover a…», and a click (or Intro) opens the
//! row as a page. Supr with a card focused sends it to the trash. The view's
//! filters, sort and hidden properties apply as in the table. A column's
//! header opens its menu: the option's name and color, moving it, and
//! deleting it (its cards go to the first column).
//!
//! Only the cards in sight are drawn: the others just take their room, so
//! thousands of rows stay smooth.
use eframe::egui::{self, TextEdit};
use silex_desktop::db::Shift;
use silex_desktop::model::Page;
use silex_desktop::property::{
    CellValue, OptionColor, Property, PropertyKind, SelectOption, TableValues,
};
use silex_desktop::view::{BoardGroup, board_groups, board_property};

use super::*;
use super::{icons, theme};

const COLUMN_WIDTH: f32 = 260.0;
/// Room between the edge of a column and its cards.
const COLUMN_PAD: f32 = 6.0;
const COLUMN_HEADER: f32 = 36.0;
const CARD_PAD: f32 = 10.0;
const TITLE_HEIGHT: f32 = 22.0;
const LINE_HEIGHT: f32 = 24.0;
const CARD_GAP: f32 = 6.0;
const NEW_CARD_HEIGHT: f32 = 32.0;

/// What a dragged card carries: its row's id.
#[derive(Debug, Clone)]
struct CardDrag(String);

fn card_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-card", row))
}

/// A card's «⋯» button (H11.1).
fn card_menu_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-card-menu", row))
}

fn group_menu_id(option: &str) -> egui::Id {
    egui::Id::unique(("silex-board-column-menu", option))
}

/// A line under a card's name: a property shown on the cards and its value.
type Line<'a> = (&'a Property, &'a CellValue);

/// What a card shows under its name: the properties on the cards that
/// have a value. An unticked box shows nothing, like an empty cell.
fn card_lines<'a>(row: &str, shown: &[&'a Property], values: &'a TableValues) -> Vec<Line<'a>> {
    let Some(cells) = values.get(row) else {
        return Vec::new();
    };
    shown
        .iter()
        .filter_map(|p| {
            let value = cells.get(&p.id)?;
            (!matches!(value, CellValue::Checkbox(false))).then_some((*p, value))
        })
        .collect()
}

fn card_height(lines: usize) -> f32 {
    2.0 * CARD_PAD + TITLE_HEIGHT + lines as f32 * LINE_HEIGHT
}

/// The option of `property` that `row` has, if it is one of its options
/// (the column the row is in; `None` is the first column).
fn current_option(row: &str, property: &Property, values: &TableValues) -> Option<String> {
    match values.get(row).and_then(|cells| cells.get(&property.id)) {
        Some(CellValue::Select(id)) if property.option(id).is_some() => Some(id.clone()),
        _ => None,
    }
}

/// The name of a column: its option, or «Sin Estado».
fn group_name(property: &Property, option: Option<&str>) -> String {
    match option.and_then(|id| property.option(id)) {
        Some(option) => option.name.clone(),
        None => format!("Sin {}", property.name),
    }
}

/// Everything a board needs while it is drawn.
struct Board<'a> {
    app: &'a SilexApp,
    rows: &'a mut Vec<Page>,
    values: &'a TableValues,
    /// The property the board groups by.
    group: &'a Property,
    /// The properties shown on the cards, in order.
    shown: Vec<&'a Property>,
    dirty: &'a mut HashSet<String>,
    /// A card whose name takes the keyboard focus (a new card, «Renombrar»).
    focus: &'a mut Option<(String, usize)>,
    /// A card is being dragged.
    dragging: bool,
    /// A column's new name, typed in its menu.
    rename: &'a mut Option<(String, String)>,
    /// The column waiting for «Borrar» to be confirmed.
    delete: &'a mut Option<String>,
    /// The card or «⋯» with the keyboard focus (Cmd+Z is for the table).
    handle_focus: &'a mut Option<egui::Id>,
    actions: &'a mut Vec<TableAction>,
    cells: &'a mut Vec<CellAction>,
}

impl SilexApp {
    /// The board under the toolbar: its columns side by side, scrolling
    /// both ways, and «Nuevo grupo» at the end.
    pub(super) fn draw_board(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        selected_tab: Option<egui::Rect>,
        actions: &mut Vec<TableAction>,
        cells: &mut Vec<CellAction>,
    ) {
        let p = theme::current(ui.ctx());
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = line(ui, ui.available_width(), p.border);
        ui.spacing_mut().item_spacing.y = spacing;
        if let Some(tab) = selected_tab {
            ui.painter()
                .hline(tab.x_range(), top, egui::Stroke::new(2.0, p.accent));
        }
        ui.add_space(theme::space::MD);

        let DatabaseState {
            id,
            rows,
            views,
            view,
            properties,
            columns,
            values,
            order,
            dirty,
            focus,
            new_property,
            group_rename,
            option_delete,
            handle_focus,
            ..
        } = state;
        let Some(config) = views.get(*view).map(|v| &v.config) else {
            return;
        };
        let Some(group) = board_property(properties, config) else {
            no_group_yet(ui, actions);
            return;
        };
        let groups = board_groups(order, rows, values, group);
        let shown = columns
            .iter()
            .filter_map(|&i| properties.get(i))
            .filter(|p| p.id != group.id)
            .collect();
        let height = (ui.available_height() - theme::space::SM).max(240.0);
        let mut board = Board {
            app: self,
            rows,
            values,
            group,
            shown,
            dirty,
            focus,
            dragging: egui::DragAndDrop::has_payload_of_type::<CardDrag>(ui.ctx()),
            rename: group_rename,
            delete: option_delete,
            handle_focus,
            actions,
            cells,
        };
        visible_scroll_bars(ui);
        egui::ScrollArea::both()
            .id_salt(("silex-board", id.as_str()))
            .auto_shrink([false, false])
            .max_height(height)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = theme::space::MD;
                    // Room for the horizontal scroll bar under the columns.
                    let column_height = height - theme::space::MD;
                    for group in &groups {
                        board.column(ui, group, column_height);
                    }
                    board.new_group(ui, new_property);
                });
            });
        if board.dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        // A new name is saved when its menu closes; Escape drops it.
        if let Some((option, _)) = board.rename
            && !egui::Popup::is_id_open(ui.ctx(), group_menu_id(option))
            && let Some((option, name)) = board.rename.take()
            && !ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            board.actions.push(TableAction::RenameOption(option, name));
        }
    }
}

/// Without a select property there is nothing to group by: say so, and
/// offer to add one.
fn no_group_yet(ui: &mut egui::Ui, actions: &mut Vec<TableAction>) {
    let p = theme::current(ui.ctx());
    ui.label(
        egui::RichText::new(
            "Un tablero agrupa las filas en columnas según una propiedad de selección, \
             y esta base de datos todavía no tiene ninguna.",
        )
        .color(p.text_muted),
    );
    ui.add_space(theme::space::SM);
    if ui
        .button((
            icons::icon(property_icon(PropertyKind::Select)),
            " Añadir la propiedad «Estado»",
        ))
        .clicked()
    {
        actions.push(TableAction::AddProperty(
            PropertyKind::Select,
            "Estado".to_owned(),
        ));
    }
}

impl Board<'_> {
    /// One column: its name and count, its cards, and «Nueva» at the end.
    /// The whole column takes a dropped card.
    fn column(&mut self, ui: &mut egui::Ui, group: &BoardGroup, min_height: f32) {
        let p = theme::current(ui.ctx());
        let option = group.option.as_deref();
        let name = group_name(self.group, option);
        let layout = egui::Layout::top_down(egui::Align::Min);
        let size = egui::vec2(COLUMN_WIDTH, min_height);
        let inner = ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.set_width(COLUMN_WIDTH);
            ui.set_min_height(min_height);
            ui.spacing_mut().item_spacing.y = 0.0;
            let background = ui.painter().add(egui::Shape::Noop);

            let sense = if option.is_some() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (header, response) =
                ui.allocate_exact_size(egui::vec2(COLUMN_WIDTH, COLUMN_HEADER), sense);
            if let Some(option) = option.and_then(|id| self.group.option(id)) {
                self.column_menu(ui, &response, option, header);
            }
            let left = egui::pos2(header.left() + COLUMN_PAD + 4.0, header.center().y);
            let name_end = match option.and_then(|id| self.group.option(id)) {
                Some(option) => {
                    paint_tag_on(ui, ui.painter(), left, option, COLUMN_WIDTH - 64.0).right()
                }
                None => paint_text(
                    ui,
                    ui.painter(),
                    left,
                    &name,
                    COLUMN_WIDTH - 64.0,
                    p.text_muted,
                ),
            };
            paint_text(
                ui,
                ui.painter(),
                egui::pos2(name_end + theme::space::SM, header.center().y),
                &group.rows.len().to_string(),
                48.0,
                p.text_faint,
            );

            for &index in &group.rows {
                self.card(ui, index, option);
            }
            let new = new_card_button(ui, &name);
            if new.clicked() {
                self.actions
                    .push(TableAction::AddCard(group.option.clone()));
            }
            (background, ui.min_rect())
        });
        let (background, rect) = inner.inner;

        // A card held over the column lights it up; let go, it moves here.
        let over = self.dragging && ui.rect_contains_pointer(rect);
        let fill = if over { p.accent_soft } else { p.subtle };
        ui.painter().set(
            background,
            egui::Shape::rect_filled(rect, egui::CornerRadius::same(theme::radius::LG), fill),
        );
        if over
            && ui.input(|i| i.pointer.any_released())
            && let Some(drag) = egui::DragAndDrop::take_payload::<CardDrag>(ui.ctx())
        {
            self.move_card(&drag.0, group.option.clone());
        }
    }

    /// The menu of an option's column, opened from its header: its name,
    /// its color, moving it and deleting it (asked first).
    fn column_menu(
        &mut self,
        ui: &mut egui::Ui,
        header: &egui::Response,
        option: &SelectOption,
        rect: egui::Rect,
    ) {
        let p = theme::current(ui.ctx());
        let header = header.clone().on_hover_text("Opciones del grupo");
        header.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Grupo «{}»", option.name))
        });
        if header.hovered() || header.has_focus() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(theme::radius::LG), p.pressed);
        }
        focus_ring(ui, rect, &header);
        let menu = group_menu_id(&option.id);
        let opening = header.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
        if opening {
            *self.rename = Some((option.id.clone(), option.name.clone()));
            *self.delete = None;
        }
        let index = self
            .group
            .options
            .iter()
            .position(|o| o.id == option.id)
            .unwrap_or_default();
        let count = self.group.options.len();
        let uses = self
            .values
            .values()
            .filter_map(|cells| cells.get(&self.group.id))
            .filter(|v| matches!(v, CellValue::Select(id) if *id == option.id))
            .count();
        let empty = group_name(self.group, None);
        egui::Popup::menu(&header)
            .id(menu)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(240.0)
            .show(|ui| {
                menu_spacing(ui);
                if let Some((_, name)) = self.rename.as_mut().filter(|(id, _)| *id == option.id) {
                    let field = menu_field(ui, name, "Nombre del grupo", "Nombre del grupo");
                    if opening {
                        field.request_focus();
                    }
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ui.close();
                    }
                }
                ui.add_space(theme::space::XS);
                menu_caption(ui, "Color");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::space::SM, theme::space::SM);
                    ui.add_space(MENU_PAD);
                    for color in OptionColor::ALL {
                        let (bg, fg) = theme::option_colors(ui.ctx(), color);
                        let current = color == option.color;
                        let swatch = color_swatch(ui, bg, fg, current, color.label())
                            .on_hover_text(color.label());
                        if swatch.clicked() && !current {
                            self.actions
                                .push(TableAction::SetOptionColor(option.id.clone(), color));
                        }
                    }
                });
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
                        self.actions
                            .push(TableAction::MoveOption(option.id.clone(), shift));
                        ui.close();
                    }
                }
                ui.separator();
                if self.delete.as_deref() == Some(option.id.as_str()) {
                    let effect = match uses {
                        0 => "Ninguna fila lo usa.".to_owned(),
                        1 => format!("1 tarjeta pasará a «{empty}»."),
                        n => format!("{n} tarjetas pasarán a «{empty}»."),
                    };
                    menu_caption(ui, &format!("¿Borrar el grupo «{}»? {effect}", option.name));
                    ui.horizontal(|ui| {
                        if ui
                            .button(egui::RichText::new("Borrar").color(p.danger))
                            .on_hover_text("No se puede deshacer")
                            .clicked()
                        {
                            self.actions
                                .push(TableAction::DeleteOption(option.id.clone()));
                            *self.delete = None;
                            // Its name is not saved into a deleted option.
                            *self.rename = None;
                            ui.close();
                        }
                        if ui.button("Cancelar").clicked() {
                            *self.delete = None;
                        }
                    });
                } else if ui
                    .button((
                        icons::icon(icons::TRASH).color(p.danger),
                        egui::RichText::new(" Borrar el grupo").color(p.danger),
                    ))
                    .clicked()
                {
                    *self.delete = Some(option.id.clone());
                }
            });
    }

    /// Puts a card in the column of `option` (a cell change, undoable).
    fn move_card(&mut self, row: &str, option: Option<String>) {
        if current_option(row, self.group, self.values) == option {
            return;
        }
        self.cells.push(CellAction::SetValue {
            row: row.to_owned(),
            property: self.group.id.clone(),
            value: option.map(CellValue::Select),
        });
    }

    /// A card: its name and the values shown on cards. Out of sight, it
    /// only takes its room (unless it is held, or its name is wanted).
    fn card(&mut self, ui: &mut egui::Ui, index: usize, option: Option<&str>) {
        let p = theme::current(ui.ctx());
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let row_id = row.id.clone();
        let lines = card_lines(&row_id, &self.shown, self.values);
        let size = egui::vec2(COLUMN_WIDTH - 2.0 * COLUMN_PAD, card_height(lines.len()));
        let rect = egui::Rect::from_min_size(ui.cursor().min + egui::vec2(COLUMN_PAD, 0.0), size);
        ui.advance_cursor_after_rect(rect);
        ui.add_space(CARD_GAP);

        let id = card_id(&row_id);
        let name_id = row_name_id(&row_id);
        let wanted = self.focus.as_ref().is_some_and(|(r, _)| *r == row_id);
        let naming = wanted || ui.memory(|m| m.has_focus(name_id));
        let held = ui.ctx().is_being_dragged(id);
        if !ui.is_rect_visible(rect) && !wanted && !naming && !held {
            return;
        }

        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        response.dnd_set_drag_payload(CardDrag(row_id.clone()));
        let title = display_title(&row.title).to_owned();
        let untitled = row.title.trim().is_empty();
        response.widget_info(|| {
            let mut label = format!("Tarjeta «{title}», en {}", group_name(self.group, option));
            for (property, value) in &lines {
                label.push_str(&format!("; {}: {}", property.name, value.display(property)));
            }
            egui::WidgetInfo::labeled(egui::Role::Button, true, label)
        });
        // Under the pointer, also when it is on the card's «⋯».
        let pointed = !self.dragging && ui.rect_contains_pointer(rect);
        let card = CardLook {
            title: (!naming).then_some((title.as_str(), untitled)),
            lines: &lines,
            hovered: pointed || response.hovered() || response.has_focus(),
        };
        if held {
            // Its place stays marked; the card follows the pointer on top.
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(theme::radius::MD), p.pressed);
            if let (Some(pointer), Some(origin)) =
                ui.input(|i| (i.pointer.interact_pos(), i.pointer.press_origin()))
            {
                let layer = egui::LayerId::new(egui::Order::Tooltip, id);
                let painter = ui.ctx().layer_painter(layer);
                card.paint(ui, &painter, rect.translate(pointer - origin));
            }
        } else {
            card.paint(ui, ui.painter(), rect);
        }
        focus_ring(ui, rect, &response);

        if naming && let Some(row) = self.rows.get_mut(index) {
            let title_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(CARD_PAD, CARD_PAD),
                egui::vec2(rect.width() - 2.0 * CARD_PAD, TITLE_HEIGHT),
            );
            let ui = &mut inside(
                ui,
                title_rect,
                egui::Layout::left_to_right(egui::Align::Center),
            );
            let name = ui.add(
                TextEdit::singleline(&mut row.title)
                    .id(name_id)
                    .frame(egui::Frame::NONE)
                    .hint_text(egui::RichText::new("Sin título").color(p.text_faint))
                    .desired_width(f32::INFINITY),
            );
            if wanted {
                name.request_focus();
                *self.focus = None;
            }
            if name.gained_focus() {
                name.scroll_to_me(None);
            }
            if name.changed() {
                self.dirty.insert(row_id.clone());
            }
        }

        if response.clicked() {
            self.actions.push(TableAction::Open(row_id.clone()));
        }
        response.context_menu(|ui| self.card_menu(ui, &row_id, option));
        if response.has_focus() {
            *self.handle_focus = Some(id);
            if take_delete_key(ui.ctx()) {
                self.actions
                    .push(TableAction::Row(RowAction::Trash(row_id.clone())));
            }
        }

        // «⋯» at the top right, by the name: the same menu (H11.1).
        if naming || held {
            return;
        }
        let side = icons::BUTTON;
        let at = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - CARD_PAD / 2.0 - side,
                rect.top() + CARD_PAD + (TITLE_HEIGHT - side) / 2.0,
            ),
            egui::Vec2::splat(side),
        );
        let menu_id = card_menu_id(&row_id);
        let shown = pointed || response.has_focus();
        let menu = icons::more_button(
            ui,
            at,
            menu_id,
            shown,
            p.bg,
            &format!("Acciones de «{title}»"),
        );
        egui::Popup::menu(&menu).show(|ui| self.card_menu(ui, &row_id, option));
        if menu.has_focus() {
            *self.handle_focus = Some(menu_id);
            if take_delete_key(ui.ctx()) {
                self.actions
                    .push(TableAction::Row(RowAction::Trash(row_id.clone())));
            }
        }
    }

    /// A card's menu: the other columns it can move to, then the usual
    /// page menu (rename, trash…).
    fn card_menu(&mut self, ui: &mut egui::Ui, row: &str, option: Option<&str>) {
        menu_spacing(ui);
        menu_caption(ui, "Mover a");
        let targets: Vec<Option<String>> = std::iter::once(None)
            .chain(self.group.options.iter().map(|o| Some(o.id.clone())))
            .filter(|target| target.as_deref() != option)
            .collect();
        for target in targets {
            let name = group_name(self.group, target.as_deref());
            if ui
                .button((icons::icon(icons::ARROW_RIGHT), format!(" {name}")))
                .clicked()
            {
                self.move_card(row, target);
                ui.close();
            }
        }
        if let Some(chosen) = self.app.page_menu(ui, row) {
            self.actions.push(TableAction::Row(chosen));
            ui.close();
        }
    }

    /// «Nuevo grupo» after the last column: a new option of the property
    /// the board groups by, named in its menu.
    fn new_group(&mut self, ui: &mut egui::Ui, name: &mut String) {
        let p = theme::current(ui.ctx());
        let button = ui
            .add(
                egui::Button::new((
                    icons::icon(icons::PLUS).color(p.text_muted),
                    egui::RichText::new(" Nuevo grupo").color(p.text_muted),
                ))
                .frame_when_inactive(false)
                .min_size(egui::vec2(0.0, COLUMN_HEADER)),
            )
            .on_hover_text(format!("Una opción nueva de «{}»", self.group.name))
            .accessible_name("Nuevo grupo");
        let menu = egui::Id::unique("silex-new-group-menu");
        let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
        if opening {
            name.clear();
        }
        let property = self.group.id.clone();
        egui::Popup::menu(&button)
            .id(menu)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(220.0)
            .show(|ui| {
                menu_spacing(ui);
                let field = menu_field(ui, name, "Nombre del grupo", "Nombre del grupo nuevo");
                if opening {
                    field.request_focus();
                }
                ui.add_space(theme::space::XS);
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = ui
                    .add_enabled(!name.trim().is_empty(), egui::Button::new("Añadir"))
                    .clicked();
                if (enter || add) && !name.trim().is_empty() {
                    self.actions
                        .push(TableAction::AddOption(property, std::mem::take(name)));
                    ui.close();
                }
            });
    }
}

/// How a card looks: its name (`None` while it is being typed in a
/// field), its lines, and whether it is hovered.
struct CardLook<'a> {
    /// The name shown and whether it is the «Sin título» placeholder.
    title: Option<(&'a str, bool)>,
    lines: &'a [Line<'a>],
    hovered: bool,
}

impl CardLook<'_> {
    fn paint(&self, ui: &egui::Ui, painter: &egui::Painter, rect: egui::Rect) {
        let p = theme::current(ui.ctx());
        let radius = egui::CornerRadius::same(theme::radius::MD);
        let stroke = if self.hovered { p.text_faint } else { p.border };
        painter.rect_filled(rect, radius, p.bg);
        painter.rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let left = rect.left() + CARD_PAD;
        let width = rect.width() - 2.0 * CARD_PAD;
        if let Some((title, untitled)) = self.title {
            let color = if untitled { p.text_faint } else { p.text };
            let y = rect.top() + CARD_PAD + TITLE_HEIGHT / 2.0;
            paint_text(ui, painter, egui::pos2(left, y), title, width, color);
        }
        let mut y = rect.top() + CARD_PAD + TITLE_HEIGHT + LINE_HEIGHT / 2.0;
        for (property, value) in self.lines {
            let at = egui::pos2(left, y);
            match value {
                CellValue::Select(id) => {
                    if let Some(option) = property.option(id) {
                        paint_tag_on(ui, painter, at, option, width);
                    }
                }
                CellValue::Checkbox(_) => {
                    let icon = painter.text(
                        at,
                        egui::Align2::LEFT_CENTER,
                        icons::TODO,
                        icons::font(theme::icon::MD),
                        p.accent,
                    );
                    let at = egui::pos2(icon.right() + theme::space::XS, y);
                    let room = width - (icon.width() + theme::space::XS);
                    paint_text(ui, painter, at, &property.name, room, p.text_muted);
                }
                other => {
                    paint_text(
                        ui,
                        painter,
                        at,
                        &other.display(property),
                        width,
                        p.text_muted,
                    );
                }
            }
            y += LINE_HEIGHT;
        }
    }
}

/// One line of text from `left_center`, cut at `max_width`. Returns where
/// it ends.
fn paint_text(
    ui: &egui::Ui,
    painter: &egui::Painter,
    left_center: egui::Pos2,
    text: &str,
    max_width: f32,
    color: egui::Color32,
) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        max_width.max(0.0),
        egui::TextStyle::Body,
    );
    let at = egui::pos2(left_center.x, left_center.y - galley.size().y / 2.0);
    let end = at.x + galley.size().x;
    painter.galley(at, galley, color);
    end
}

/// «Nueva» at the end of a column: a new card in it.
fn new_card_button(ui: &mut egui::Ui, column: &str) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(COLUMN_WIDTH, NEW_CARD_HEIGHT),
        egui::Sense::click(),
    );
    let rect = rect.shrink2(egui::vec2(COLUMN_PAD, 0.0));
    let name = format!("Nueva tarjeta en «{column}»");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.pressed);
    }
    let icon = ui.painter().text(
        egui::pos2(rect.left() + CARD_PAD - COLUMN_PAD + 4.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icons::PLUS,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    ui.painter().text(
        egui::pos2(icon.right() + theme::space::XS, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Nueva",
        egui::TextStyle::Body.resolve(ui.style()),
        p.text_muted,
    );
    focus_ring(ui, rect, &response);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use silex_desktop::property::{OptionColor, SelectOption};

    fn estado() -> Property {
        Property {
            id: "estado".into(),
            name: "Estado".into(),
            kind: PropertyKind::Select,
            options: vec![SelectOption {
                id: "hecho".into(),
                name: "Hecho".into(),
                color: OptionColor::Green,
            }],
        }
    }

    #[test]
    fn cards_show_only_the_values_they_have() {
        let estado = estado();
        let leido = Property {
            id: "leido".into(),
            name: "Leído".into(),
            kind: PropertyKind::Checkbox,
            options: Vec::new(),
        };
        let paginas = Property {
            id: "paginas".into(),
            name: "Páginas".into(),
            kind: PropertyKind::Number,
            options: Vec::new(),
        };
        let mut values = TableValues::new();
        let cells = values.entry("a".into()).or_default();
        cells.insert("leido".into(), CellValue::Checkbox(false));
        cells.insert("paginas".into(), CellValue::Number(12.0));
        cells.insert("estado".into(), CellValue::Select("perdida".into()));
        let shown = [&leido, &paginas];
        let lines = card_lines("a", &shown, &values);
        assert_eq!(lines.len(), 1, "an unticked box shows nothing");
        assert_eq!(lines[0].0.id, "paginas");
        assert!(card_lines("b", &shown, &values).is_empty());
        assert!(card_height(1) > card_height(0));

        // An option that is not one of the property's: no column of its own.
        assert_eq!(current_option("a", &estado, &values), None);
        values
            .entry("a".into())
            .or_default()
            .insert("estado".into(), CellValue::Select("hecho".into()));
        assert_eq!(
            current_option("a", &estado, &values).as_deref(),
            Some("hecho")
        );
        assert_eq!(group_name(&estado, Some("hecho")), "Hecho");
        assert_eq!(group_name(&estado, None), "Sin Estado");
    }
}
