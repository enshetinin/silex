//! «Comparar con la actual» (H12.10): a version of the page next to the
//! page now, in the history window. Added blocks are green, removed ones
//! red and crossed out, changed ones have an amber bar and their words
//! marked. What changed is worked out in `silex_desktop::versions`.
use eframe::egui;
use silex_desktop::model::{Block, BlockKind, Page, list_numbers};
use silex_desktop::versions::{BlockChange, BlockDiff, Change, Comparison, Pieces};

use super::blocks::{heading_space, paint_check_box};
use super::theme::{self, Palette};
use super::*;

/// Behind added or removed text and blocks: the color, faint.
fn tint(color: egui::Color32) -> egui::Color32 {
    color.linear_multiply(0.16)
}

/// The comparison, read-only: a summary, the title and every block.
pub(super) fn comparison_view(ui: &mut egui::Ui, comparison: &Comparison, pages: &[Page]) {
    let p = theme::current(ui.ctx());
    ui.label(egui::RichText::new(comparison.describe()).color(p.text_muted));
    if !comparison.is_same() {
        legend(ui, p);
    }
    ui.add_space(theme::space::SM);
    let title_font = egui::TextStyle::Name(theme::text::TITLE.into()).resolve(ui.style());
    let base = base_format(title_font, p.text);
    let job = if comparison.title.iter().all(|(_, t)| t.is_empty()) {
        egui::text::LayoutJob::single_section(display_title("").to_owned(), base)
    } else {
        pieces_job(&comparison.title, p, base)
    };
    ui.add(egui::Label::new(job).wrap());
    ui.add_space(theme::space::SM);
    let shown: Vec<Block> = comparison.blocks.iter().map(|d| d.block.clone()).collect();
    let numbers = list_numbers(&shown);
    for (index, (diff, number)) in comparison.blocks.iter().zip(numbers).enumerate() {
        if index > 0 {
            ui.add_space(heading_space(diff.block.kind));
        }
        diff_row(ui, diff, number, pages, p);
    }
}

/// Three swatches: what each color means.
fn legend(ui: &mut egui::Ui, p: &Palette) {
    ui.horizontal(|ui| {
        for (color, word) in [
            (p.success, "Añadido"),
            (p.danger, "Quitado"),
            (p.warning, "Cambiado"),
        ] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(2), color);
            ui.label(egui::RichText::new(word).small().color(p.text_muted));
            ui.add_space(theme::space::SM);
        }
    });
}

/// The font and color text starts from.
fn base_format(font_id: egui::FontId, color: egui::Color32) -> egui::TextFormat {
    egui::TextFormat {
        font_id,
        color,
        // As `ui.label` does: centered on the line, like the markers.
        valign: egui::Align::Center,
        ..Default::default()
    }
}

/// `pieces` as one text: removed words crossed out on red, added ones on
/// green.
fn pieces_job(pieces: &Pieces, p: &Palette, base: egui::TextFormat) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut after_removed = false;
    for (change, text) in pieces {
        let format = match change {
            Change::Removed => egui::TextFormat {
                background: tint(p.danger),
                strikethrough: egui::Stroke::new(1.0, p.danger),
                ..base.clone()
            },
            Change::Added => egui::TextFormat {
                background: tint(p.success),
                ..base.clone()
            },
            Change::Same => base.clone(),
        };
        // A little air between what went and what came in its place.
        let gap = if after_removed && *change == Change::Added {
            4.0
        } else {
            0.0
        };
        after_removed = *change == Change::Removed;
        job.append(text, gap, format);
    }
    job
}

/// What a block says, for its row and the screen reader: its text, or for
/// a card, the file's name or the linked page's title.
fn block_text(block: &Block, pages: &[Page]) -> String {
    match block.kind {
        BlockKind::Divider => "Separador".to_owned(),
        BlockKind::PageLink => block
            .link
            .as_deref()
            .and_then(|id| pages.iter().find(|page| page.id == id))
            .map_or_else(
                || display_title(&block.content).to_owned(),
                |page| display_title(&page.title).to_owned(),
            ),
        _ => block.content.clone(),
    }
}

