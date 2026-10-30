//! Links between pages (H7.5, ADR 0008): the link card, opening its page
//! (or offering to restore it from the trash), and the page picker that
//! «[[», «/enlace» and the palette open.
use std::collections::HashMap;

use anyhow::Result;
use eframe::egui;
use silex_desktop::db::LinkTarget;
use silex_desktop::editor::fold;
use silex_desktop::model::{Block, BlockKind, Page, PageKind, ancestors};

use super::*;
use super::{icons, theme};

/// Height of a link card.
const LINK_HEIGHT: f32 = 36.0;
/// Most pages the picker lists.
const PICKER_RESULTS: usize = 30;

/// What a link card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinkState {
    /// A live page: its current title, kind and path («Proyecto › Diseño»).
    Live {
        title: String,
        kind: PageKind,
        path: String,
        /// Its emoji (H12.8).
        icon: Option<String>,
    },
    /// In the trash: opening it offers to restore it.
    Trashed { title: String },
    /// Its page is gone (or unknown).
    Gone,
}

/// The page picker: what is typed, the page highlighted, and where the
/// link goes.
#[derive(Debug, Clone, Default)]
pub(super) struct LinkPicker {
    pub(super) query: String,
    pub(super) selected: usize,
    /// The link goes after this block (else at the end).
    pub(super) after: Option<String>,
    /// «[[» or «/enlace» left this block empty: the link takes its place.
    pub(super) replace: Option<String>,
    /// Not a link: these blocks are moved to the page chosen (ADR 0017).
    pub(super) moving: Option<Vec<String>>,
    /// Focus its field on the next frame.
    pub(super) opening: bool,
}

/// Links of the open page.
#[derive(Default)]
pub(super) struct Links {
    /// The pages its links point at, from the database: what is known of
    /// those in the trash (live ones are read from the page list).
    pub(super) targets: HashMap<String, LinkTarget>,
    pub(super) picker: Option<LinkPicker>,
    /// A page in the trash the user wants to open: asked first.
    pub(super) restore: Option<(String, String)>,
    /// The live pages that link to the open one («Enlazada desde», H7.6).
    pub(super) backlinks: Vec<String>,
}

/// «Proyecto › Diseño»: the titles above a page, root first.
fn page_path(pages: &[Page], id: &str) -> String {
    let mut titles: Vec<&str> = ancestors(pages, id)
        .iter()
        .filter_map(|a| pages.iter().find(|p| p.id == *a))
        .map(|p| display_title(&p.title))
        .collect();
    titles.reverse();
    titles.join(" › ")
}

/// What the card of `block` shows: from the live pages first, else from
/// what the database said about the pages in the trash.
pub(super) fn link_state(
    pages: &[Page],
    targets: &HashMap<String, LinkTarget>,
    icons: &HashMap<String, String>,
    block: &Block,
) -> LinkState {
    let Some(id) = block.link.as_deref() else {
        return LinkState::Gone;
    };
    if let Some(page) = pages.iter().find(|p| p.id == id) {
        return LinkState::Live {
            title: page.title.clone(),
            kind: page.kind,
            path: page_path(pages, id),
            icon: icons.get(id).cloned(),
        };
    }
    match targets.get(id) {
        Some(target) if target.trashed => LinkState::Trashed {
            title: target.title.clone(),
        },
        _ => LinkState::Gone,
    }
}

/// The pages the picker offers for `query`: live pages but `exclude` (the
/// open one), those whose title starts with the query first, then those
/// that contain it; by title within each group. Without accents or case.
pub(super) fn picker_matches<'a>(
    pages: &'a [Page],
    query: &str,
    exclude: Option<&str>,
) -> Vec<&'a Page> {
    let query = fold(query.trim());
    let mut found: Vec<(bool, String, &Page)> = pages
        .iter()
        .filter(|p| Some(p.id.as_str()) != exclude)
        .filter_map(|p| {
            let title = fold(display_title(&p.title));
            let starts = title.starts_with(&query);
            (starts || title.contains(&query)).then_some((!starts, title, p))
        })
        .collect();
    found.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    found
        .into_iter()
        .take(PICKER_RESULTS)
        .map(|(_, _, p)| p)
        .collect()
}

