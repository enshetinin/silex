//! Rows in their manual order (H13.1): drag a row by its handle in the
//! table (the chosen rows, if it is one of them) or a card within its
//! board column, or use «Subir» / «Bajar» in its menu. Only when the view
//! has no sort: then the rows follow the sort, and trying says so. The
//! new order is saved in one transaction (`Database::arrange_rows`), and
//! is one step of Cmd+Z in the table (`TableOp::Order`).
use anyhow::Result;
use eframe::egui;
use silex_desktop::db::Shift;
use silex_desktop::view::{NAME, board_groups, board_property, changed_span, drop_rows};

use super::*;
use super::{icons, theme};

/// What a row dragged by its handle carries: the rows it moves, in the
/// view's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RowDrag(pub(super) Vec<String>);

/// Within this distance of the top or bottom of the rows while dragging,
/// they scroll.
const SCROLL_EDGE: f32 = 32.0;

impl DatabaseState {
    /// The ids of the rows on show, in the view's order.
    pub(super) fn shown_ids(&self) -> Vec<String> {
        self.order
            .iter()
            .filter_map(|&i| self.rows.get(i).map(|r| r.id.clone()))
            .collect()
    }

    /// The ids of every row, in their manual order.
    fn row_ids(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.id.clone()).collect()
    }

    /// The list `row` is shown in, in order: the table's rows, or the
    /// cards of its column on a board. `None` if it is not on show.
    pub(super) fn shown_with(&self, row: &str) -> Option<Vec<String>> {
        let list = if self.is_board() {
            let property = board_property(&self.properties, self.config()?)?;
            board_groups(&self.order, &self.rows, &self.values, property)
                .into_iter()
                .map(|group| {
                    group
                        .rows
                        .iter()
                        .filter_map(|&i| self.rows.get(i).map(|r| r.id.clone()))
                        .collect::<Vec<_>>()
                })
                .find(|ids| ids.iter().any(|id| id == row))?
        } else {
            self.shown_ids()
        };
        list.iter().any(|id| id == row).then_some(list)
    }

    /// The name of what the view is sorted by first («Páginas»), if it is
    /// sorted: then rows cannot be moved by hand.
    pub(super) fn sorted_by(&self) -> Option<String> {
        let config = self.config()?;
        if !config.is_sorted(&self.properties) {
            return None;
        }
        config.sorts.iter().find_map(|sort| {
            if sort.property == NAME {
                return Some("Nombre".to_owned());
            }
            self.properties
                .iter()
                .find(|p| p.id == sort.property)
                .map(|p| p.name.clone())
        })
    }
}

/// Why rows cannot be moved by hand in a sorted view, and how to.
pub(super) fn sorted_explanation(by: &str) -> String {
    format!(
        "Esta vista está ordenada por «{by}». Para mover las filas a mano, quita el orden en «Ordenar»."
    )
}

impl SilexApp {
    /// Moves `moving` to the gap `gap` of `shown` (the rows of the table,
    /// or of a board column, in the view's order). One step of Cmd+Z. In a
    /// sorted view it only says why it cannot.
    pub(super) fn drop_rows(
        &mut self,
        moving: &[String],
        shown: &[String],
        gap: usize,
    ) -> Result<()> {
        if let Some(by) = self.database.as_ref().and_then(DatabaseState::sorted_by) {
            self.notice = Some(sorted_explanation(&by));
            return Ok(());
        }
        self.save_table_edits()?;
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        let all = state.row_ids();
        let Some(after) = drop_rows(&all, shown, moving, gap) else {
            return Ok(());
        };
        let (was, is) = changed_span(&all, &after);
        let database = state.id.clone();
        if self.db.arrange_rows(&database, &is)? {
            self.record_table(TableOp::Order {
                before: was,
                after: is,
            });
            self.reload_rows()?;
        }
        Ok(())
    }

    /// «Subir» / «Bajar» in a row's menu, in the table or a board: one
    /// place in the list it is shown in.
    pub(super) fn shift_row(&mut self, row: &str, shift: Shift) -> Result<()> {
        let Some(shown) = self.database.as_ref().and_then(|d| d.shown_with(row)) else {
            return Ok(());
        };
        let Some(index) = shown.iter().position(|id| id == row) else {
            return Ok(());
        };
        let gap = match shift {
            Shift::Up if index > 0 => index - 1,
            Shift::Down if index + 1 < shown.len() => index + 2,
            _ => return Ok(()),
        };
        self.drop_rows(&[row.to_owned()], &shown, gap)
    }

    /// Reads the rows again after their order changed, and shows them in
    /// it (the table's order is otherwise kept while it is open).
    pub(super) fn reload_rows(&mut self) -> Result<()> {
        self.reload_pages()?;
        if let Some(state) = self.database.as_mut() {
            state.apply_view();
        }
        Ok(())
    }
}

