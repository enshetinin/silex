//! A database's calendar view (H13.5, ADR 0006): a month, Monday first,
//! with each row in sight on the day of a date property, as a label with
//! its name. A click (or Intro) opens the row as a page; dragging it to
//! another day sets that date, and to «Sin fecha» empties it (one change
//! in the table's undo). «+» on a day adds a row with that date and opens
//! it. The view's filters and sort apply as in the table: within a day,
//! rows keep the view's order. Days with more rows than fit show «+N más»,
//! a menu with all of them.
use eframe::egui;
use silex_desktop::model::Page;
use silex_desktop::property::{
    CalendarMonth, CellValue, Date, Property, PropertyKind, TableValues, spoken_date,
};
use silex_desktop::view::{calendar_property, calendar_rows};

use super::board::paint_text;
use super::*;
use super::{icons, theme};

const WEEKDAYS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const WEEKDAY_HEIGHT: f32 = 28.0;
const DAY_MIN_HEIGHT: f32 = 64.0;
/// The top of a day: its number and «+».
const DAY_TOP: f32 = 26.0;
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_GAP: f32 = 2.0;
const PAD: f32 = 4.0;
const UNDATED_CHIP_WIDTH: f32 = 200.0;
/// Room kept under the month for «Sin fecha» when it has rows.
const UNDATED_ROOM: f32 = 76.0;

/// What a dragged label carries: its row's id.
#[derive(Debug, Clone)]
struct ChipDrag(String);

pub(super) fn chip_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-calendar-chip", row))
}

pub(super) fn add_on_day_id(date: Date) -> egui::Id {
    egui::Id::unique(("silex-calendar-add", date))
}

fn more_id(date: Date) -> egui::Id {
    egui::Id::unique(("silex-calendar-more", date))
}

/// How many labels a day with `count` rows shows when `capacity` fit, and
/// how many go in «+N más». Never «+1 más»: that one fits instead.
pub(super) fn chips_that_fit(count: usize, capacity: usize) -> (usize, usize) {
    let capacity = capacity.max(1);
    if count <= capacity {
        (count, 0)
    } else {
        let shown = capacity - 1;
        (shown, count - shown)
    }
}

/// «Noviembre de 2026».
fn month_title(month: CalendarMonth) -> String {
    let label = month.label();
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => label,
    }
}

/// Everything the month needs while it is drawn.
struct Calendar<'a> {
    app: &'a SilexApp,
    rows: &'a [Page],
    values: &'a TableValues,
    property: &'a Property,
    today: Option<Date>,
    /// The row of the label being dragged.
    dragging: Option<String>,
    /// The label with the keyboard focus (Cmd+Z is for the table).
    handle_focus: &'a mut Option<egui::Id>,
    actions: &'a mut Vec<TableAction>,
    cells: &'a mut Vec<CellAction>,
}

impl SilexApp {
    /// Today in local time, for the calendar (UTC if it cannot be read).
    fn calendar_today(&self) -> Option<Date> {
        self.db
            .local_today()
            .ok()
            .and_then(|today| Date::parse(&today))
            .or_else(Date::today_utc)
    }