/// A link block: a card with its page's icon, title and path. The card
/// takes the block's focus id, like a file card; a click (or Intro) opens
/// its page.
pub(super) fn link_card(
    ui: &mut egui::Ui,
    id: egui::Id,
    state: &LinkState,
) -> (egui::Response, Option<BlockAction>) {
    let p = theme::current(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), LINK_HEIGHT),
        egui::Sense::hover(),
    );
    let response = ui.interact(rect, id, egui::Sense::click());
    let label = match state {
        LinkState::Live { title, .. } => format!("Enlace a «{}»", display_title(title)),
        LinkState::Trashed { title } => {
            format!("Enlace a «{}», en la papelera", display_title(title))
        }
        LinkState::Gone => "Enlace a una página que ya no existe".to_owned(),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    let radius = egui::CornerRadius::same(theme::radius::MD);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, radius, p.subtle);
    }
    focus_ring(ui, rect, &response);
    let icon = match state {
        LinkState::Live {
            kind: PageKind::Database,
            ..
        } => icons::TABLE,
        LinkState::Gone => icons::LINK,
        _ => icons::PAGE,
    };
    let emoji = match state {
        LinkState::Live { icon, .. } => icon.as_deref(),
        _ => None,
    };
    let muted = !matches!(state, LinkState::Live { .. });
    let at = icons::paint_page_icon(
        ui.painter(),
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        emoji,
        icon,
        theme::icon::MD,
        p.text_muted,
    );
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let (title, note) = match state {
        LinkState::Live { title, path, .. } => (display_title(title).to_owned(), path.clone()),
        LinkState::Trashed { title } => {
            (display_title(title).to_owned(), "en la papelera".to_owned())
        }
        LinkState::Gone => ("Página que ya no existe".to_owned(), String::new()),
    };
    let mut title_format =
        egui::TextFormat::simple(body, if muted { p.text_muted } else { p.text });
    if !muted {
        title_format.underline = egui::Stroke::new(1.0, p.border);
    }
    if matches!(state, LinkState::Trashed { .. }) {
        title_format.strikethrough = egui::Stroke::new(1.0, p.text_muted);
    }
    job.append(&title, 0.0, title_format);
    if !note.is_empty() {
        job.append(
            &format!("   {note}"),
            0.0,
            egui::TextFormat::simple(small, p.text_muted),
        );
    }
    let left = at.right() + theme::space::SM;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left - 8.0).max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), rect.max);
    let at = egui::Align2::LEFT_CENTER
        .align_size_within_rect(galley.size(), text_rect)
        .min;
    ui.painter().galley(at, galley, p.text);
    let action = response.clicked().then_some(BlockAction::OpenLink);
    (response, action)
}

/// The pages of «Enlazada desde» with their paths: those of `ids` still
/// live, by title (without accents or case), then by path.
pub(super) fn backlink_rows<'a>(pages: &'a [Page], ids: &[String]) -> Vec<(&'a Page, String)> {
    let mut rows: Vec<(String, &Page, String)> = ids
        .iter()
        .filter_map(|id| pages.iter().find(|p| p.id == *id))
        .map(|p| (fold(display_title(&p.title)), p, page_path(pages, &p.id)))
        .collect();
    rows.sort_by(|a, b| (&a.0, &a.2).cmp(&(&b.0, &b.2)));
    rows.into_iter().map(|(_, p, path)| (p, path)).collect()
}

/// One page in a list: icon, title and path in gray, the whole width.
/// `role` is a button in the picker and a link under the page.
fn page_row(
    ui: &mut egui::Ui,
    page: &Page,
    emoji: Option<&str>,
    path: &str,
    selected: bool,
    role: egui::Role,
) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let title = display_title(&page.title);
    response.widget_info(|| egui::WidgetInfo::selected(role, true, selected, title));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    if selected {
        ui.painter().rect_filled(rect, radius, p.accent_soft);
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, radius, p.subtle);
    }
    focus_ring(ui, rect, &response);
    let icon = if page.kind == PageKind::Database {
        icons::TABLE
    } else {
        icons::PAGE
    };
    let at = icons::paint_page_icon(
        ui.painter(),
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        emoji,
        icon,
        theme::icon::MD,
        p.text_muted,
    );
    let mut job = egui::text::LayoutJob::default();
    job.append(
        title,
        0.0,
        egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), p.text),
    );
    if !path.is_empty() {
        job.append(
            &format!("   {path}"),
            0.0,
            egui::TextFormat::simple(egui::TextStyle::Small.resolve(ui.style()), p.text_muted),
        );
    }
    let left = at.right() + theme::space::SM;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left - 8.0).max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), rect.max);
    let at = egui::Align2::LEFT_CENTER
        .align_size_within_rect(galley.size(), text_rect)
        .min;
    ui.painter().galley(at, galley, p.text);
    response
}

