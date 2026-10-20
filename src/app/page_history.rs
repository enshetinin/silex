//! «Historial de versiones» (H7.8, ADR 0009): the versions of the open
//! page, one shown read-only, and restoring it as an undoable change.
use std::time::Instant;

use anyhow::{Context as _, Result};
use eframe::egui;
use silex_desktop::history::{EditKind, Snapshot};
use silex_desktop::model::{Block, BlockKind, Page, PageKind, list_numbers};
use silex_desktop::versions::{VersionBlocks, VersionInfo, describe_when};

use super::*;
use super::{icons, theme};

/// Width of the list of versions.
const LIST_WIDTH: f32 = 250.0;
/// Height of the list and of the version shown.
const PANE_HEIGHT: f32 = 420.0;

/// The history window of one page, while open.
pub(super) struct PageHistory {
    page: String,
    pub(super) versions: Vec<VersionInfo>,
    /// Local date today, for «Hoy» and «Ayer».
    pub(super) today: String,
    pub(super) selected: usize,
    /// The blocks of the selected version (read when it is chosen).
    preview: Option<(i64, VersionBlocks)>,
}

impl PageHistory {
    fn selected(&self) -> Option<&VersionInfo> {
        self.versions.get(self.selected)
    }

    fn when(&self, version: &VersionInfo) -> String {
        describe_when(&version.local, &self.today)
    }
}

/// One version in the list: when, and its title if it was different.
fn version_row(
    ui: &mut egui::Ui,
    when: &str,
    note: Option<&str>,
    selected: bool,
) -> egui::Response {
    let p = theme::current(ui.ctx());
    let height = if note.is_some() { 44.0 } else { 32.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, when));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    if selected {
        ui.painter().rect_filled(rect, radius, p.accent_soft);
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, radius, p.subtle);
    }
    focus_ring(ui, rect, &response);
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let left = rect.left() + theme::space::SM;
    match note {
        Some(note) => {
            ui.painter().text(
                egui::pos2(left, rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                when,
                body,
                p.text,
            );
            let mut job =
                egui::text::LayoutJob::simple_singleline(note.to_owned(), small, p.text_muted);
            job.wrap = egui::text::TextWrapping::truncate_at_width(
                (rect.width() - 2.0 * theme::space::SM).max(0.0),
            );
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(
                egui::pos2(left, rect.bottom() - 6.0 - galley.size().y),
                galley,
                p.text_muted,
            );
        }
        None => {
            ui.painter().text(
                egui::pos2(left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                when,
                body,
                p.text,
            );
        }
    }
    response
}

/// A version's title and blocks, read-only, laid out like the page (block
/// kinds, checks, numbers, files and links) but with nothing to edit.
fn version_preview(ui: &mut egui::Ui, title: &str, blocks: &[Block], pages: &[Page]) {
    let p = theme::current(ui.ctx());
    ui.label(
        egui::RichText::new(display_title(title))
            .text_style(egui::TextStyle::Name(theme::text::TITLE.into()))
            .color(p.text),
    );
    ui.add_space(theme::space::SM);
    if blocks.is_empty() {
        ui.label(egui::RichText::new("Esta versión no tiene bloques.").color(p.text_muted));
    }
    let numbers = list_numbers(blocks);
    for (index, (block, number)) in blocks.iter().zip(numbers).enumerate() {
        if index > 0 {
            ui.add_space(heading_space(block.kind));
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(f32::from(block.indent) * 24.0);
            preview_block(ui, block, number, pages);
        });
    }
}

fn preview_block(ui: &mut egui::Ui, block: &Block, number: Option<usize>, pages: &[Page]) {
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
            ui.label(egui::RichText::new(text).color(p.text));
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
            ui.label(egui::RichText::new(title).underline().color(p.text));
        }
    }
}

impl SilexApp {
    /// Opens «Historial de versiones» for the open page (from its «…»
    /// menu or the palette). Pending edits are saved first, so the list is
    /// up to date.
    pub(super) fn open_page_history(&mut self) -> Result<()> {
        let page = match &self.selected {
            Some(id) if self.page_shows_blocks(id) => id.clone(),
            _ => {
                self.notice =
                    Some("Abre una página (no una base de datos) para ver su historial.".into());
                return Ok(());
            }
        };
        // A failed save is shown by the status bar; the history still opens.
        let _ = self.save_now();
        let versions = self.db.page_versions(&page)?;
        let today = self.db.local_today()?;
        self.slash = None;
        self.page_history = Some(PageHistory {
            page,
            versions,
            today,
            selected: 0,
            preview: None,
        });
        Ok(())
    }