    /// The calendar under the toolbar: the month's name and arrows, the
    /// days, and the rows without a date.
    pub(super) fn draw_calendar(
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
        ui.add_space(theme::space::SM);

        let DatabaseState {
            id,
            rows,
            views,
            view,
            properties,
            values,
            order,
            month,
            today,
            handle_focus,
            ..
        } = state;
        let Some(config) = views.get(*view).map(|v| &v.config) else {
            return;
        };
        let Some(property) = calendar_property(properties, config) else {
            no_date_yet(ui, actions);
            return;
        };
        if month.is_none() {
            *today = self.calendar_today();
            *month = today.map(CalendarMonth::of);
        }
        let Some(shown) = *month else {
            return;
        };

        // ‹ › «Noviembre de 2026» Hoy
        ui.horizontal(|ui| {
            if icons::button(
                ui,
                true,
                icons::CARET_LEFT,
                p.text_muted,
                "Mes anterior",
                None,
            )
            .clicked()
            {
                *month = Some(shown.previous());
            }
            if icons::button(
                ui,
                true,
                icons::CARET_RIGHT,
                p.text_muted,
                "Mes siguiente",
                None,
            )
            .clicked()
            {
                *month = Some(shown.next());
            }
            ui.add_space(theme::space::XS);
            ui.label(
                egui::RichText::new(month_title(shown))
                    .text_style(egui::TextStyle::Name(theme::text::H3.into()))
                    .color(p.text),
            );
            ui.add_space(theme::space::SM);
            let back = ui.button("Hoy");
            let back = match *today {
                Some(day) => back.on_hover_text(spoken_date(day)),
                None => back,
            };
            if back.clicked() {
                *today = self.calendar_today();
                if let Some(day) = *today {
                    *month = Some(CalendarMonth::of(day));
                }
            }
        });
        ui.add_space(theme::space::SM);

        let by_day = calendar_rows(order, rows, values, property);
        let weeks = shown.weeks();
        let dragging = egui::DragAndDrop::payload::<ChipDrag>(ui.ctx()).map(|d| d.0.clone());
        let mut calendar = Calendar {
            app: self,
            rows,
            values,
            property,
            today: *today,
            dragging,
            handle_focus,
            actions,
            cells,
        };
        let undated_room = if by_day.undated.is_empty() {
            0.0
        } else {
            UNDATED_ROOM
        };
        let room = ui.available_height() - WEEKDAY_HEIGHT - undated_room - theme::space::SM;
        let weeks_count = weeks.len().max(1) as f32;
        let day_height = (room / weeks_count).floor().max(DAY_MIN_HEIGHT);
        visible_scroll_bars(ui);
        egui::ScrollArea::vertical()
            .id_salt(("silex-calendar", id.as_str()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let day_width = (ui.available_width() / 7.0).floor();
                let (head, _) = ui.allocate_exact_size(
                    egui::vec2(7.0 * day_width, WEEKDAY_HEIGHT),
                    egui::Sense::hover(),
                );
                for (i, name) in WEEKDAYS.iter().enumerate() {
                    ui.painter().text(
                        egui::pos2(
                            head.left() + i as f32 * day_width + PAD + 2.0,
                            head.center().y,
                        ),
                        egui::Align2::LEFT_CENTER,
                        *name,
                        egui::TextStyle::Small.resolve(ui.style()),
                        p.text_muted,
                    );
                }
                let (grid, _) = ui.allocate_exact_size(
                    egui::vec2(7.0 * day_width, weeks_count * day_height),
                    egui::Sense::hover(),
                );
                for (w, week) in weeks.iter().enumerate() {
                    for (d, day) in week.iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            grid.min + egui::vec2(d as f32 * day_width, w as f32 * day_height),
                            egui::vec2(day_width, day_height),
                        );
                        let in_day = day
                            .and_then(|date| by_day.days.get(&date))
                            .map_or(&[][..], Vec::as_slice);
                        calendar.day(ui, rect, *day, in_day);
                    }
                }
                calendar.undated(ui, &by_day.undated);
            });
        if calendar.dragging.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    /// «+» on a day (H13.5): a new row with that date in the calendar's
    /// property, opened as a page to name it. One undo step for the row,
    /// another for its date, as a new card on a board.
    pub(super) fn add_on_date(&mut self, database: &str, date: Date) -> Result<()> {
        let property = self
            .database
            .as_ref()
            .and_then(|d| calendar_property(&d.properties, d.config()?))
            .map(|p| p.id.clone())
            .ok_or_else(|| anyhow::anyhow!("El calendario no tiene una propiedad de fecha"))?;
        let row = self.insert_row(database)?;
        self.store_cell(&row, &property, Some(CellValue::Date(date)))?;
        self.open_page(row)
    }
}

/// Without a date property there is nothing to put the rows by: say so,
/// and offer to add one.
fn no_date_yet(ui: &mut egui::Ui, actions: &mut Vec<TableAction>) {
    let p = theme::current(ui.ctx());
    ui.label(
        egui::RichText::new(
            "Un calendario pone las filas en los días según una propiedad de fecha, \
             y esta base de datos todavía no tiene ninguna.",
        )
        .color(p.text_muted),
    );
    ui.add_space(theme::space::SM);
    if ui
        .button((
            icons::icon(property_icon(PropertyKind::Date)),
            " Añadir la propiedad «Fecha»",
        ))
        .clicked()
    {
        actions.push(TableAction::AddProperty(
            PropertyKind::Date,
            "Fecha".to_owned(),
        ));
    }
}