/// One block: a colored bar and fill, its marker, its text and, when
/// changed, what else changed.
fn diff_row(
    ui: &mut egui::Ui,
    diff: &BlockDiff,
    number: Option<usize>,
    pages: &[Page],
    p: &Palette,
) {
    let block = &diff.block;
    let (bar, fill, word) = match &diff.change {
        BlockChange::Same => (None, egui::Color32::TRANSPARENT, None),
        BlockChange::Added => (Some(p.success), tint(p.success), Some("Añadido")),
        BlockChange::Removed => (Some(p.danger), tint(p.danger), Some("Quitado")),
        BlockChange::Changed { .. } => (
            Some(p.warning),
            egui::Color32::TRANSPARENT,
            Some("Cambiado"),
        ),
    };
    let text = block_text(block, pages);
    let style = match block.kind {
        BlockKind::Heading1 => egui::TextStyle::Name(theme::text::H1.into()),
        BlockKind::Heading2 => egui::TextStyle::Name(theme::text::H2.into()),
        BlockKind::Heading3 => egui::TextStyle::Name(theme::text::H3.into()),
        BlockKind::Code => egui::TextStyle::Monospace,
        _ => egui::TextStyle::Body,
    };
    let font = style.resolve(ui.style());
    let removed = diff.change == BlockChange::Removed;
    let muted = removed || (block.kind == BlockKind::Todo && block.checked);
    let mut base = base_format(font, if muted { p.text_muted } else { p.text });
    base.italics = block.kind == BlockKind::Quote;
    if removed {
        base.strikethrough = egui::Stroke::new(1.0, p.danger);
    }
    let (pieces, notes): (Pieces, &[String]) = match &diff.change {
        BlockChange::Changed { words, notes } if block.kind.is_text() => (words.clone(), notes),
        BlockChange::Changed { notes, .. } => (vec![(Change::Same, text.clone())], notes),
        _ => (vec![(Change::Same, text.clone())], &[]),
    };
    let empty = pieces.iter().all(|(_, t)| t.is_empty());
    let frame = egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(theme::radius::SM))
        .inner_margin(egui::Margin {
            left: 12,
            right: 6,
            top: 2,
            bottom: 2,
        });
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let row = ui.horizontal_wrapped(|ui| {
            ui.add_space(f32::from(block.indent) * 24.0);
            match block.kind {
                BlockKind::Todo => {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    paint_check_box(ui, rect.center(), block.checked, &response);
                }
                BlockKind::Bulleted => {
                    ui.label(egui::RichText::new("•").color(p.text));
                }
                BlockKind::Numbered => {
                    ui.label(
                        egui::RichText::new(format!("{}.", number.unwrap_or(1))).color(p.text),
                    );
                }
                BlockKind::Quote => {
                    // A bar, as in the editor.
                    let height = ui.text_style_height(&egui::TextStyle::Body);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, height), egui::Sense::hover());
                    ui.painter().vline(
                        rect.center().x,
                        rect.y_range(),
                        egui::Stroke::new(3.0, p.border),
                    );
                }
                BlockKind::Toggle => {
                    icons::decoration(ui, icons::CARET_DOWN, theme::icon::MD, p.text_muted);
                }
                BlockKind::Callout => {
                    icons::decoration(ui, icons::LIGHTBULB, theme::icon::MD, p.accent_strong);
                }
                BlockKind::File => {
                    icons::decoration(ui, icons::PAPERCLIP, theme::icon::MD, p.text_muted);
                }
                BlockKind::PageLink => {
                    icons::decoration(ui, icons::PAGE, theme::icon::MD, p.text_muted);
                }
                _ => {}
            }
            if empty {
                ui.label(
                    egui::RichText::new("Bloque vacío")
                        .italics()
                        .color(p.text_faint),
                )
            } else {
                let job = pieces_job(&pieces, p, base);
                ui.add(egui::Label::new(job).wrap())
            }
        });
        if !notes.is_empty() {
            ui.label(
                egui::RichText::new(notes.join(" · "))
                    .small()
                    .color(p.text_muted),
            );
        }
        row.inner
    });
    if let Some(color) = bar {
        let rect = shown.response.rect;
        ui.painter().vline(
            rect.left() + 2.0,
            rect.y_range().shrink(1.0),
            egui::Stroke::new(3.0, color),
        );
    }
    // The screen reader hears what changed, not only the text.
    if let Some(word) = word {
        let mut label = format!("{word}: {}", if empty { "bloque vacío" } else { &text });
        if !notes.is_empty() {
            label.push_str(&format!(" ({})", notes.join(", ")));
        }
        ui.ctx().accesskit_node_builder(shown.inner.id, |node| {
            node.set_label(label);
        });
    }
}