impl SilexApp {
    /// What the database knows of the pages the open page links to (to
    /// tell those in the trash), and of the pages that link to it.
    pub(super) fn load_links(&mut self) -> Result<()> {
        let Some(page) = &self.selected else {
            self.links.targets.clear();
            self.links.backlinks.clear();
            return Ok(());
        };
        self.links.targets = self
            .db
            .page_link_targets(page)?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        self.links.backlinks = self.db.backlinks(page)?;
        Ok(())
    }

    /// «Enlazada desde»: the live pages that link to the open one, under
    /// its blocks; nothing if there are none. Returns the page clicked.
    pub(super) fn backlinks_section(&self, ui: &mut egui::Ui) -> Option<String> {
        let rows = backlink_rows(&self.pages, &self.links.backlinks);
        if rows.is_empty() {
            return None;
        }
        let p = theme::current(ui.ctx());
        let mut open = None;
        ui.add_space(theme::space::XL);
        ui.horizontal_top(|ui| {
            gutter_space(ui);
            ui.vertical(|ui| {
                ui.separator();
                ui.add_space(theme::space::SM);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Enlazada desde")
                            .text_style(egui::TextStyle::Small)
                            .strong()
                            .color(p.text_muted),
                    );
                    ui.label(
                        egui::RichText::new(rows.len().to_string())
                            .text_style(egui::TextStyle::Small)
                            .color(p.text_faint),
                    );
                });
                ui.add_space(theme::space::XS);
                for (page, path) in &rows {
                    let emoji = self.page_icons.get(&page.id).map(String::as_str);
                    if page_row(ui, page, emoji, path, false, egui::Role::Link)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open = Some(page.id.clone());
                    }
                }
            });
        });
        open
    }

    /// Opens the page a link points at (as the click asks: here, in a new
    /// tab or beside); one in the trash is offered for restoring first.
    pub(super) fn open_link(&mut self, block: &str) -> Result<()> {
        let Some(link) = self.blocks.iter().find(|b| b.id == block) else {
            return Ok(());
        };
        match link_state(&self.pages, &self.links.targets, &self.page_icons, link) {
            LinkState::Live { .. } => {
                if let Some(target) = link.link.clone() {
                    self.open_clicked(target)?;
                }
            }
            LinkState::Trashed { title } => {
                if let Some(target) = link.link.clone() {
                    self.links.restore = Some((target, title));
                }
            }
            LinkState::Gone => {
                self.notice = Some("La página enlazada ya no existe.".into());
            }
        }
        Ok(())
    }

    /// «Restaurar y abrir» for a linked page in the trash.
    pub(super) fn link_restore_dialog(&mut self, ctx: &egui::Context) {
        let Some((target, title)) = self.links.restore.clone() else {
            return;
        };
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("silex_link_restore")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.heading("Página en la papelera");
            ui.add_space(theme::space::SM);
            ui.label(format!(
                "«{}» está en la papelera. ¿Quieres restaurarla y abrirla?",
                short_title(&title)
            ));
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                if ui.button("Restaurar y abrir").clicked() {
                    restore = true;
                }
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
            });
        });
        if restore {
            self.links.restore = None;
            let result = self.restore_from_trash(&target);
            self.report(result);
        } else if cancel || response.should_close() {
            self.links.restore = None;
        }
    }

    /// Opens the page picker; the link goes after `after`, or in place of
    /// `replace` if that block is still empty when a page is chosen.
    pub(super) fn open_link_picker(&mut self, after: Option<String>, replace: Option<String>) {
        if !self.can_hold_blocks() {
            self.notice = Some("Abre una página (no una base de datos) para enlazar otra.".into());
            return;
        }
        let after = after
            .or_else(|| self.caret.as_ref().map(|c| c.block_id.clone()))
            .or_else(|| self.blocks.last().map(|b| b.id.clone()));
        self.links.picker = Some(LinkPicker {
            after,
            replace,
            opening: true,
            ..LinkPicker::default()
        });
        self.slash = None;
    }

    /// Whether the open page shows blocks (not a database's table).
    fn can_hold_blocks(&self) -> bool {
        self.selected
            .as_ref()
            .and_then(|id| self.find_page(id))
            .is_some_and(|p| p.kind == PageKind::Page)
    }

    /// «[[» just typed in the block being edited: the two brackets go and
    /// the page picker opens (H7.5). Runs after layout, like the slash menu.
    pub(super) fn take_double_bracket(&mut self) {
        let Some(caret) = self.caret.clone().filter(|c| !c.selection) else {
            return;
        };
        if self.ime_composing || self.links.picker.is_some() {
            return;
        }
        let Some(index) = self.blocks.iter().position(|b| b.id == caret.block_id) else {
            return;
        };
        let block = &mut self.blocks[index];
        if !block.kind.is_text() || block.kind == BlockKind::Code || caret.char < 2 {
            return;
        }
        let chars: Vec<char> = block.content.chars().collect();
        if chars.get(caret.char - 2..caret.char) != Some(&['[', '['][..]) {
            return;
        }
        block.content = chars[..caret.char - 2]
            .iter()
            .chain(&chars[caret.char..])
            .collect();
        block.marks = silex_desktop::marks::replace(&block.marks, caret.char - 2, caret.char, 0);
        let empty = block.content.trim().is_empty() && block.kind == BlockKind::Paragraph;
        let id = block.id.clone();
        self.mark_dirty();
        self.pending_focus = Some((id.clone(), caret.char - 2));
        self.open_link_picker(Some(id.clone()), empty.then_some(id));
    }

    /// The page picker, a modal window over the page. Typing filters by
    /// title; ↑ ↓ choose, Intro links, Esc closes.
    pub(super) fn link_picker(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.links.picker.take() else {
            return;
        };
        let p = theme::current(ctx);
        let exclude = self.selected.clone();
        let moving = picker.moving.is_some();
        // Blocks only go to pages (a database shows its table).
        let matches: Vec<Page> = picker_matches(&self.pages, &picker.query, exclude.as_deref())
            .into_iter()
            .filter(|p| !moving || p.kind == PageKind::Page)
            .cloned()
            .collect();
        use egui::{Key, Modifiers as M};
        let last = matches.len().saturating_sub(1);
        if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
            picker.selected = (picker.selected + 1).min(last);
        }
        if take_exact_key(ctx, Key::ArrowUp, M::NONE) {
            picker.selected = picker.selected.saturating_sub(1);
        }
        picker.selected = picker.selected.min(last);
        let mut chosen = take_exact_key(ctx, Key::Enter, M::NONE)
            .then(|| matches.get(picker.selected).cloned())
            .flatten();
        let mut close = take_exact_key(ctx, Key::Escape, M::NONE);
        let pages = &self.pages;
        let page_icons = &self.page_icons;
        let response = egui::Modal::new(egui::Id::unique("silex_link_picker")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(if moving {
                "Mover a una página"
            } else {
                "Enlazar a una página"
            });
            ui.add_space(theme::space::SM);
            let field = framed(
                ui,
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text(
                        egui::RichText::new("Busca una página por su título").color(p.text_faint),
                    )
                    .desired_width(f32::INFINITY),
            )
            .accessible_name(if moving {
                "Página a la que mover"
            } else {
                "Página a enlazar"
            });
            if std::mem::take(&mut picker.opening) {
                field.request_focus();
            }
            if field.changed() {
                picker.selected = 0;
            }
            ui.add_space(theme::space::SM);
            if matches.is_empty() {
                ui.label(
                    egui::RichText::new("Ninguna página tiene ese título.").color(p.text_muted),
                );
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, page) in matches.iter().enumerate() {
                        let path = page_path(pages, &page.id);
                        let row = page_row(
                            ui,
                            page,
                            page_icons.get(&page.id).map(String::as_str),
                            &path,
                            i == picker.selected,
                            egui::Role::Button,
                        );
                        if i == picker.selected {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            chosen = Some(page.clone());
                        }
                    }
                });
            ui.add_space(theme::space::SM);
            ui.label(
                egui::RichText::new(if moving {
                    "↑ ↓ para moverte · Intro para mover los bloques al final · Esc para cerrar"
                } else {
                    "↑ ↓ para moverte · Intro para enlazar · Esc para cerrar"
                })
                .small()
                .color(p.text_muted),
            );
        });
        close |= response.should_close();
        if let Some(page) = chosen {
            match &picker.moving {
                Some(ids) => {
                    let result = self.move_blocks_to(ids, &page);
                    self.report(result);
                }
                None => self.insert_link(&picker, &page),
            }
        } else if !close {
            self.links.picker = Some(picker);
        }
    }

    /// Adds a link to `page` where the picker was opened: after its block,
    /// or in place of it if «[[»/«/enlace» left it empty. One undo step.
    fn insert_link(&mut self, picker: &LinkPicker, page: &Page) {
        let Some(open) = self.selected.clone() else {
            return;
        };
        let mut at = picker
            .after
            .as_ref()
            .and_then(|id| self.blocks.iter().position(|b| b.id == *id))
            .map_or(self.blocks.len(), |i| i + 1);
        let indent = at
            .checked_sub(1)
            .and_then(|i| self.blocks.get(i))
            .map_or(0, |b| b.indent);
        if let Some(index) = at.checked_sub(1)
            && picker.replace.is_some()
            && picker.replace == picker.after
            && self
                .blocks
                .get(index)
                .is_some_and(|b| b.kind == BlockKind::Paragraph && b.content.trim().is_empty())
        {
            self.blocks.remove(index);
            at = index;
        }
        let link = Block {
            id: uuid::Uuid::new_v4().to_string(),
            page_id: open,
            kind: BlockKind::PageLink,
            content: page.title.clone(),
            checked: false,
            position: 0,
            indent,
            attachment: None,
            link: Some(page.id.clone()),
            marks: Vec::new(),
        };
        self.pending_focus = Some((link.id.clone(), 0));
        self.blocks.insert(at.min(self.blocks.len()), link);
        self.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, title: &str, parent: Option<&str>) -> Page {
        Page {
            id: id.into(),
            parent_id: parent.map(str::to_owned),
            title: title.into(),
            kind: PageKind::Page,
        }
    }

    #[test]
    fn the_picker_puts_titles_that_start_with_the_query_first() {
        let pages = vec![
            page("1", "Recetas", None),
            page("2", "Mis recetas vascas", None),
            page("3", "Pan", Some("1")),
            page("4", "Árbol de recetas", None),
            page("5", "", None),
        ];
        let titles = |q: &str, exclude: Option<&str>| {
            picker_matches(&pages, q, exclude)
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("rec", None), ["1", "4", "2"]);
        assert_eq!(titles("ARBOL", None), ["4"]);
        assert_eq!(titles("rec", Some("1")), ["4", "2"]);
        assert_eq!(
            titles("sin tit", None),
            ["5"],
            "an untitled page by its placeholder"
        );
        assert_eq!(titles("", None).len(), 5);
        assert_eq!(page_path(&pages, "3"), "Recetas");
        assert_eq!(page_path(&pages, "1"), "");
    }

    #[test]
    fn backlinks_go_by_title_without_accents_and_skip_pages_not_live() {
        let pages = vec![
            page("1", "Recetas", None),
            page("2", "Pan", Some("1")),
            page("3", "Árbol", None),
            page("4", "zumos", None),
            page("5", "Pan", None),
        ];
        let ids: Vec<String> = ["4", "2", "9", "3", "5"].map(String::from).into();
        let rows: Vec<(&str, String)> = backlink_rows(&pages, &ids)
            .into_iter()
            .map(|(p, path)| (p.id.as_str(), path))
            .collect();
        // «9» is not live (in the trash, or gone); same titles by path.
        assert_eq!(
            rows,
            [
                ("3", String::new()),
                ("5", String::new()),
                ("2", "Recetas".to_owned()),
                ("4", String::new()),
            ]
        );
    }

    #[test]
    fn a_card_shows_live_trashed_or_gone_pages() {
        let pages = vec![page("1", "Recetas", None), page("2", "Pan", Some("1"))];
        let block = |link: Option<&str>| Block {
            id: "b".into(),
            page_id: "x".into(),
            kind: BlockKind::PageLink,
            content: "viejo".into(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: None,
            link: link.map(str::to_owned),
            marks: Vec::new(),
        };
        let icons = HashMap::from([("2".to_owned(), "🍞".to_owned())]);
        let mut targets = HashMap::new();
        targets.insert(
            "9".to_owned(),
            LinkTarget {
                id: "9".into(),
                title: "Borrada".into(),
                kind: PageKind::Page,
                trashed: true,
            },
        );
        assert_eq!(
            link_state(&pages, &targets, &icons, &block(Some("2"))),
            LinkState::Live {
                title: "Pan".into(),
                kind: PageKind::Page,
                path: "Recetas".into(),
                icon: Some("🍞".into())
            }
        );
        assert_eq!(
            link_state(&pages, &targets, &icons, &block(Some("9"))),
            LinkState::Trashed {
                title: "Borrada".into()
            }
        );
        assert_eq!(
            link_state(&pages, &targets, &icons, &block(Some("7"))),
            LinkState::Gone
        );
        assert_eq!(
            link_state(&pages, &targets, &icons, &block(None)),
            LinkState::Gone
        );
    }
}