    /// Whether `id` is a page that shows blocks (not a database's table).
    pub(super) fn page_shows_blocks(&self, id: &str) -> bool {
        self.find_page(id).is_some_and(|p| p.kind == PageKind::Page)
    }

    /// The history window: versions on the left (newest first), the chosen
    /// one on the right. ↑ ↓ choose, Intro restores, Esc closes.
    pub(super) fn page_history_window(&mut self, ctx: &egui::Context) {
        let Some(mut history) = self.page_history.take() else {
            return;
        };
        if self.selected.as_deref() != Some(history.page.as_str()) {
            // Another page was opened meanwhile.
            return;
        }
        use egui::{Key, Modifiers as M};
        let last = history.versions.len().saturating_sub(1);
        if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
            history.selected = (history.selected + 1).min(last);
        }
        if take_exact_key(ctx, Key::ArrowUp, M::NONE) {
            history.selected = history.selected.saturating_sub(1);
        }
        history.selected = history.selected.min(last);
        let mut restore = take_exact_key(ctx, Key::Enter, M::NONE);
        let mut close = take_exact_key(ctx, Key::Escape, M::NONE);

        // Read the chosen version's blocks once.
        let chosen = history.selected().map(|v| v.id);
        if history.preview.as_ref().map(|(id, _)| *id) != chosen {
            history.preview = match chosen {
                Some(id) => match self.db.version_blocks(id) {
                    Ok(Some(blocks)) => Some((id, blocks)),
                    Ok(None) => Some((id, Err("Esta versión ya no existe.".into()))),
                    Err(err) => Some((id, Err(format!("{err:#}")))),
                },
                None => None,
            };
        }
        let readable = matches!(history.preview, Some((_, Ok(_))));
        restore &= readable;

        let p = theme::current(ctx);
        let pages = &self.pages;
        let current_title = self.title.clone();
        let response = egui::Modal::new(egui::Id::unique("silex_page_history")).show(ctx, |ui| {
            ui.set_width(820.0);
            ui.heading("Historial de versiones");
            ui.add_space(theme::space::XS);
            ui.label(
                egui::RichText::new(
                    "Silex guarda cómo estaba la página, como mucho cada 10 minutos de edición.",
                )
                .color(p.text_muted),
            );
            ui.add_space(theme::space::MD);
            if history.versions.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Todavía no hay versiones. Silex guarda una al editar, \
                         como mucho cada 10 minutos.",
                    )
                    .color(p.text_muted),
                );
            } else {
                // A fixed height: inside a modal, anything that takes the
                // available height (the scroll areas, the separator) would
                // otherwise make the window taller every frame.
                let size = egui::vec2(ui.available_width(), PANE_HEIGHT);
                let layout = egui::Layout::left_to_right(egui::Align::Min);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    ui.set_height(PANE_HEIGHT);
                    ui.vertical(|ui| {
                        ui.set_width(LIST_WIDTH);
                        ui.set_height(PANE_HEIGHT);
                        egui::ScrollArea::vertical()
                            .id_salt("silex_history_list")
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                for (i, version) in history.versions.iter().enumerate() {
                                    let when = history.when(version);
                                    let note = (version.title != current_title)
                                        .then(|| display_title(&version.title).to_owned());
                                    let row = version_row(
                                        ui,
                                        &when,
                                        note.as_deref(),
                                        i == history.selected,
                                    );
                                    if i == history.selected {
                                        row.scroll_to_me(None);
                                    }
                                    if row.clicked() {
                                        history.selected = i;
                                    }
                                    if row.double_clicked() {
                                        restore = readable;
                                    }
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(PANE_HEIGHT);
                        egui::ScrollArea::vertical()
                            .id_salt("silex_history_preview")
                            .auto_shrink(false)
                            .show(ui, |ui| match (&history.preview, history.selected()) {
                                (Some((_, Ok(blocks))), Some(version)) => {
                                    version_preview(ui, &version.title, blocks, pages);
                                }
                                (Some((_, Err(why))), _) => {
                                    ui.label(egui::RichText::new(why.as_str()).color(p.text_muted));
                                }
                                _ => {}
                            });
                    });
                });
            }
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                if !history.versions.is_empty()
                    && ui
                        .add_enabled(readable, egui::Button::new("Restaurar esta versión"))
                        .clicked()
                {
                    restore = true;
                }
                if ui.button("Cerrar").clicked() {
                    close = true;
                }
            });
        });
        if restore && let Some(version) = history.selected().cloned() {
            let when = history.when(&version);
            let blocks = match history.preview.take() {
                Some((_, Ok(blocks))) => blocks,
                _ => return,
            };
            let result = self.restore_version(&version, blocks, &when, ctx);
            self.report(result);
            return;
        }
        if !close && !response.should_close() {
            self.page_history = Some(history);
        }
    }

    /// Puts a version back as the open page's title and blocks. The page
    /// as it is now is saved as a version first, and Cmd+Z undoes it.
    fn restore_version(
        &mut self,
        version: &VersionInfo,
        blocks: Vec<Block>,
        when: &str,
        ctx: &egui::Context,
    ) -> Result<()> {
        let page = version.page_id.clone();
        if self.selected.as_deref() != Some(page.as_str()) {
            return Ok(());
        }
        self.save_now()
            .context("No se restauró la versión: no se pudieron guardar los cambios")?;
        self.db.save_version(&page)?;
        let blocks = self.db.restorable_blocks(&page, blocks)?;
        let current = self.snapshot();
        let target = Snapshot {
            title: version.title.clone(),
            blocks,
        };
        if target == current {
            self.notice = Some("La página ya está como en esa versión.".into());
            return Ok(());
        }
        self.history
            .record(current.clone(), EditKind::Other, Instant::now());
        self.apply_snapshot(target, &current);
        // Saved now, so its files and links are read from the database.
        self.save_now()?;
        self.load_attachments(&page)?;
        self.load_links()?;
        self.notice = Some(format!(
            "Se restauró la versión de {}. {}+Z la deshace.",
            lowercase_first(when),
            cmd_label(ctx)
        ));
        Ok(())
    }
}

