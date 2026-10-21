//! Search window (Cmd+K): commands (H5.8) and pages (H5.7). Pages come
//! ranked, with their path and a snippet of the matching block; searches
//! run on a worker thread (`SearchWorker`) so typing never waits for
//! SQLite. Commands are matched inline (`palette::matching_commands`).
use eframe::egui::{self, TextEdit, text::LayoutJob};
use silex_desktop::{
    editor,
    model::ancestors,
    palette::{Command, matching_commands},
    search::{SearchHit, SearchWorker, TextPart},
};

use super::palette::{command_icon, command_shortcut};
use super::*;
use super::{icons, theme};

pub(super) fn search_field_id() -> egui::Id {
    egui::Id::unique("silex-search")
}

/// Width of the window on a wide screen.
const WIDTH: f32 = 640.0;
/// Distance from the top of the window.
const TOP: f32 = 72.0;
/// Space above and below the text of a result row.
const ROW_PADDING: f32 = 6.0;
/// Height of a command row.
const COMMAND_ROW: f32 = 32.0;

pub(super) struct SearchWindow {
    pub(super) query: String,
    /// Opened with Cmd+T: the page chosen opens in a new tab (ADR 0011).
    pub(super) new_tab: bool,
    /// The query last sent to the search (its results may still be coming).
    requested: String,
    /// Commands matching the query, listed before the pages.
    pub(super) commands: Vec<Command>,
    pub(super) results: Vec<SearchHit>,
    /// Highlighted entry (↑/↓) in commands, then pages; Enter chooses it.
    pub(super) selected: usize,
    pub(super) error: Option<String>,
    /// `None` for an in-memory database (tests): searches run inline.
    worker: Option<SearchWorker>,
    /// Focus the text field on the next frame.
    focus: bool,
    /// ↑/↓ moved the highlight: scroll it into view.
    scroll_to_selected: bool,
    /// An IME composition is in progress in the field.
    composing: bool,
    /// Enter was pressed: choose the highlighted entry once the entries
    /// for what is in the field are there (never an older query's).
    open_when_ready: bool,
}

/// What an entry of the window is.
enum Entry {
    Command(Command),
    Page(SearchHit),
}

impl SearchWindow {
    fn len(&self) -> usize {
        self.commands.len() + self.results.len()
    }

    fn entry(&self, index: usize) -> Option<Entry> {
        match index.checked_sub(self.commands.len()) {
            None => self.commands.get(index).copied().map(Entry::Command),
            Some(page) => self.results.get(page).cloned().map(Entry::Page),
        }
    }
}

impl SilexApp {
    /// Opens the search window. Pending edits are saved first, so the
    /// search sees what is on screen.
    pub(super) fn open_search(&mut self, ctx: &egui::Context) {
        // A save error is already shown by the status bar.
        let _ = self.save_now();
        // Keys go to the window now; a block selection must not see them.
        self.selection = None;
        self.slash = None;
        let commands = matching_commands("", &self.palette_context());
        let repaint = ctx.clone();
        // Without a file (tests) or a thread, search inline.
        let worker = self
            .db
            .path()
            .and_then(|path| SearchWorker::spawn(path, move || repaint.request_repaint()).ok());
        self.search = Some(SearchWindow {
            query: String::new(),
            requested: String::new(),
            commands,
            results: Vec::new(),
            selected: 0,
            error: None,
            worker,
            focus: true,
            scroll_to_selected: false,
            composing: false,
            open_when_ready: false,
            new_tab: false,
        });
    }

    /// Sends the query when it changed and collects results.
    fn refresh_search(&mut self) {
        let palette = self.palette_context();
        let Some(window) = self.search.as_mut() else {
            return;
        };
        if window.query != window.requested {
            window.requested = window.query.clone();
            window.commands = matching_commands(&window.query, &palette);
            window.selected = 0;
            window.error = None;
            if window.query.trim().is_empty() {
                window.results.clear();
                if let Some(worker) = window.worker.as_mut() {
                    worker.cancel();
                }
            } else if let Some(worker) = window.worker.as_mut() {
                worker.request(window.query.clone());
            } else {
                match self.db.search(&window.query) {
                    Ok(hits) => window.results = hits,
                    Err(err) => window.error = Some(format!("Error de búsqueda: {err:#}")),
                }
            }
        }
        if let Some(result) = window.worker.as_mut().and_then(SearchWorker::poll) {
            match result {
                Ok(hits) => window.results = hits,
                Err(err) => window.error = Some(format!("Error de búsqueda: {err:#}")),
            }
            window.selected = window.selected.min(window.len().saturating_sub(1));
        }
    }

