//! Web links in a block's text (H12.1d, ADR 0015): the field that gives
//! some text its URL (Cmd+K over a selection, «Enlace…» in the block menu
//! and the palette), and opening a link with Cmd+clic or Cmd+Intro.
//!
//! Opening asks the system to show the URL (a browser or a mail app), like
//! «Buscar actualizaciones»: the app itself connects to nothing.
use eframe::egui::{self};
use silex_desktop::marks::{self, Mark};
use silex_desktop::model::BlockKind;

use super::blocks::take_exact_key;
use super::find::match_rects;
use super::*;
use super::{icons, theme};

/// Width of the link field's popup.
const POPUP_WIDTH: f32 = 320.0;

/// The link field, open over the characters `[start, end)` of a block.
#[derive(Debug, Clone)]
pub(super) struct LinkEdit {
    pub(super) block_id: String,
    pub(super) start: usize,
    pub(super) end: usize,
    /// The block's selection when it opened (anchor, cursor): given back
    /// when it closes.
    selection: (usize, usize),
    /// What is typed: the link's URL.
    pub(super) url: String,
    /// The text already had a link: «Quitar enlace» is offered.
    existing: bool,
    /// Why the last URL was refused.
    pub(super) error: Option<&'static str>,
    /// Focus the field on the next frame.
    opening: bool,
    /// Where its text is on screen, set by the editor each frame. Without
    /// it (the block or its page is gone) the field closes.
    pub(super) anchor: Option<egui::Rect>,
}

/// Id of the URL field.
pub(super) fn url_field_id() -> egui::Id {
    egui::Id::unique("silex-link-url")
}

/// Shown when a URL is refused.
const REFUSED: &str = "Solo enlaces web (http, https) o de correo (mailto:).";

/// Asks the system to open `url`. Only allowed URLs, even if a stored
/// mark said otherwise (ADR 0015).
pub(super) fn open_web_link(ctx: &egui::Context, url: &str) {
    if marks::normalize_url(url).as_deref() == Some(url) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

/// The link under `pos` in a text field laid out as `galley` at `origin`.
pub(super) fn link_under<'a>(
    galley: &egui::Galley,
    origin: egui::Pos2,
    marks: &'a [Mark],
    pos: egui::Pos2,
) -> Option<&'a Mark> {
    // The nearest cursor is between two characters: the pointer is on one
    // of them, or on neither (past the end of a line, between lines).
    let index = galley.cursor_from_pos(pos - origin).index.0;
    [index.checked_sub(1), Some(index)]
        .into_iter()
        .flatten()
        .filter_map(|char| marks::link_over(marks, char).map(|link| (char, link)))
        .find(|(char, _)| {
            match_rects(galley, origin, *char, char + 1)
                .iter()
                .any(|rect| rect.contains(pos))
        })
        .map(|(_, link)| link)
}

/// What a screen reader hears about the links of a block, after its text:
/// «Enlaces: documentación (https://…)».
pub(super) fn links_description(text: &str, marks: &[Mark]) -> Option<String> {
    let links: Vec<String> = marks
        .iter()
        .filter_map(|m| {
            let url = m.url()?;
            let words: String = text.chars().skip(m.start).take(m.end - m.start).collect();
            Some(format!("{words} ({url})"))
        })
        .collect();
    match links.len() {
        0 => None,
        1 => Some(format!("Enlace: {}", links[0])),
        _ => Some(format!("Enlaces: {}", links.join("; "))),
    }
}

impl SilexApp {
    /// The block whose selected text Cmd+K links (ADR 0015): the focused
    /// text block, if it has text selected. Otherwise Cmd+K opens the
    /// search window.
    pub(super) fn link_shortcut_block(&self, ctx: &egui::Context) -> Option<String> {
        let caret = self.caret.as_ref().filter(|c| c.selection)?;
        self.blocks
            .iter()
            .find(|b| b.id == caret.block_id && b.kind.is_text())
            .filter(|b| ctx.memory(|m| m.has_focus(self.block_field_id(&b.id))))
            .map(|b| b.id.clone())
    }

