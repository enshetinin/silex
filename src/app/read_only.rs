//! A page drawn read-only, as the editor lays it out (block kinds, checks,
//! numbers, files and links) but with nothing to edit: the version shown
//! in the history (H7.8) and the page beside (H8.2).
use eframe::egui;
use silex_desktop::model::{Block, BlockKind, Page, PageKind, list_numbers};

use super::*;
use super::{icons, theme};

/// What a click in a read-only page asks for (only when `interactive`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReadOnlyAction {
    /// A link's page.
    OpenPage(String),
    /// A file block's attachment.
    OpenFile(String),
}

/// `title` and `blocks`, read-only. With `interactive`, links and files
/// can be clicked; `empty` is said when there are no blocks.
pub(super) fn read_only_page(
    ui: &mut egui::Ui,
    title: &str,
    blocks: &[Block],
    pages: &[Page],
    interactive: bool,
    empty: &str,
) -> Option<ReadOnlyAction> {
    let p = theme::current(ui.ctx());
    ui.label(
        egui::RichText::new(display_title(title))
            .text_style(egui::TextStyle::Name(theme::text::TITLE.into()))
            .color(p.text),
    );
    ui.add_space(theme::space::SM);
    if blocks.is_empty() {
        ui.label(egui::RichText::new(empty).color(p.text_muted));
    }
    let mut action = None;
    let numbers = list_numbers(blocks);
    for (index, (block, number)) in blocks.iter().zip(numbers).enumerate() {
        if index > 0 {
            ui.add_space(heading_space(block.kind));
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(f32::from(block.indent) * 24.0);
            if let Some(chosen) = read_only_block(ui, block, number, pages, interactive) {
                action = Some(chosen);
            }
        });
    }
    action
}

fn read_only_block(
    ui: &mut egui::Ui,
    block: &Block,
    number: Option<usize>,
    pages: &[Page],
    interactive: bool,
) -> Option<ReadOnlyAction> {
    let p = theme::current(ui.ctx());
    let text = block.content.as_str();
    let styled = |style: &str| {
        egui::RichText::new(text)
            .text_style(egui::TextStyle::Name(style.into()))
            .color(p.text)
    };
    match block.kind {
        BlockKind::Paragraph => {
            ui.label(egui::RichText::new(text).color(p.text));
        }
        BlockKind::Heading1 => {
            ui.label(styled(theme::text::H1));
        }
        BlockKind::Heading2 => {
            ui.label(styled(theme::text::H2));
        }
        BlockKind::Heading3 => {
            ui.label(styled(theme::text::H3));
        }
        BlockKind::Todo => {
            // The editor's box, read-only: named for the screen reader.
            let checked = block.checked;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::CheckBox, false, checked, text)
            });
            paint_check_box(ui, rect.center(), checked, &response);
            let color = if checked { p.text_muted } else { p.text };
            let mut label = egui::RichText::new(text).color(color);
            if checked {
                label = label.strikethrough();
            }
            ui.label(label);
        }
        BlockKind::Bulleted => {
            ui.label(egui::RichText::new(format!("•  {text}")).color(p.text));
        }
        BlockKind::Numbered => {
            let n = number.unwrap_or(1);
            ui.label(egui::RichText::new(format!("{n}.  {text}")).color(p.text));
        }
        BlockKind::Quote => {
            // A bar on the left, as in the editor.
            ui.add_space(theme::space::MD);
            let quote = ui.label(egui::RichText::new(text).italics().color(p.text_muted));
            let x = quote.rect.left() - theme::space::SM;
            ui.painter()
                .vline(x, quote.rect.y_range(), egui::Stroke::new(3.0, p.border));
        }
        BlockKind::Code => {
            egui::Frame::new()
                .fill(p.subtle)
                .corner_radius(egui::CornerRadius::same(theme::radius::SM))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).monospace().color(p.text));
                });
        }
        BlockKind::File => {
            ui.label(icons::icon(icons::PAPERCLIP).color(p.text_muted));
            match (&block.attachment, interactive) {
                (Some(file), true) => {
                    if ui
                        .link(egui::RichText::new(text).color(p.text))
                        .on_hover_text("Abrir")
                        .clicked()
                    {
                        return Some(ReadOnlyAction::OpenFile(file.clone()));
                    }
                }
                _ => {
                    ui.label(egui::RichText::new(text).color(p.text));
                }
            }
        }
        BlockKind::PageLink => {
            // The page's current title, as its card shows it.
            let live = block
                .link
                .as_deref()
                .and_then(|id| pages.iter().find(|page| page.id == id));
            let (icon, title) = match live {
                Some(page) if page.kind == PageKind::Database => {
                    (icons::TABLE, display_title(&page.title).to_owned())
                }
                Some(page) => (icons::PAGE, display_title(&page.title).to_owned()),
                None => (icons::LINK, display_title(text).to_owned()),
            };
            ui.label(icons::icon(icon).color(p.text_muted));
            match (live, interactive) {
                (Some(page), true) => {
                    if ui.link(egui::RichText::new(title).color(p.text)).clicked() {
                        return Some(ReadOnlyAction::OpenPage(page.id.clone()));
                    }
                }
                _ => {
                    ui.label(egui::RichText::new(title).underline().color(p.text));
                }
            }
        }
    }
    None
}