    /// The entry Enter asked for, once the entries match the field. Commands
    /// are matched at once; pages may still be on their way.
    fn ready_to_choose(window: &mut SearchWindow) -> Option<usize> {
        if !window.open_when_ready || window.query != window.requested {
            return None;
        }
        let on_page = window.selected >= window.commands.len();
        if on_page && window.worker.as_ref().is_some_and(SearchWorker::is_pending) {
            return None;
        }
        window.open_when_ready = false;
        (window.selected < window.len()).then_some(window.selected)
    }

    /// "Padre › Hija" for a page (empty at the root).
    fn page_path(&self, id: &str) -> String {
        ancestors(&self.pages, id)
            .iter()
            .rev()
            .filter_map(|a| self.pages.iter().find(|p| &p.id == a))
            .map(|p| display_title(&p.title))
            .collect::<Vec<_>>()
            .join(" › ")
    }

    /// Opens a result. A content match puts the cursor on the match.
    fn open_search_hit(&mut self, hit: &SearchHit, new_tab: bool) -> Result<()> {
        self.open_page_in(hit.page.id.clone(), new_tab)?;
        if self.selected.as_deref() != Some(hit.page.id.as_str()) {
            anyhow::bail!("La página ya no existe o está en la papelera");
        }
        // Only a block that is on the page: a focus request for a missing
        // field would never be fulfilled.
        if let Some(found) = &hit.block
            && let Some(block) = self.blocks.iter().find(|b| b.id == found.block_id)
        {
            let at = found.char_offset.min(editor::block_char_len(block));
            self.pending_focus = Some((block.id.clone(), at));
        }
        Ok(())
    }

