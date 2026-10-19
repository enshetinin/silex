//! The date cell (H6.6): a click opens a calendar under the cell, with a
//! field to type the date (`AAAA-MM-DD` or `D/M/AAAA`) above the month.
//! A click on a day, «Hoy» or Intro in the field saves it; «Quitar la
//! fecha» empties the cell. No dependency: the calendar's arithmetic is
//! in `property.rs` (`Date`, `CalendarMonth`).
//!
//! «Hoy» comes from the system clock in UTC: between midnight and the UTC
//! offset it can still be yesterday (the standard library has no local
//! time zone).
use eframe::egui;
use silex_desktop::property::{CalendarMonth, CellValue, Date, spoken_date};

use super::*;
use super::{icons, theme};

/// Size of a day in the grid, and the room between days.
const DAY: egui::Vec2 = egui::vec2(32.0, 28.0);
const DAY_GAP: f32 = 2.0;
/// Room for seven days and the menu's padding.
const MENU_WIDTH: f32 = 7.0 * 32.0 + 6.0 * DAY_GAP + 2.0 * MENU_PAD;

const WEEKDAYS: [&str; 7] = ["L", "M", "X", "J", "V", "S", "D"];

pub(super) fn date_menu_id(row: &str, property: &str) -> egui::Id {
    egui::Id::unique(("silex-date-menu", row, property))
}

/// A date cell: its value, and the calendar while its menu is open.
/// `typed` is the text of the menu's field (shared with the select menu:
/// only one cell menu is open at a time).
pub(super) fn date_cell(
    ui: &mut egui::Ui,
    cell: &Cell<'_>,
    editing: &mut Option<CellEdit>,
    typed: &mut String,
    actions: &mut Vec<CellAction>,
) {
    let p = theme::current(ui.ctx());
    let current = match cell.value {
        Some(CellValue::Date(date)) => Some(*date),
        _ => None,
    };
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
        paint_cell_text(ui, cell.rect, &shown, egui::Align2::LEFT_CENTER, p.text);
    }

    let menu = date_menu_id(&cell.row.id, &cell.property.id);
    let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), menu);
    let today = Date::today_utc();
    if opening {
        *typed = shown;
        if let Some(old) = editing.take() {
            actions.push(CellAction::Commit(old));
        }
        // The calendar opens on the cell's month, else on this month.
        if let Some(month) = current.or(today).map(CalendarMonth::of) {
            ui.data_mut(|d| d.insert_temp(menu, month));
        }
    }
    let set = |value: Option<Date>| CellAction::SetValue {
        row: cell.row.id.clone(),
        property: cell.property.id.clone(),
        value: value.map(CellValue::Date),
    };
    egui::Popup::menu(&button)
        .id(menu)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(MENU_WIDTH)
        .show(|ui| {
            menu_spacing(ui);
            let field = menu_field(
                ui,
                typed,
                "AAAA-MM-DD o D/M/AAAA",
                &format!("Fecha de {}", cell.name()),
            );
            if opening {
                field.request_focus();
            }
            let parsed = Date::parse(typed);
            // The calendar follows a date being typed.
            if field.changed()
                && let Some(date) = parsed
            {
                ui.data_mut(|d| d.insert_temp(menu, CalendarMonth::of(date)));
            }
            let mut chosen = None;
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter {
                if typed.trim().is_empty() {
                    chosen = Some(None);
                } else if parsed.is_some() {
                    chosen = Some(parsed);
                }
            }
            if !typed.trim().is_empty() && parsed.is_none() {
                menu_caption(ui, "Escribe una fecha como 2026-10-19 o 19/10/2026.");
            }
            ui.add_space(theme::space::XS);

            let month = ui
                .data(|d| d.get_temp::<CalendarMonth>(menu))
                .or(today.map(CalendarMonth::of));
            if let Some(month) = month {
                let marked = parsed.or(current);
                if let Some(day) = calendar(ui, menu, month, marked, today) {
                    chosen = Some(Some(day));
                }
            }

            ui.separator();
            ui.horizontal(|ui| {
                if let Some(today) = today
                    && ui
                        .button((icons::icon(icons::CALENDAR), " Hoy"))
                        .on_hover_text(spoken_date(today))
                        .clicked()
                {
                    chosen = Some(Some(today));
                }
                if current.is_some()
                    && ui
                        .button((icons::icon(icons::CLOSE), " Quitar la fecha"))
                        .clicked()
                {
                    chosen = Some(None);
                }
            });
            if let Some(value) = chosen {
                if value != current {
                    actions.push(set(value));
                }
                ui.close();
            }
        });
}

/// The month's name between «‹» and «›», the days of the week and the
/// grid. `marked` is the date in the cell (or being typed). Returns the
/// day clicked.
fn calendar(
    ui: &mut egui::Ui,
    menu: egui::Id,
    month: CalendarMonth,
    marked: Option<Date>,
    today: Option<Date>,
) -> Option<Date> {
    let p = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(MENU_PAD, 2.0);
        let previous = ui
            .button(icons::icon(icons::CARET_LEFT))
            .on_hover_text("Mes anterior")
            .accessible_name("Mes anterior");
        if previous.clicked() {
            ui.data_mut(|d| d.insert_temp(menu, month.previous()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let next = ui
                .button(icons::icon(icons::CARET_RIGHT))
                .on_hover_text("Mes siguiente")
                .accessible_name("Mes siguiente");
            if next.clicked() {
                ui.data_mut(|d| d.insert_temp(menu, month.next()));
            }
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(month.label()).strong().color(p.text));
            });
        });
    });

    let mut chosen = None;
    let spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(DAY_GAP, DAY_GAP);
    ui.horizontal(|ui| {
        for name in WEEKDAYS {
            let (rect, _) = ui.allocate_exact_size(DAY, egui::Sense::hover());
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                name,
                egui::TextStyle::Small.resolve(ui.style()),
                p.text_faint,
            );
        }
    });
    for week in month.weeks() {
        ui.horizontal(|ui| {
            for day in week {
                let Some(day) = day else {
                    ui.allocate_exact_size(DAY, egui::Sense::hover());
                    continue;
                };
                if day_button(ui, day, marked == Some(day), today == Some(day)).clicked() {
                    chosen = Some(day);
                }
            }
        });
    }
    ui.spacing_mut().item_spacing = spacing;
    chosen
}

/// A day of the grid: filled with the accent when it is the cell's date,
/// outlined when it is today. Named by its full date for screen readers.
fn day_button(ui: &mut egui::Ui, day: Date, marked: bool, today: bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(DAY, egui::Sense::click());
    let mut name = spoken_date(day);
    if today {
        name.push_str(", hoy");
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, marked, &name));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    let text = if marked {
        ui.painter().rect_filled(rect, radius, p.accent);
        p.on_accent
    } else {
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, radius, p.subtle);
        }
        if today { p.accent_strong } else { p.text }
    };
    if today && !marked {
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        day.day().to_string(),
        egui::TextStyle::Body.resolve(ui.style()),
        text,
    );
    focus_ring(ui, rect, &response);
    response
}
