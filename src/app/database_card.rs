//! The card of a database in a page (H12.11): its name and row count, and
//! a small table with the first rows of the view last open in it, read
//! only. A click (or Intro) opens the database.
use eframe::egui;
use silex_desktop::property::{CellValue, PropertyKind};
use silex_desktop::view::TablePreview;

use super::blocks::{BlockAction, paint_check_box};
use super::cells::{CELL_PAD, focus_ring, paint_cell_text, paint_tag_on, property_icon};
use super::links::LinkState;
use super::*;
use super::{icons, theme};

/// Rows the card shows at most.
pub(super) const PREVIEW_ROWS: usize = 5;
const HEADER: f32 = 44.0;
const COLUMN_HEAD: f32 = 30.0;
const ROW: f32 = 32.0;
const FOOTER: f32 = 32.0;
/// Air above and below the card, between it and the blocks around.
const MARGIN: f32 = 6.0;
/// Narrowest the «Nombre» column and the others get: columns that do not
/// fit are left out (the database shows them all).
const NAME_WIDTH: f32 = 180.0;
const COLUMN_WIDTH: f32 = 120.0;

/// «1 fila», «12 filas».
fn rows_text(n: usize) -> String {
    if n == 1 {
        "1 fila".to_owned()
    } else {
        format!("{n} filas")
    }
}

/// How many property columns fit after «Nombre» in `width`, and how wide
/// «Nombre» and each of them are.
fn column_widths(width: f32, properties: usize) -> (f32, usize, f32) {
    let name = NAME_WIDTH.max(width * 0.34);
    let fit = (((width - name) / COLUMN_WIDTH).floor().max(0.0) as usize).min(properties);
    if fit == 0 {
        return (width, 0, 0.0);
    }
    (name, fit, (width - name) / fit as f32)
}