    /// ↑ ↓ Enter in the window, taken before its text field sees them.
    /// Nothing is taken during an IME composition (Enter confirms it).
    /// Returns whether Enter was pressed.
    fn search_keys(window: &mut SearchWindow, ctx: &egui::Context) -> bool {
        use egui::{Key, Modifiers as M};
        let was_composing = window.composing;
        window.composing = ime_composing_after(ctx, window.composing);
        if was_composing || window.composing {
            // The IME sends the text as a commit; the key must not also
            // leave the field.
            take_exact_key(ctx, Key::Enter, M::NONE);
            return false;
        }
        let last = window.len().saturating_sub(1);
        if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
            window.selected = (window.selected + 1).min(last);
            window.scroll_to_selected = true;
        }
        if take_exact_key(ctx, Key::ArrowUp, M::NONE) {
            window.selected = window.selected.saturating_sub(1);
            window.scroll_to_selected = true;
        }
        take_exact_key(ctx, Key::Enter, M::NONE)
    }

    /// The search window, when open: field, results and a key hint.
    pub(super) fn search_window(&mut self, ctx: &egui::Context) {
        if self.search.is_none() {
            return;
        }
        self.refresh_search();
        let Some(mut window) = self.search.take() else {
            return;
        };
        let p = theme::current(ctx);
        if Self::search_keys(&mut window, ctx) {
            window.open_when_ready = true;
        }
        let mut open = None;
        let paths: Vec<String> = window
            .results
            .iter()
            .map(|hit| self.page_path(&hit.page.id))
            .collect();
        let pending = window.worker.as_ref().is_some_and(SearchWorker::is_pending);

        let id = egui::Id::unique("silex_search_window");
        let width = (ctx.content_rect().width() - 2.0 * theme::space::XL).clamp(240.0, WIDTH);
        let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, [0.0, TOP]);
        let frame = egui::Frame::popup(&ctx.global_style())
            .fill(p.surface)
            .inner_margin(egui::Margin::ZERO)
            .corner_radius(theme::radius::LG);
        let modal = egui::Modal::new(id)
            .area(area)
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            icons::decoration(ui, icons::SEARCH, theme::icon::LG, p.text_muted);
                            let esc_width = 32.0;
                            let field = ui
                                .add(
                                    TextEdit::singleline(&mut window.query)
                                        .id(search_field_id())
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .font(egui::FontId::proportional(16.0))
                                        // As tall as the row, its text centred:
                                        // on the line of the icon and «Esc».
                                        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                                        .align(egui::Align2::LEFT_CENTER)
                                        .hint_text(
                                            egui::RichText::new("Buscar páginas o acciones")
                                                .color(p.text_faint),
                                        )
                                        .desired_width(ui.available_width() - esc_width),
                                )
                                .accessible_name("Buscar páginas o acciones");
                            // Typing always goes to the field, unless Tab moved
                            // the focus to a result.
                            let nothing_focused = ui.memory(|m| m.focused().is_none());
                            if std::mem::take(&mut window.focus) || nothing_focused {
                                field.request_focus();
                            }
                            ui.label(egui::RichText::new("Esc").small().color(p.text_muted));
                        });
                    });
                divider(ui, p);

                egui::ScrollArea::vertical()
                    .id_salt("silex_search_results")
                    .max_height(400.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add_space(theme::space::XS);
                        let scroll = std::mem::take(&mut window.scroll_to_selected);
                        let cmd = cmd_label(ui.ctx());
                        if !window.commands.is_empty() {
                            section(ui, p, "Acciones");
                        }
                        for (index, command) in window.commands.iter().enumerate() {
                            let selected = index == window.selected;
                            let shortcut = command_shortcut(*command, cmd);
                            let row = command_row(ui, p, *command, shortcut.as_deref(), selected);
                            if row.clicked() {
                                open = Some(index);
                            }
                            if selected && scroll {
                                row.scroll_to_me(None);
                            }
                        }
                        if let Some(error) = &window.error {
                            message(ui, error, p.danger);
                        } else if !window.results.is_empty() {
                            section(ui, p, "Páginas");
                        } else if !window.query.trim().is_empty()
                            && window.commands.is_empty()
                            && !pending
                        {
                            message(
                                ui,
                                &format!("Nada coincide con «{}».", window.query.trim()),
                                p.text_muted,
                            );
                        }
                        let first_page = window.commands.len();
                        for (offset, (hit, path)) in window.results.iter().zip(&paths).enumerate() {
                            let index = first_page + offset;
                            let selected = index == window.selected;
                            let row = result_row(ui, p, hit, path, selected);
                            if row.clicked() {
                                open = Some(index);
                            }
                            if selected && scroll {
                                row.scroll_to_me(None);
                            }
                        }
                        ui.add_space(theme::space::XS);
                    });
                divider(ui, p);

                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    "↑ ↓ para moverte · Intro para elegir · Esc para cerrar",
                                )
                                .small()
                                .color(p.text_muted),
                            );
                            if pending {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new("Buscando…")
                                                .small()
                                                .color(p.text_muted),
                                        );
                                    },
                                );
                            }
                        });
                    });
            });

        if window.query != window.requested {
            // Search now for what was just typed; show it next frame.
            self.search = Some(window);
            self.refresh_search();
            ctx.request_repaint();
            let Some(sent) = self.search.take() else {
                return;
            };
            window = sent;
        }
        // A click chooses what is shown; Enter waits for current entries.
        let open = open.or_else(|| Self::ready_to_choose(&mut window));
        let chosen = open.and_then(|i| window.entry(i));
        if chosen.is_none() && !modal.should_close() {
            self.search = Some(window);
            return;
        }
        // Closing: the worker thread ends with the window.
        let result = match chosen {
            Some(Entry::Page(hit)) => self.open_search_hit(&hit, window.new_tab),
            Some(Entry::Command(command)) => self.run_command(ctx, command),
            None => Ok(()),
        };
        self.report(result);
    }
}

/// Small muted title above a group of entries.
fn section(ui: &mut egui::Ui, p: &theme::Palette, title: &str) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 6,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(title).small().color(p.text_muted));
        });
}

/// Background, focus outline and accessible name shared by all rows.
fn row_frame(
    ui: &mut egui::Ui,
    p: &theme::Palette,
    height: f32,
    selected: bool,
    label: &str,
) -> (egui::Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let row = rect.shrink2(egui::vec2(theme::space::XS + 2.0, 0.0));
    let fill = if selected {
        p.accent_soft
    } else if response.hovered() || response.has_focus() {
        p.subtle
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(row, egui::CornerRadius::same(theme::radius::MD), fill);
    if response.has_focus() {
        ui.painter().rect_stroke(
            row.shrink(0.5),
            egui::CornerRadius::same(theme::radius::MD),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    (row, response)
}

/// One command: icon, label and its keyboard shortcut at the right.
fn command_row(
    ui: &mut egui::Ui,
    p: &theme::Palette,
    command: Command,
    shortcut: Option<&str>,
    selected: bool,
) -> egui::Response {
    let label = match shortcut {
        Some(keys) => format!("{} ({keys})", command.label()),
        None => command.label().to_owned(),
    };
    let (row, response) = row_frame(ui, p, COMMAND_ROW, selected, &label);
    let painter = ui.painter_at(row);
    let left = row.left() + theme::space::SM;
    let cy = row.center().y;
    painter.text(
        egui::pos2(left, cy),
        egui::Align2::LEFT_CENTER,
        command_icon(command),
        icons::font(theme::icon::MD),
        if selected {
            p.accent_strong
        } else {
            p.text_muted
        },
    );
    let mut right = row.right() - theme::space::SM;
    if let Some(keys) = shortcut {
        let galley = painter.layout_no_wrap(
            keys.to_owned(),
            egui::FontId::proportional(12.0),
            p.text_muted,
        );
        right -= galley.size().x;
        painter.galley(
            egui::pos2(right, cy - galley.size().y / 2.0),
            galley,
            p.text_muted,
        );
        right -= theme::space::MD;
    }
    let text_left = left + 26.0;
    let mut job = LayoutJob::simple_singleline(
        command.label().to_owned(),
        egui::FontId::proportional(14.0),
        p.text,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - text_left).max(0.0));
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(text_left, cy - galley.size().y / 2.0),
        galley,
        p.text,
    );
    response
}

/// Thin line across the window.
fn divider(ui: &mut egui::Ui, p: &theme::Palette) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, (1.0, p.border));
}