impl Calendar<'_> {
    /// One day of the grid: its number, «+», its rows and «+N más». It
    /// takes a dropped label. Days of other months are blank.
    fn day(&mut self, ui: &mut egui::Ui, rect: egui::Rect, date: Option<Date>, rows: &[usize]) {
        let p = theme::current(ui.ctx());
        let border = egui::Stroke::new(1.0, p.border);
        let Some(date) = date else {
            ui.painter().rect_filled(rect, 0.0, p.subtle);
            // On the edge: next days share one line.
            ui.painter()
                .rect_stroke(rect, 0.0, border, egui::StrokeKind::Middle);
            return;
        };
        let over = self.dragging.is_some() && ui.rect_contains_pointer(rect);
        let fill = if over { p.accent_soft } else { p.bg };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter()
            .rect_stroke(rect, 0.0, border, egui::StrokeKind::Middle);

        // The day's number; today's on an accent circle.
        let number_at = egui::pos2(rect.left() + PAD + 11.0, rect.top() + DAY_TOP / 2.0);
        let font = egui::TextStyle::Small.resolve(ui.style());
        if Some(date) == self.today {
            ui.painter().circle_filled(number_at, 11.0, p.accent);
            ui.painter().text(
                number_at,
                egui::Align2::CENTER_CENTER,
                date.day().to_string(),
                font,
                p.on_accent,
            );
        } else {
            ui.painter().text(
                number_at,
                egui::Align2::CENTER_CENTER,
                date.day().to_string(),
                font,
                p.text_muted,
            );
        }

        // «+» at the top right: always there for Tab, seen under the pointer.
        let side = 24.0;
        let add = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - PAD - side,
                rect.top() + (DAY_TOP - side) / 2.0,
            ),
            egui::Vec2::splat(side),
        );
        let pointed = self.dragging.is_none() && ui.rect_contains_pointer(rect);
        let name = format!("Nueva fila el {}", spoken_date(date));
        if add_button(ui, add, add_on_day_id(date), pointed, &name).clicked() {
            self.actions.push(TableAction::AddOnDate(date));
        }

        let room = rect.height() - DAY_TOP - PAD;
        let capacity = (room / (CHIP_HEIGHT + CHIP_GAP)).floor().max(1.0) as usize;
        let (shown, more) = chips_that_fit(rows.len(), capacity);
        let width = rect.width() - 2.0 * PAD;
        let mut y = rect.top() + DAY_TOP;
        for &index in &rows[..shown] {
            let chip = egui::Rect::from_min_size(
                egui::pos2(rect.left() + PAD, y),
                egui::vec2(width, CHIP_HEIGHT),
            );
            self.chip(ui, chip, index, Some(date));
            y += CHIP_HEIGHT + CHIP_GAP;
        }
        if more > 0 {
            let at = egui::Rect::from_min_size(
                egui::pos2(rect.left() + PAD, y),
                egui::vec2(width, CHIP_HEIGHT),
            );
            self.more(ui, at, date, rows, more);
        }

        if over
            && ui.input(|i| i.pointer.any_released())
            && let Some(drag) = egui::DragAndDrop::take_payload::<ChipDrag>(ui.ctx())
        {
            self.set_date(&drag.0, Some(date));
        }
    }

    /// «+N más» on a full day: a menu with every row of the day.
    fn more(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        date: Date,
        rows: &[usize],
        more: usize,
    ) {
        let p = theme::current(ui.ctx());
        let response = ui.interact(rect, more_id(date), egui::Sense::click());
        let label = format!("+{more} más");
        let name = format!("{label} el {}", spoken_date(date));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.subtle);
        }
        paint_text(
            ui,
            ui.painter(),
            egui::pos2(rect.left() + 6.0, rect.center().y),
            &label,
            rect.width() - 12.0,
            p.text_muted,
        );
        focus_ring(ui, rect, &response);
        egui::Popup::menu(&response).width(240.0).show(|ui| {
            menu_spacing(ui);
            menu_caption(ui, &spoken_date(date));
            for &index in rows {
                let Some(row) = self.rows.get(index) else {
                    continue;
                };
                if ui
                    .button((
                        icons::icon(icons::PAGE),
                        format!(" {}", display_title(&row.title)),
                    ))
                    .clicked()
                {
                    self.actions.push(TableAction::Open(row.id.clone()));
                    ui.close();
                }
            }
        });
    }

    /// The rows without a date, under the month; while a label is
    /// dragged, also a place to drop it and empty its date.
    fn undated(&mut self, ui: &mut egui::Ui, rows: &[usize]) {
        if rows.is_empty() && self.dragging.is_none() {
            return;
        }
        let p = theme::current(ui.ctx());
        ui.add_space(theme::space::MD);
        let background = ui.painter().add(egui::Shape::Noop);
        let inner = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(theme::space::XS, theme::space::XS);
            let title = match rows.len() {
                0 => "Sin fecha".to_owned(),
                n => format!("Sin fecha · {n}"),
            };
            ui.label(egui::RichText::new(title).small().color(p.text_muted));
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new("Suéltala aquí para quitarle la fecha.")
                        .color(p.text_faint),
                );
            }
            ui.horizontal_wrapped(|ui| {
                for &index in rows {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(UNDATED_CHIP_WIDTH, CHIP_HEIGHT),
                        egui::Sense::hover(),
                    );
                    self.chip(ui, rect, index, None);
                }
            });
        });
        let rect = inner.response.rect.expand(PAD);
        let over = self.dragging.is_some() && ui.rect_contains_pointer(rect);
        if over {
            ui.painter().set(
                background,
                egui::Shape::rect_filled(
                    rect,
                    egui::CornerRadius::same(theme::radius::MD),
                    p.accent_soft,
                ),
            );
        }
        if over
            && ui.input(|i| i.pointer.any_released())
            && let Some(drag) = egui::DragAndDrop::take_payload::<ChipDrag>(ui.ctx())
        {
            self.set_date(&drag.0, None);
        }
    }

    /// A row's label: its name. A click opens it, it can be dragged, and
    /// its context menu is the page's.
    fn chip(&mut self, ui: &mut egui::Ui, rect: egui::Rect, index: usize, date: Option<Date>) {
        let p = theme::current(ui.ctx());
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let row_id = row.id.clone();
        let id = chip_id(&row_id);
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        response.dnd_set_drag_payload(ChipDrag(row_id.clone()));
        let title = display_title(&row.title).to_owned();
        let untitled = row.title.trim().is_empty();
        let label = match date {
            Some(date) => format!("«{title}», {}", spoken_date(date)),
            None => format!("«{title}», sin fecha"),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        let held = self.dragging.as_deref() == Some(row_id.as_str());
        let radius = egui::CornerRadius::same(theme::radius::SM);
        let look = |painter: &egui::Painter, rect: egui::Rect, hovered: bool| {
            let fill = if hovered { p.pressed } else { p.subtle };
            painter.rect_filled(rect, radius, fill);
            let color = if untitled { p.text_faint } else { p.text };
            paint_text(
                ui,
                painter,
                egui::pos2(rect.left() + 6.0, rect.center().y),
                &title,
                rect.width() - 12.0,
                color,
            );
        };
        if held {
            // Its place stays marked; the label follows the pointer on top.
            ui.painter().rect_stroke(
                rect,
                radius,
                egui::Stroke::new(1.0, p.border_strong),
                egui::StrokeKind::Inside,
            );
            if let (Some(pointer), Some(origin)) =
                ui.input(|i| (i.pointer.interact_pos(), i.pointer.press_origin()))
            {
                let layer = egui::LayerId::new(egui::Order::Tooltip, id);
                let painter = ui.ctx().layer_painter(layer);
                look(&painter, rect.translate(pointer - origin), true);
            }
        } else {
            look(
                ui.painter(),
                rect,
                response.hovered() || response.has_focus(),
            );
        }
        focus_ring(ui, rect, &response);

        if response.clicked() {
            self.actions.push(TableAction::Open(row_id.clone()));
        }
        response.context_menu(|ui| {
            if let Some(chosen) = self.app.page_menu(ui, &row_id) {
                self.actions.push(TableAction::Row(chosen));
                ui.close();
            }
        });
        if response.has_focus() {
            *self.handle_focus = Some(id);
            if take_delete_key(ui.ctx()) {
                self.actions
                    .push(TableAction::Row(RowAction::Trash(row_id.clone())));
            }
        }
    }

    /// Gives `row` the date `date` (or none), if it is not that already.
    fn set_date(&mut self, row: &str, date: Option<Date>) {
        let current = match self
            .values
            .get(row)
            .and_then(|cells| cells.get(&self.property.id))
        {
            Some(CellValue::Date(d)) => Some(*d),
            _ => None,
        };
        if current == date {
            return;
        }
        self.cells.push(CellAction::SetValue {
            row: row.to_owned(),
            property: self.property.id.clone(),
            value: date.map(CellValue::Date),
        });
    }
}