/// The card. `state` is the link's (a live database), `table` what it shows.
pub(super) fn database_card(
    ui: &mut egui::Ui,
    id: egui::Id,
    state: &LinkState,
    table: &TablePreview,
) -> (egui::Response, Option<BlockAction>) {
    let p = theme::current(ui.ctx());
    let (title, emoji) = match state {
        LinkState::Live { title, icon, .. } => (display_title(title).to_owned(), icon.as_deref()),
        _ => (display_title("").to_owned(), None),
    };
    let shown = table.rows.len();
    let footer = table.total > shown || table.total == 0;
    let height = HEADER + COLUMN_HEAD + shown as f32 * ROW + if footer { FOOTER } else { 0.0 };
    let (outer, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height + 2.0 * MARGIN),
        egui::Sense::hover(),
    );
    let rect = outer.shrink2(egui::vec2(0.0, MARGIN));
    let response = ui.interact(rect, id, egui::Sense::click());
    let label = format!("Base de datos «{title}», {}. Abrir", rows_text(table.total));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let lit = response.hovered() || response.has_focus();

    let painter = ui.painter_at(rect.expand(2.0));
    let radius = egui::CornerRadius::same(theme::radius::LG);
    painter.rect_filled(rect, radius, p.bg);
    let line = egui::Stroke::new(1.0, p.border);
    let edge = if lit { p.border_strong } else { p.border };

    // Header: icon, name, and on the right the rows, the view and «Abrir».
    let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), HEADER));
    let at = icons::paint_page_icon(
        &painter,
        egui::pos2(header.left() + 14.0, header.center().y),
        egui::Align2::LEFT_CENTER,
        emoji,
        icons::TABLE,
        theme::icon::LG,
        p.accent_strong,
    );
    let open_color = if lit { p.accent_strong } else { p.text_muted };
    let open_icon = painter.text(
        egui::pos2(header.right() - 14.0, header.center().y),
        egui::Align2::RIGHT_CENTER,
        icons::OPEN_EXTERNAL,
        icons::font(theme::icon::MD),
        open_color,
    );
    let small = egui::TextStyle::Small.resolve(ui.style());
    let open = painter.text(
        egui::pos2(open_icon.left() - 6.0, header.center().y),
        egui::Align2::RIGHT_CENTER,
        "Abrir",
        egui::TextStyle::Body.resolve(ui.style()),
        open_color,
    );
    let meta = painter.text(
        egui::pos2(open.left() - theme::space::LG, header.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{} · {}", rows_text(table.total), table.view),
        small.clone(),
        p.text_muted,
    );
    let left = at.right() + 10.0;
    let mut job = egui::text::LayoutJob::simple_singleline(
        title,
        egui::FontId::new(16.0, egui::FontFamily::Name(theme::SEMIBOLD.into())),
        p.text,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (meta.left() - theme::space::LG - left).max(0.0),
    );
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(
        egui::pos2(left, header.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );

    // The column heads, on a band.
    let (name_width, fit, width) = column_widths(rect.width(), table.columns.len());
    let columns = &table.columns[..fit];
    let head = egui::Rect::from_min_size(
        egui::pos2(rect.left(), header.bottom()),
        egui::vec2(rect.width(), COLUMN_HEAD),
    );
    painter.rect_filled(head, 0.0, p.subtle);
    painter.hline(head.x_range(), head.top(), line);
    painter.hline(head.x_range(), head.bottom(), line);
    let column_x = |i: usize| rect.left() + name_width + i as f32 * width;
    let head_cell = |x: f32, w: f32, icon: &str, name: &str| {
        let icon_rect = painter.text(
            egui::pos2(x + CELL_PAD, head.center().y),
            egui::Align2::LEFT_CENTER,
            icon,
            icons::font(theme::icon::MD - 2.0),
            p.text_muted,
        );
        let mut job =
            egui::text::LayoutJob::simple_singleline(name.to_owned(), small.clone(), p.text_muted);
        job.wrap = egui::text::TextWrapping::truncate_at_width(
            (x + w - icon_rect.right() - 6.0 - CELL_PAD).max(0.0),
        );
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(
            egui::pos2(
                icon_rect.right() + 6.0,
                head.center().y - galley.size().y / 2.0,
            ),
            galley,
            p.text_muted,
        );
    };
    head_cell(rect.left(), name_width, icons::TEXT, "Nombre");
    for (i, property) in columns.iter().enumerate() {
        head_cell(
            column_x(i),
            width,
            property_icon(property.kind),
            &property.name,
        );
    }

    // The rows.
    for (r, (row, values)) in table.rows.iter().enumerate() {
        let top = head.bottom() + r as f32 * ROW;
        let band =
            egui::Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(rect.width(), ROW));
        if r > 0 {
            painter.hline(band.x_range(), band.top(), line);
        }
        let name = egui::Rect::from_min_size(band.min, egui::vec2(name_width, ROW));
        paint_cell_text(
            ui,
            name,
            display_title(&row.title),
            egui::Align2::LEFT_CENTER,
            p.text,
        );
        for (i, property) in columns.iter().enumerate() {
            let cell =
                egui::Rect::from_min_size(egui::pos2(column_x(i), top), egui::vec2(width, ROW));
            match (property.kind, values.get(&property.id)) {
                (PropertyKind::Checkbox, value) => {
                    let on = matches!(value, Some(CellValue::Checkbox(true)));
                    let center = egui::pos2(cell.left() + CELL_PAD + 8.0, cell.center().y);
                    paint_check_box(ui, center, on, &response);
                }
                (_, Some(CellValue::MultiSelect(ids))) => {
                    let options: Vec<_> = property
                        .options
                        .iter()
                        .filter(|o| ids.contains(&o.id))
                        .collect();
                    paint_tags(
                        ui,
                        &painter,
                        egui::pos2(cell.left() + CELL_PAD, cell.center().y),
                        &options,
                        cell.width() - 2.0 * CELL_PAD,
                    );
                }
                (_, Some(CellValue::Select(option))) => {
                    if let Some(option) = property.option(option) {
                        paint_tag_on(
                            ui,
                            &painter,
                            egui::pos2(cell.left() + CELL_PAD, cell.center().y),
                            option,
                            cell.width() - 2.0 * CELL_PAD,
                        );
                    }
                }
                (kind, Some(value)) => {
                    let align = if kind == PropertyKind::Number {
                        egui::Align2::RIGHT_CENTER
                    } else {
                        egui::Align2::LEFT_CENTER
                    };
                    paint_cell_text(ui, cell, &value.display(property), align, p.text);
                }
                (_, None) => {}
            }
        }
    }
    // Lines between the columns, from the heads to the last row.
    let bottom = head.bottom() + shown as f32 * ROW;
    for i in 0..fit {
        painter.vline(column_x(i), head.top()..=bottom, line);
    }

    // What is not shown, or that there is nothing yet.
    if footer {
        let band = egui::Rect::from_min_max(egui::pos2(rect.left(), bottom), rect.max);
        painter.hline(band.x_range(), band.top(), line);
        let text = if table.total == 0 {
            "Sin filas todavía. Ábrela para añadir la primera.".to_owned()
        } else {
            let more = table.total - shown;
            if more == 1 {
                "+ 1 fila más".to_owned()
            } else {
                format!("+ {more} filas más")
            }
        };
        painter.text(
            egui::pos2(band.left() + 14.0, band.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            small,
            p.text_muted,
        );
    }
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    focus_ring(ui, rect, &response);
    let action = response.clicked().then_some(BlockAction::OpenLink);
    (response, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_that_do_not_fit_are_left_out() {
        // Wide: «Nombre» a third, four columns share the rest.
        let (name, fit, width) = column_widths(800.0, 4);
        assert_eq!(fit, 4);
        assert!((name - 272.0).abs() < 0.01);
        assert!((width - 132.0).abs() < 0.01);
        // Narrow: fewer columns, never narrower than they should be.
        let (name, fit, width) = column_widths(500.0, 6);
        assert_eq!(name, NAME_WIDTH);
        assert_eq!(fit, 2);
        assert!(width >= COLUMN_WIDTH);
        // No room, or no properties: «Nombre» takes it all.
        assert_eq!(column_widths(250.0, 3), (250.0, 0, 0.0));
        assert_eq!(column_widths(720.0, 0), (720.0, 0, 0.0));
        assert_eq!(rows_text(1), "1 fila");
        assert_eq!(rows_text(0), "0 filas");
    }
}