    /// Opens the link field on block `id`, over its selected text. From a
    /// menu or the palette (`whole`) with nothing selected: over the link
    /// at the cursor, or else the whole block. Code blocks take no links:
    /// a notice says so.
    pub(super) fn open_link_edit(&mut self, ctx: &egui::Context, id: &str, whole: bool) {
        let Some(block) = self.blocks.iter().find(|b| b.id == id) else {
            return;
        };
        if !block.kind.takes_marks() {
            if block.kind == BlockKind::Code {
                self.notice = Some(CODE_NOTICE.into());
            }
            return;
        }
        let (anchor, cursor) = self.format_selection(ctx, id, whole);
        let len = block.content.chars().count();
        let (start, end) = if anchor != cursor {
            (anchor.min(cursor), anchor.max(cursor))
        } else if let Some(link) = marks::link_at(&block.marks, cursor) {
            (link.start, link.end)
        } else {
            (0, len)
        };
        if start == end {
            self.notice = Some("Escribe primero el texto del enlace.".into());
            return;
        }
        let url = marks::link_url_in(&block.marks, start, end).map(str::to_owned);
        self.typing_format = None;
        self.slash = None;
        self.link_edit = Some(LinkEdit {
            block_id: id.to_owned(),
            start,
            end,
            selection: (anchor, cursor),
            existing: url.is_some(),
            url: url.unwrap_or_default(),
            error: None,
            opening: true,
            anchor: None,
        });
    }

    /// Closes the link field; with `restore`, the block gets back its focus
    /// and selection.
    fn close_link_edit(&mut self, restore: bool) {
        if let Some(edit) = self.link_edit.take()
            && restore
        {
            self.pending_focus = Some((edit.block_id.clone(), edit.selection.1));
            self.pending_anchor = Some((edit.block_id, edit.selection.0));
        }
    }

    /// The link field, under its text: Intro applies the URL (nothing
    /// typed removes the link), Esc or a click elsewhere cancels. One undo
    /// step per change.
    pub(super) fn link_edit_ui(&mut self, ctx: &egui::Context) {
        let Some(edit) = self.link_edit.as_mut() else {
            return;
        };
        let Some(anchor) = edit.anchor.take() else {
            self.link_edit = None;
            return;
        };
        use egui::{Key, Modifiers as M};
        let mut apply = take_exact_key(ctx, Key::Enter, M::NONE);
        let cancel = take_exact_key(ctx, Key::Escape, M::NONE);
        let mut remove = false;
        let p = theme::current(ctx);
        let area = egui::Area::new(egui::Id::unique("silex-link-edit"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_TOP)
            .fixed_pos(anchor.left_bottom() + egui::vec2(0.0, 6.0))
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(POPUP_WIDTH);
                    ui.horizontal(|ui| {
                        icons::decoration(ui, icons::LINK, 16.0, p.text_muted);
                        let field = ui
                            .add(
                                egui::TextEdit::singleline(&mut edit.url)
                                    .id(url_field_id())
                                    .hint_text("Pega o escribe un enlace")
                                    .desired_width(f32::INFINITY),
                            )
                            .accessible_name("Dirección del enlace");
                        if std::mem::take(&mut edit.opening) {
                            field.request_focus();
                        }
                        if field.changed() {
                            edit.error = None;
                        }
                    });
                    if let Some(error) = edit.error {
                        ui.label(egui::RichText::new(error).small().color(p.danger));
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Intro aplica · Esc cancela")
                                .small()
                                .color(p.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            apply |= ui.button("Aplicar").clicked();
                            if edit.existing {
                                remove = ui.button("Quitar enlace").clicked();
                            }
                        });
                    });
                });
            });
        // A click elsewhere closes it, like a menu; the click goes where
        // it was meant to, so the block does not take the focus back.
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if remove {
            edit.url.clear();
            apply = true;
        }
        if apply {
            let (block_id, start, end, url) = (
                edit.block_id.clone(),
                edit.start,
                edit.end,
                edit.url.clone(),
            );
            let Some(block) = self.blocks.iter_mut().find(|b| b.id == block_id) else {
                self.link_edit = None;
                return;
            };
            match marks::set_link(&block.marks, start, end, &url) {
                Some(marks) => {
                    if marks != block.marks {
                        block.marks = marks;
                        self.mark_dirty();
                    }
                    self.close_link_edit(true);
                }
                None => {
                    if let Some(edit) = self.link_edit.as_mut() {
                        edit.error = Some(REFUSED);
                        edit.opening = true;
                    }
                }
            }
        } else if cancel {
            self.close_link_edit(true);
        } else if outside {
            self.close_link_edit(false);
        }
    }
}
