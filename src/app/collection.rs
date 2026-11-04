//! A database's list and gallery views (H13.6, ADR 0006). The list has a
//! line per row: its icon and name, and the properties on show on the
//! right. The gallery has a card per row, like a board's, with the row's
//! first image (H7.2) as its cover when any row has one. In both, a click
//! (or Intro) opens the row as a page, the context menu (or «⋯») is the
//! page's, Supr sends it to the trash, and «Nueva» adds a row and opens
//! it. The view's filters, sort and hidden properties apply as in the
//! table. Only the lines or cards in sight are drawn.
use std::collections::HashMap;

use eframe::egui;
use silex_desktop::property::{CellValue, Property, TableValues};

use super::board::{
    CARD_PAD, CardLook, LINE_HEIGHT, TITLE_HEIGHT, card_lines, paint_text, paint_value,
};
use super::*;
use super::{icons, theme};

const LIST_ROW: f32 = 36.0;
/// Room for each property on a list's line.
const VALUE_WIDTH: f32 = 160.0;
/// The name keeps at least this much of a line.
const LIST_NAME_MIN: f32 = 160.0;
const CARD_WIDTH: f32 = 220.0;
const CARD_GAP: f32 = 12.0;
const COVER_HEIGHT: f32 = 132.0;

pub(super) fn list_row_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-list-row", row))
}

pub(super) fn gallery_card_id(row: &str) -> egui::Id {
    egui::Id::unique(("silex-gallery-card", row))
}

fn row_menu(row: &str) -> egui::Id {
    egui::Id::unique(("silex-collection-menu", row))
}

/// What part of an image of `image` size fills `area` without stretching
/// (cut at the sides or at top and bottom, centered), in texture
/// coordinates.
pub(super) fn cover_uv(image: egui::Vec2, area: egui::Vec2) -> egui::Rect {
    let full = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    if image.x <= 0.0 || image.y <= 0.0 || area.x <= 0.0 || area.y <= 0.0 {
        return full;
    }
    let scale = (area.x / image.x).max(area.y / image.y);
    let u = (area.x / (image.x * scale)).min(1.0);
    let v = (area.y / (image.y * scale)).min(1.0);
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v))
}

/// How many property values fit on a list's line of `width`, after the
/// name and «⋯».
pub(super) fn values_that_fit(width: f32, wanted: usize) -> usize {
    let room = width - LIST_NAME_MIN - icons::BUTTON - 2.0 * theme::space::SM;
    ((room / VALUE_WIDTH).floor().max(0.0) as usize).min(wanted)
}

/// The name a screen reader says for a row: its title and the values on
/// show («Rayuela; Autor: Julio Cortázar»).
fn spoken_row(prefix: &str, title: &str, lines: &[(&Property, &CellValue)]) -> String {
    let mut label = format!("{prefix}«{title}»");
    for (property, value) in lines {
        label.push_str(&format!("; {}: {}", property.name, value.display(property)));
    }
    label
}

/// The line under the view's tabs, with the open tab underlined.
fn under_tabs(ui: &mut egui::Ui, selected_tab: Option<egui::Rect>) {
    let p = theme::current(ui.ctx());
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = line(ui, ui.available_width(), p.border);
    ui.spacing_mut().item_spacing.y = spacing;
    if let Some(tab) = selected_tab {
        ui.painter()
            .hline(tab.x_range(), top, egui::Stroke::new(2.0, p.accent));
    }
}

/// «Nueva» after the rows: a new row, opened to be named.
fn new_row_button(ui: &mut egui::Ui, actions: &mut Vec<TableAction>) {
    let p = theme::current(ui.ctx());
    if ui
        .add(
            egui::Button::new((
                icons::icon(icons::PLUS).color(p.text_muted),
                egui::RichText::new(" Nueva").color(p.text_muted),
            ))
            .frame_when_inactive(false),
        )
        .on_hover_text("Una fila nueva, abierta para escribir su nombre")
        .accessible_name("Nueva fila")
        .clicked()
    {
        actions.push(TableAction::AddAndOpen);
    }
}