/// «+» on a day: always there, so Tab reaches it, but seen only while
/// `shown` (the day is under the pointer), on hover or with the focus.
fn add_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    shown: bool,
    name: &str,
) -> egui::Response {
    let p = theme::current(ui.ctx());
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let active = response.hovered() || response.has_focus();
    if (shown || active) && ui.is_rect_visible(rect) {
        let radius = egui::CornerRadius::same(theme::radius::SM);
        if active {
            ui.painter().rect_filled(rect, radius, p.pressed);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::PLUS,
            icons::font(theme::icon::MD),
            p.text_muted,
        );
    }
    focus_ring(ui, rect, &response);
    response.on_hover_text(name).accessible_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_day_shows_what_fits_and_counts_the_rest() {
        assert_eq!(chips_that_fit(0, 3), (0, 0));
        assert_eq!(chips_that_fit(3, 3), (3, 0), "no «+1 más»");
        assert_eq!(chips_that_fit(4, 3), (2, 2));
        assert_eq!(chips_that_fit(10, 3), (2, 8));
        assert_eq!(chips_that_fit(2, 0), (0, 2), "at least «+N más»");
    }

    #[test]
    fn the_month_title_starts_with_a_capital() {
        let month = CalendarMonth::of(Date::parse("2026-11-04").expect("date"));
        assert_eq!(month_title(month), "Noviembre de 2026");
    }
}