/// The handle in the gutter at the left of a table row (`strip`, out of
/// the rows' clip): dragging it moves the row, or the chosen rows if it is
/// one of them. In a sorted view, it says why it cannot. Not in the Tab
/// order: «Subir» and «Bajar» in the row's menu do the same by keyboard.
pub(super) fn row_handle(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    row: &Page,
    moving: impl FnOnce() -> Vec<String>,
    sorted: Option<&str>,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    let rect = egui::Rect::from_center_size(strip.center(), egui::vec2(22.0, 22.0));
    let sense = if sorted.is_some() {
        egui::Sense::CLICK
    } else {
        egui::Sense::DRAG
    };
    let handle = ui.interact(rect, egui::Id::unique(("silex-row-handle", &row.id)), sense);
    let name = format!("Mover «{}»", display_title(&row.title));
    handle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if handle.hovered() || handle.dragged() {
        ui.painter().rect_filled(rect, theme::radius::SM, p.subtle);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icons::DRAG_HANDLE,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    match sorted {
        Some(by) => {
            if handle.clicked() {
                actions.push(TableAction::ExplainSorted);
            }
            handle.on_hover_text(sorted_explanation(by));
        }
        None => {
            if handle.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), RowDrag(moving()));
            }
            handle
                .on_hover_cursor(egui::CursorIcon::Grab)
                .on_hover_text("Arrastrar para mover la fila");
        }
    }
}

/// Where the rows of a table are drawn this frame, for a row being dragged.
pub(super) struct TableDrop {
    /// Where the first row of the view would be (it may be scrolled away).
    pub(super) top: f32,
    /// The rows in sight: the scroll area's clip.
    pub(super) clip: egui::Rect,
    /// Left edge of the handles' gutter.
    pub(super) gutter_left: f32,
    pub(super) row_height: f32,
}

/// While rows are dragged by their handle in the table: their name by the
/// pointer, the line where they would go, scrolling near the edges, and
/// Esc to give up. On release over the table, the drop.
pub(super) fn drag_rows(
    ui: &mut egui::Ui,
    state: &DatabaseState,
    drag: &RowDrag,
    at: &TableDrop,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        egui::DragAndDrop::clear_payload(&ctx);
        return;
    }
    let Some(pointer) = ctx.pointer_interact_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    // Their name (or how many) follows the pointer, on top.
    let label = match drag.0.as_slice() {
        [one] => state
            .rows
            .iter()
            .find(|r| r.id == *one)
            .map(|r| display_title(&r.title).chars().take(48).collect())
            .unwrap_or_default(),
        many => format!("{} filas", many.len()),
    };
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("silex_row_drag"));
    let painter = ctx.layer_painter(layer);
    let galley = painter.layout_no_wrap(label, egui::TextStyle::Body.resolve(ui.style()), p.text);
    let card_at = pointer + egui::vec2(14.0, 6.0);
    let card = egui::Rect::from_min_size(card_at, galley.size() + egui::vec2(16.0, 10.0));
    painter.rect_filled(card, egui::CornerRadius::same(theme::radius::SM), p.bg);
    painter.rect_stroke(
        card,
        egui::CornerRadius::same(theme::radius::SM),
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.galley(card_at + egui::vec2(8.0, 5.0), galley, p.text);

    // Over the table: its gutter, its rows, a little above and below.
    let over = egui::Rect::from_x_y_ranges(
        at.gutter_left..=at.clip.right(),
        at.clip.top() - at.row_height / 2.0..=at.clip.bottom() + at.row_height,
    );
    if !over.contains(pointer) || state.order.is_empty() {
        return;
    }
    // Scroll when near the top or bottom of the rows, if there is more
    // that way.
    let rows = state.order.len();
    let below = rows as f32 * at.row_height - state.scroll.y - at.clip.height();
    let speed = if pointer.y < at.clip.top() + SCROLL_EDGE && state.scroll.y > 0.0 {
        8.0
    } else if pointer.y > at.clip.bottom() - SCROLL_EDGE && below > 0.0 {
        -8.0
    } else {
        0.0
    };
    if speed != 0.0 {
        ui.scroll_with_delta(egui::vec2(0.0, speed));
        ctx.request_repaint();
    }
    // The gap: the row boundary nearest the pointer.
    let gap = (((pointer.y - at.top) / at.row_height).round().max(0.0) as usize).min(rows);
    let shown = state.shown_ids();
    let moves = drop_rows(&state.row_ids(), &shown, &drag.0, gap).is_some();
    if moves {
        let y = (at.top + gap as f32 * at.row_height).clamp(at.clip.top(), at.clip.bottom());
        ui.painter().hline(
            at.clip.left()..=at.clip.right(),
            y,
            egui::Stroke::new(2.0, p.accent),
        );
    }
    let released = ctx.input(|i| i.pointer.any_released());
    if released && egui::DragAndDrop::take_payload::<RowDrag>(&ctx).is_some() && moves {
        actions.push(TableAction::DropRows {
            moving: drag.0.clone(),
            shown,
            gap,
        });
    }
}