/// «Hoy, 17:40» → «hoy, 17:40», for the middle of a sentence.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// «Recetas de otoño», edited on three days (fixed clock, from 18 to 20
/// October 2026): three versions, newest first 20 oct 12:00 (with a link
/// and a quote), 19 oct 09:05 (a list and a check) and 18 oct 17:40 (a
/// heading and a paragraph). Now it is «Recetas», with one paragraph.
/// Returns the app (with the history not open) and the page.
#[cfg(test)]
pub(super) fn history_fixture() -> Result<(SilexApp, String)> {
    use silex_desktop::db::Database;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Recetas de otoño", None)?;
    let mercado = db.create_page("Mercado de San Miguel", None)?;
    let block = |id: &str, kind: BlockKind, content: &str| Block {
        id: id.into(),
        page_id: page.clone(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
    };
    let first = vec![
        block("h", BlockKind::Heading1, "Sopas"),
        block(
            "p",
            BlockKind::Paragraph,
            "Calabaza asada con jengibre y un poco de comino.",
        ),
    ];
    let mut second = first.clone();
    second.extend([
        Block {
            checked: true,
            ..block("t", BlockKind::Todo, "Comprar calabaza")
        },
        block("t2", BlockKind::Todo, "Probar con leche de coco"),
        block("b1", BlockKind::Bulleted, "Jengibre fresco"),
        Block {
            indent: 1,
            ..block("b2", BlockKind::Bulleted, "Rallado fino")
        },
        block("n1", BlockKind::Numbered, "Asar la calabaza 40 minutos"),
        block("n2", BlockKind::Numbered, "Triturar con el caldo"),
    ]);
    let mut third = second.clone();
    third.extend([
        block("q", BlockKind::Quote, "Mejor al día siguiente."),
        Block {
            link: Some(mercado.clone()),
            ..block("l", BlockKind::PageLink, "")
        },
    ]);
    let now = vec![block(
        "p",
        BlockKind::Paragraph,
        "Solo queda la idea de la sopa.",
    )];
    for (at, title, blocks) in [
        ("2026-10-18T09:00:00.000Z", "Recetas de otoño", &first),
        ("2026-10-18T17:40:00.000Z", "Recetas de otoño", &second),
        ("2026-10-19T09:05:00.000Z", "Recetas de otoño", &third),
        ("2026-10-20T12:00:00.000Z", "Recetas", &now),
    ] {
        db.set_clock(Some(at));
        db.save_document(&page, title, blocks)?;
    }
    let mut app = SilexApp::new(db)?;
    app.open_page(page.clone())?;
    Ok((app, page))
}

/// Opens the history as on 20 October 2026, with UTC times (the system
/// time zone would change what the tests see).
#[cfg(test)]
pub(super) fn open_fixed_history(app: &mut SilexApp) -> Result<()> {
    app.open_page_history()?;
    if let Some(history) = &mut app.page_history {
        history.today = "2026-10-20".into();
        for version in &mut history.versions {
            version.local = version
                .created_at
                .replace('T', " ")
                .chars()
                .take(16)
                .collect();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_start_lowercase() {
        assert_eq!(lowercase_first("Hoy, 17:40"), "hoy, 17:40");
        assert_eq!(lowercase_first("18 oct, 17:40"), "18 oct, 17:40");
        assert_eq!(lowercase_first(""), "");
    }
}