/// What a line or card does when clicked, from the keyboard, or through
/// its menus.
fn row_actions(
    app: &SilexApp,
    ui: &mut egui::Ui,
    response: &egui::Response,
    more: Option<egui::Response>,
    row: &str,
    handle_focus: &mut Option<egui::Id>,
    actions: &mut Vec<TableAction>,
) {
    if response.clicked() {
        actions.push(TableAction::Open(row.to_owned()));
    }
    let mut menu = |ui: &mut egui::Ui| {
        if let Some(chosen) = app.page_menu(ui, row) {
            actions.push(TableAction::Row(chosen));
            ui.close();
        }
    };
    response.context_menu(&mut menu);
    let mut focused = response.has_focus().then_some(response.id);
    if let Some(more) = more {
        egui::Popup::menu(&more).show(&mut menu);
        if more.has_focus() {
            focused = Some(more.id);
        }
    }
    if let Some(id) = focused {
        *handle_focus = Some(id);
        if take_delete_key(ui.ctx()) {
            actions.push(TableAction::Row(RowAction::Trash(row.to_owned())));
        }
    }
}

impl SilexApp {
    /// The list under the toolbar: a line per row in sight, then «Nueva».
    pub(super) fn draw_list(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        selected_tab: Option<egui::Rect>,
        actions: &mut Vec<TableAction>,
        _cells: &mut Vec<CellAction>,
    ) {
        let p = theme::current(ui.ctx());
        under_tabs(ui, selected_tab);
        let DatabaseState {
            id,
            rows,
            properties,
            columns,
            values,
            order,
            handle_focus,
            ..
        } = state;
        let shown: Vec<&Property> = columns.iter().filter_map(|&i| properties.get(i)).collect();
        if order.is_empty() {
            ui.add_space(theme::space::MD);
            ui.label(egui::RichText::new("Sin filas todavía.").color(p.text_muted));
        }
        let max_height = (ui.available_height() - 48.0).max(3.0 * LIST_ROW);
        visible_scroll_bars(ui);
        egui::ScrollArea::vertical()
            .id_salt(("silex-list", id.as_str()))
            .auto_shrink([false, true])
            .max_height(max_height)
            .show_rows(ui, LIST_ROW, order.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for &index in &order[range] {
                    let Some(row) = rows.get(index) else {
                        continue;
                    };
                    let width = ui.available_width();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, LIST_ROW), egui::Sense::hover());
                    self.list_row(ui, rect, row, &shown, values, handle_focus, actions);
                }
            });
        ui.add_space(theme::space::SM);
        new_row_button(ui, actions);
    }

    /// One line of the list.
    #[allow(clippy::too_many_arguments)]
    fn list_row(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        row: &silex_desktop::model::Page,
        shown: &[&Property],
        values: &TableValues,
        handle_focus: &mut Option<egui::Id>,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        let response = ui.interact(rect, list_row_id(&row.id), egui::Sense::click());
        let title = display_title(&row.title);
        let untitled = row.title.trim().is_empty();
        let lines = card_lines(&row.id, shown, values);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, spoken_row("", title, &lines))
        });
        let pointed = ui.rect_contains_pointer(rect);
        if pointed || response.has_focus() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.subtle);
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, p.border),
        );
        focus_ring(ui, rect, &response);

        let left = rect.left() + theme::space::SM;
        let icon = icons::paint_page_icon(
            ui.painter(),
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            self.page_icons.get(&row.id).map(String::as_str),
            icons::PAGE,
            theme::icon::MD,
            p.text_muted,
        );
        // The values on the right, each in its slot, as many as fit.
        let fit = values_that_fit(rect.width(), shown.len());
        let more_left = rect.right() - theme::space::SM - icons::BUTTON;
        let values_left = more_left - fit as f32 * VALUE_WIDTH;
        for (slot, property) in shown.iter().take(fit).enumerate() {
            let Some(value) = values
                .get(&row.id)
                .and_then(|cells| cells.get(&property.id))
            else {
                continue;
            };
            if matches!(value, CellValue::Checkbox(false)) {
                continue;
            }
            let at = egui::pos2(values_left + slot as f32 * VALUE_WIDTH, rect.center().y);
            paint_value(
                ui,
                ui.painter(),
                at,
                property,
                value,
                VALUE_WIDTH - theme::space::SM,
            );
        }
        let name_left = icon.right() + theme::space::SM;
        let color = if untitled { p.text_faint } else { p.text };
        paint_text(
            ui,
            ui.painter(),
            egui::pos2(name_left, rect.center().y),
            title,
            values_left - name_left - theme::space::SM,
            color,
        );

        let at = egui::Rect::from_min_size(
            egui::pos2(more_left, rect.center().y - icons::BUTTON / 2.0),
            egui::Vec2::splat(icons::BUTTON),
        );
        let more = icons::more_button(
            ui,
            at,
            row_menu(&row.id),
            pointed || response.has_focus(),
            p.subtle,
            &format!("Acciones de «{title}»"),
        );
        row_actions(
            self,
            ui,
            &response,
            Some(more),
            &row.id,
            handle_focus,
            actions,
        );
    }

    /// The gallery under the toolbar: cards in as many columns as fit,
    /// then «Nueva».
    pub(super) fn draw_gallery(
        &self,
        ui: &mut egui::Ui,
        state: &mut DatabaseState,
        selected_tab: Option<egui::Rect>,
        actions: &mut Vec<TableAction>,
        cells: &mut Vec<CellAction>,
    ) {
        let p = theme::current(ui.ctx());
        under_tabs(ui, selected_tab);
        ui.add_space(theme::space::MD);
        if state.covers.is_none() {
            let covers = self.db.database_covers(&state.id).unwrap_or_else(|err| {
                cells.push(CellAction::Notice(format!(
                    "No se pudieron leer las portadas de la galería: {err:#}"
                )));
                HashMap::new()
            });
            state.covers = Some(covers);
        }
        let DatabaseState {
            id,
            rows,
            properties,
            columns,
            values,
            order,
            handle_focus,
            covers,
            covers_seen,
            ..
        } = state;
        let covers = covers.as_ref();
        let shown: Vec<&Property> = columns.iter().filter_map(|&i| properties.get(i)).collect();
        if order.is_empty() {
            ui.label(egui::RichText::new("Sin filas todavía.").color(p.text_muted));
        }
        let cover_of = |index: usize| {
            let row = rows.get(index)?;
            covers?.get(&row.id)
        };
        // Covers take room only if some card in sight has one.
        let cover = if order.iter().any(|&i| cover_of(i).is_some()) {
            COVER_HEIGHT
        } else {
            0.0
        };
        let card_height = cover + 2.0 * CARD_PAD + TITLE_HEIGHT + shown.len() as f32 * LINE_HEIGHT;
        let width = ui.available_width();
        let per_line = (((width + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).floor() as usize).max(1);
        let lines = order.len().div_ceil(per_line);
        let max_height = (ui.available_height() - 48.0).max(card_height);
        let mut seen = Vec::new();
        visible_scroll_bars(ui);
        egui::ScrollArea::vertical()
            .id_salt(("silex-gallery", id.as_str()))
            .auto_shrink([false, true])
            .max_height(max_height)
            .show_rows(ui, card_height, lines, |ui, range| {
                ui.spacing_mut().item_spacing = egui::vec2(CARD_GAP, CARD_GAP);
                for line in range {
                    ui.horizontal(|ui| {
                        let start = line * per_line;
                        let end = (start + per_line).min(order.len());
                        for &index in &order[start..end] {
                            let Some(row) = rows.get(index) else {
                                continue;
                            };
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(CARD_WIDTH, card_height),
                                egui::Sense::hover(),
                            );
                            let card = GalleryCard {
                                row,
                                shown: &shown,
                                values,
                                cover,
                                image: cover_of(index),
                            };
                            if let Some(image) = card.image
                                && ui.is_rect_visible(rect)
                            {
                                seen.push(image.clone());
                            }
                            self.gallery_card(ui, rect, &card, handle_focus, actions);
                        }
                    });
                }
            });
        *covers_seen = seen;
        ui.add_space(theme::space::SM);
        new_row_button(ui, actions);
    }

    /// One card of the gallery: its cover, name and values.
    fn gallery_card(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        card: &GalleryCard<'_>,
        handle_focus: &mut Option<egui::Id>,
        actions: &mut Vec<TableAction>,
    ) {
        let p = theme::current(ui.ctx());
        let row = card.row;
        let response = ui.interact(rect, gallery_card_id(&row.id), egui::Sense::click());
        let title = display_title(&row.title);
        let lines = card_lines(&row.id, card.shown, card.values);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                true,
                spoken_row("Tarjeta ", title, &lines),
            )
        });
        let pointed = ui.rect_contains_pointer(rect);
        CardLook {
            title: Some((title, row.title.trim().is_empty())),
            lines: &lines,
            hovered: pointed || response.has_focus(),
            cover: card.cover,
        }
        .paint(ui, ui.painter(), rect);
        if card.cover > 0.0 {
            let area = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), card.cover))
                .shrink(1.0);
            let radius = egui::CornerRadius {
                nw: theme::radius::MD,
                ne: theme::radius::MD,
                sw: 0,
                se: 0,
            };
            match card.image.and_then(|id| self.files.previews.get(id)) {
                Some(PreviewState::Ready(texture)) => {
                    egui::Image::from_texture(texture)
                        .uv(cover_uv(texture.size_vec2(), area.size()))
                        .corner_radius(radius)
                        .paint_at(ui, area);
                }
                _ => {
                    ui.painter().rect_filled(area, radius, p.subtle);
                }
            }
            ui.painter().hline(
                area.x_range(),
                area.bottom() + 0.5,
                egui::Stroke::new(1.0, p.border),
            );
        }
        focus_ring(ui, rect, &response);

        let side = icons::BUTTON;
        let at = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - CARD_PAD / 2.0 - side,
                rect.top() + card.cover + CARD_PAD + (TITLE_HEIGHT - side) / 2.0,
            ),
            egui::Vec2::splat(side),
        );
        let more = icons::more_button(
            ui,
            at,
            row_menu(&row.id),
            pointed || response.has_focus(),
            p.bg,
            &format!("Acciones de «{title}»"),
        );
        row_actions(
            self,
            ui,
            &response,
            Some(more),
            &row.id,
            handle_focus,
            actions,
        );
    }
}

