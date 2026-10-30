//! Tabs in the window (H8.1, ADR 0011): the bar above the page, and
//! opening, closing and switching tabs. Which pages are open, and the
//! rules, are in `silex_desktop::tabs`.
use anyhow::Result;
use eframe::egui;
use silex_desktop::history::History;
use silex_desktop::model::PageKind;

use super::*;
use super::{icons, theme};

/// What a tab keeps while another one is shown.
pub(super) struct Parked {
    pub(super) history: History,
    pub(super) scroll: f32,
}

/// Height of the tab bar.
const TAB_HEIGHT: f32 = 26.0;
/// Space between the track's edge and the tabs.
const TRACK_PAD: f32 = 3.0;
/// Widest a tab gets.
const TAB_MAX_WIDTH: f32 = 200.0;

/// What a click in the tab bar asks for.
enum TabAction {
    Select(usize),
    Close(usize),
}

impl SilexApp {
    /// Opens a page in a new tab (Cmd/Ctrl+click, «Abrir en una pestaña
    /// nueva»), or shows the tab that has it.
    pub(super) fn open_in_new_tab(&mut self, id: &str) -> Result<()> {
        if self.db.get_page(id)?.is_none() {
            return Ok(());
        }
        // The first tab is the page already open.
        if self.tabs.is_empty()
            && let Some(open) = self.selected.clone()
        {
            self.tabs.open_here(&open);
        }
        for closed in self.tabs.open_new(id) {
            self.parked.remove(&closed);
        }
        self.show_active_tab(false)
    }

    /// Opens a page in the active tab, or in a new one when `new_tab`.
    pub(super) fn open_page_in(&mut self, id: String, new_tab: bool) -> Result<()> {
        if new_tab {
            self.open_in_new_tab(&id)
        } else {
            self.open_page(id)
        }
    }

    pub(super) fn select_tab(&mut self, index: usize) -> Result<()> {
        if self.tabs.select(index) {
            self.show_active_tab(false)?;
        }
        Ok(())
    }

    /// The next or previous tab (Ctrl+Tab, Ctrl+Mayús+Tab).
    pub(super) fn cycle_tab(&mut self, forward: bool) -> Result<()> {
        self.tabs.cycle(forward);
        self.show_active_tab(false)
    }

    /// Closes a tab (its «×», a middle click, Cmd/Ctrl+W). The only tab
    /// stays: the window always shows a page.
    pub(super) fn close_tab(&mut self, index: usize) -> Result<()> {
        if let Some(closed) = self.tabs.close(index) {
            self.parked.remove(&closed);
            self.show_active_tab(false)?;
        }
        Ok(())
    }

    /// The tab bar, when more than one tab is open: icon, title and «×»
    /// (on the active tab, or under the pointer).
    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        if self.tabs.len() < 2 {
            return;
        }
        let p = theme::current(ui.ctx());
        let mut action = None;
        let count = self.tabs.len() as f32;
        // A segmented control: the tabs sit in a track, inset by PAD, and
        // the open one is a raised chip.
        let spacing = 2.0;
        // Each tab as wide as its title, up to an equal share of the row.
        let max_width = ((ui.available_width() - 2.0 * TRACK_PAD - spacing * (count - 1.0))
            / count)
            .min(TAB_MAX_WIDTH);
        let font = egui::FontId::proportional(13.0);
        let track = ui.painter().add(egui::Shape::Noop);
        ui.add_space(TRACK_PAD);
        let dark = ui.visuals().dark_mode;
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            ui.add_space(TRACK_PAD);
            for (index, id) in self.tabs.ids().iter().enumerate() {
                let active = index == self.tabs.active();
                // The open page's title as it is being typed.
                let (title, kind) = if active {
                    (self.title.clone(), self.find_page(id).map(|p| p.kind))
                } else {
                    match self.find_page(id) {
                        Some(page) => (page.title.clone(), Some(page.kind)),
                        None => (String::new(), None),
                    }
                };
                let title = display_title(&title).to_owned();
                // Icon, title and room for «×» (kept on every tab, so a tab
                // does not change width under the pointer).
                let text = ui.fonts_mut(|f| {
                    f.layout_no_wrap(title.clone(), font.clone(), p.text)
                        .size()
                        .x
                });
                let width = (8.0 + theme::icon::MD + 6.0 + text + 6.0 + 20.0 + 4.0).min(max_width);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Tab, true, active, &title)
                });
                let radius = egui::CornerRadius::same(theme::radius::MD);
                if active {
                    let chip = if dark { p.pressed } else { p.bg };
                    ui.painter().rect(
                        rect,
                        radius,
                        chip,
                        egui::Stroke::new(1.0, p.border),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, radius, p.pressed);
                }
                focus_ring(ui, rect, &response);
                let icon = if kind == Some(PageKind::Database) {
                    icons::TABLE
                } else {
                    icons::PAGE
                };
                let at = icons::paint_page_icon(
                    ui.painter(),
                    egui::pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    self.page_icons.get(id).map(String::as_str),
                    icon,
                    theme::icon::MD,
                    p.text_muted,
                );
                // «×» on the active tab and under the pointer.
                let show_close = active || response.hovered();
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 14.0, rect.center().y),
                    egui::vec2(20.0, 20.0),
                );
                let right = if show_close {
                    close_rect.left() - 4.0
                } else {
                    rect.right() - 8.0
                };
                let left = at.right() + 6.0;
                let mut job = egui::text::LayoutJob::simple_singleline(
                    title.clone(),
                    font.clone(),
                    if active { p.text } else { p.text_muted },
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(
                    egui::pos2(left, rect.center().y - galley.size().y / 2.0),
                    galley,
                    p.text,
                );
                if show_close {
                    let close = ui
                        .interact(
                            close_rect,
                            ui.make_persistent_id(("silex_tab_close", id)),
                            egui::Sense::click(),
                        )
                        .on_hover_text("Cerrar la pestaña");
                    let close_label = format!("Cerrar «{title}»");
                    close.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &close_label)
                    });
                    if close.hovered() {
                        ui.painter().rect_filled(
                            close_rect,
                            egui::CornerRadius::same(theme::radius::SM),
                            p.subtle,
                        );
                    }
                    ui.painter().text(
                        close_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icons::CLOSE,
                        icons::font(14.0),
                        p.text_muted,
                    );
                    if close.clicked() {
                        action = Some(TabAction::Close(index));
                        continue;
                    }
                }
                if response.middle_clicked() {
                    action = Some(TabAction::Close(index));
                } else if response.clicked() {
                    action = Some(TabAction::Select(index));
                }
            }
        });
        ui.add_space(TRACK_PAD);
        // The track behind the tabs, drawn under them.
        let rect = row
            .response
            .rect
            .expand2(egui::vec2(0.0, TRACK_PAD))
            .with_max_x(row.response.rect.right() + TRACK_PAD - spacing);
        ui.painter().set(
            track,
            egui::epaint::RectShape::new(
                rect,
                egui::CornerRadius::same(theme::radius::MD + 2),
                p.subtle,
                egui::Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            ),
        );
        let result = match action {
            Some(TabAction::Select(index)) => self.select_tab(index),
            Some(TabAction::Close(index)) => self.close_tab(index),
            None => Ok(()),
        };
        self.report(result);
    }
}
