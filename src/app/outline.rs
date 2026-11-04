//! The page's outline (H12.5): its H1–H3 in a panel on the right, the one
//! being read marked, a click to go to each. Only view: it is not in the
//! document and needs no migration. Shown or hidden is a setting.
use eframe::egui;
use silex_desktop::editor;
use silex_desktop::settings::Settings;

use super::side_panel::MIN_PAGE_WIDTH;
use super::*;
use super::{icons, theme};

/// Narrowest and default width of the panel.
const MIN_WIDTH: f32 = 180.0;
const DEFAULT_WIDTH: f32 = 230.0;
const MAX_WIDTH: f32 = 340.0;
/// Indent per heading level.
const LEVEL_INDENT: f32 = 14.0;
/// Height of an entry.
const ROW: f32 = 28.0;
/// Room at the left of every entry, where the current one has its bar.
const BAR_ROOM: f32 = 10.0;
/// A heading counts as the one being read once its top is this close to
/// the top of the page's view.
pub(super) const READING_MARGIN: f32 = 48.0;

impl SilexApp {
    /// Whether the outline is on screen: wanted, a page (not a database)
    /// open, no page beside, and room for it.
    pub(super) fn outline_shown(&self) -> bool {
        self.settings.outline
            && self.selected.is_some()
            && self.database.is_none()
            && self.side.is_none()
            && self.main_width >= MIN_PAGE_WIDTH + MIN_WIDTH
    }

    /// Shows or hides the outline, and remembers it.
    pub(super) fn toggle_outline(&mut self, ctx: &egui::Context) {
        let settings = Settings {
            outline: !self.settings.outline,
            ..self.settings
        };
        self.set_settings(ctx, settings);
    }

    /// Goes to heading `id`: opens what hides it, scrolls it to the top
    /// of the page and puts the cursor at its end.
    pub(super) fn go_to_heading(&mut self, id: &str) {
        let Some(index) = self.blocks.iter().position(|b| b.id == id) else {
            return;
        };
        self.reveal_block(index);
        self.selection = None;
        self.outline_target = Some(id.to_owned());
        self.outline_chosen = Some((id.to_owned(), None));
        let end = editor::block_char_len(&self.blocks[index]);
        self.pending_focus = Some((id.to_owned(), end));
    }

    /// The panel, when shown.
    pub(super) fn outline_panel(&mut self, ui: &mut egui::Ui) {
        if !self.outline_shown() {
            return;
        }
        let p = theme::current(ui.ctx());
        let entries = editor::outline(&self.blocks);
        let max = (self.main_width - MIN_PAGE_WIDTH).clamp(MIN_WIDTH, MAX_WIDTH);
        let mut go = None;
        let mut hide = false;
        egui::Panel::right("silex_outline")
            .resizable(true)
            .default_size(DEFAULT_WIDTH)
            .size_range(MIN_WIDTH..=max)
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("En esta página")
                            .small()
                            .color(p.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        hide = icons::button(
                            ui,
                            true,
                            icons::CLOSE,
                            p.text_muted,
                            "Cerrar el índice",
                            None,
                        )
                        .clicked();
                    });
                });
                ui.add_space(theme::space::SM);
                if entries.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "Los títulos de la página (Título 1, 2 y 3) aparecen aquí.",
                        )
                        .small()
                        .color(p.text_muted),
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (n, entry) in entries.iter().enumerate() {
                            // A little air before each top-level section.
                            if n > 0 && entry.level == 0 {
                                ui.add_space(theme::space::SM);
                            }
                            let id = &self.blocks[entry.index].id;
                            let current = self.outline_current.as_deref() == Some(id);
                            if outline_entry(ui, entry, current).clicked() {
                                go = Some(id.clone());
                            }
                        }
                    });
            });
        if hide {
            let ctx = ui.ctx().clone();
            self.toggle_outline(&ctx);
        }
        if let Some(id) = go {
            self.go_to_heading(&id);
        }
    }
}

/// One heading in the outline. The level shows three ways: the largest
/// headings in semibold, the next ones indented, the smallest also
/// smaller and muted; a thin line joins each to the section it is in.
/// The one being read is in the accent color, with a bar at its left.
fn outline_entry(ui: &mut egui::Ui, entry: &editor::OutlineEntry, current: bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::hover());
    let response = ui.interact(
        rect,
        egui::Id::unique(("silex-outline-entry", entry.index)),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &entry.text));
    let painter = ui.painter();
    if response.hovered() || response.has_focus() {
        painter.rect_filled(rect, egui::CornerRadius::same(theme::radius::SM), p.subtle);
    }
    // The guides of the sections this heading is in.
    for level in 0..entry.level {
        // On a pixel's center, so a 1 px line stays sharp.
        let x = painter
            .round_to_pixel_center(rect.left() + BAR_ROOM + f32::from(level) * LEVEL_INDENT + 3.0);
        painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, p.border_strong));
    }
    if current {
        let bar = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(0.0, 5.0),
            egui::vec2(2.0, ROW - 10.0),
        );
        painter.rect_filled(bar, 1.0, p.accent);
    }
    let (font, color) = match entry.level {
        0 => (
            egui::FontId::new(14.0, egui::FontFamily::Name(theme::SEMIBOLD.into())),
            p.text,
        ),
        1 => (egui::FontId::proportional(14.0), p.text),
        _ => (egui::FontId::proportional(13.0), p.text_muted),
    };
    let color = if current { p.accent_strong } else { color };
    let left = rect.left() + BAR_ROOM + f32::from(entry.level) * LEVEL_INDENT;
    let mut job = egui::text::LayoutJob::simple_singleline(entry.text.clone(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - 6.0 - left).max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let elided = galley.elided;
    let top = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(egui::pos2(left, top), galley, color);
    super::cells::focus_ring(ui, rect, &response);
    // A long title, cut with «…»: whole on hover.
    if elided {
        response.on_hover_text(&entry.text)
    } else {
        response
    }
}