/// A muted (or error) line in the results area.
fn message(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
        });
}

/// Text with the matched parts in semibold and the given colors.
fn marked_job(
    parts: &[TextPart],
    size: f32,
    color: egui::Color32,
    matched_color: egui::Color32,
    max_width: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for part in parts {
        let (family, color) = if part.matched {
            (
                egui::FontFamily::Name(theme::SEMIBOLD.into()),
                matched_color,
            )
        } else {
            (egui::FontFamily::Proportional, color)
        };
        job.append(
            &part.text,
            0.0,
            egui::TextFormat::simple(egui::FontId::new(size, family), color),
        );
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    job
}

/// One result: page icon, title with the matched words in semibold, the
/// page's path, and a snippet of the matching block below.
fn result_row(
    ui: &mut egui::Ui,
    p: &theme::Palette,
    hit: &SearchHit,
    path: &str,
    selected: bool,
) -> egui::Response {
    let title_parts: Vec<TextPart> = if hit.page.title.trim().is_empty() {
        vec![TextPart {
            text: display_title("").to_owned(),
            matched: false,
        }]
    } else {
        hit.title.clone()
    };
    let snippet = hit.block.as_ref().map(|b| b.snippet.as_slice());
    let title_line = 20.0;
    let snippet_line = 18.0;
    let height = ROW_PADDING * 2.0 + title_line + snippet.map_or(0.0, |_| snippet_line);
    let label = {
        let title: String = title_parts.iter().map(|t| t.text.as_str()).collect();
        let mut label = title;
        if !path.is_empty() {
            label.push_str(&format!(", en {path}"));
        }
        if let Some(parts) = snippet {
            let text: String = parts.iter().map(|t| t.text.as_str()).collect();
            label.push_str(&format!(": {text}"));
        }
        label
    };
    let (row, response) = row_frame(ui, p, height, selected, &label);
    let painter = ui.painter_at(row);
    let left = row.left() + theme::space::SM;
    let top = row.top() + ROW_PADDING;
    painter.text(
        egui::pos2(left, top + title_line / 2.0),
        egui::Align2::LEFT_CENTER,
        icons::PAGE,
        icons::font(theme::icon::MD),
        if selected {
            p.accent_strong
        } else {
            p.text_muted
        },
    );
    let text_left = left + 26.0;
    let text_width = (row.right() - theme::space::SM - text_left).max(0.0);

    let title =
        ui.fonts_mut(|f| f.layout_job(marked_job(&title_parts, 14.0, p.text, p.text, text_width)));
    let title_size = title.size();
    painter.galley(
        egui::pos2(text_left, top + (title_line - title_size.y) / 2.0),
        title,
        p.text,
    );
    let path_left = text_left + title_size.x + theme::space::SM;
    let path_width = row.right() - theme::space::SM - path_left;
    if !path.is_empty() && path_width > 24.0 {
        let mut job = LayoutJob::simple_singleline(
            path.to_owned(),
            egui::FontId::proportional(12.0),
            p.text_muted,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(path_width);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let y = top + (title_line - galley.size().y) / 2.0;
        painter.galley(egui::pos2(path_left, y), galley, p.text_muted);
    }
    if let Some(parts) = snippet {
        let galley = ui
            .fonts_mut(|f| f.layout_job(marked_job(parts, 13.0, p.text_muted, p.text, text_width)));
        let y = top + title_line + (snippet_line - galley.size().y) / 2.0;
        painter.galley(egui::pos2(text_left, y), galley, p.text_muted);
    }
    response
}