/// What a gallery card shows.
struct GalleryCard<'a> {
    row: &'a silex_desktop::model::Page,
    shown: &'a [&'a Property],
    values: &'a TableValues,
    /// Height of the cover area (0: no card in sight has one).
    cover: f32,
    /// The attachment of the row's cover, if it has one.
    image: Option<&'a String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: egui::Rect, b: egui::Rect) -> bool {
        (a.min - b.min).length() < 1e-4 && (a.max - b.max).length() < 1e-4
    }

    #[test]
    fn a_cover_is_cut_to_fill_without_stretching() {
        let full = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
        // Same shape: all of it.
        assert!(close(
            cover_uv(egui::vec2(200.0, 100.0), egui::vec2(400.0, 200.0)),
            full
        ));
        // A tall image in a wide area: the middle half, top and bottom cut.
        let uv = cover_uv(egui::vec2(100.0, 100.0), egui::vec2(200.0, 100.0));
        assert!(close(
            uv,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.25), egui::pos2(1.0, 0.75))
        ));
        // A wide image in a square: the sides cut.
        let uv = cover_uv(egui::vec2(400.0, 100.0), egui::vec2(100.0, 100.0));
        assert!(close(
            uv,
            egui::Rect::from_min_max(egui::pos2(0.375, 0.0), egui::pos2(0.625, 1.0))
        ));
        // Nothing to measure: all of it.
        assert!(close(
            cover_uv(egui::Vec2::ZERO, egui::vec2(10.0, 10.0)),
            full
        ));
    }

    #[test]
    fn a_narrow_list_shows_fewer_values() {
        assert_eq!(values_that_fit(1200.0, 3), 3);
        assert_eq!(values_that_fit(1200.0, 10), 6);
        assert_eq!(values_that_fit(400.0, 3), 1);
        assert_eq!(values_that_fit(200.0, 3), 0);
    }
}
