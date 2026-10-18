//! Editor area: block rendering, editor keys, selection, slash menu, paste.

use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::Shift,
    editor,
    history::EditKind,
    model::{Block, BlockKind, descendants, list_numbers},
};

use super::*;

/// Cursor state of the focused block text field.
#[derive(Debug, Clone)]
pub(super) struct CaretInfo {
    pub(super) block_id: String,
    pub(super) char: usize,
    pub(super) selection: bool,
    pub(super) first_row: bool,
    pub(super) last_row: bool,
}

/// Structural keys handled by the block editor (see `src/editor.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorKey {
    Enter,
    Backspace,
    Delete,
    Indent,
    Outdent,
    Up,
    Down,
    /// Alt+Shift+↑ / ↓: move the block.
    MoveUp,
    MoveDown,
    /// Cmd/Ctrl+D: duplicate the block.
    Duplicate,
    /// Escape: leave the text and select the block.
    Select,
    /// Enter in the slash menu: apply this type.
    Slash(BlockKind),
}

/// Slash menu shown under the focused block while "/word" is being typed.
#[derive(Debug, Clone)]
pub(super) struct SlashMenu {
    pub(super) block_id: String,
    pub(super) matches: Vec<BlockKind>,
    pub(super) selected: usize,
}

/// Selected blocks (by id): `anchor` stays, `head` moves with Shift+↑/↓.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlockSelection {
    pub(super) anchor: String,
    pub(super) head: String,
}

/// Text field ids follow the page/block id, not the position on screen, so
/// each field keeps its own undo history (egui: Cmd/Ctrl+Z, Shift+Cmd+Z or
/// Ctrl+Y) when blocks are added or pages switched. See ADR 0002.
pub(super) fn field_id(generation: u64, kind: &str, id: &str) -> egui::Id {
    egui::Id::unique(("silex-field", generation, kind, id))
}

/// Horizontal space per indent level.
pub(super) const INDENT_WIDTH: f32 = 24.0;

/// Renders one block: type selector, indent, marker (checkbox, bullet,
/// number or quote bar) and its text field. `number` is the list number for
/// numbered blocks. Returns true when anything in the block changed.
/// Actions in a block's "…" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockAction {
    MoveUp,
    MoveDown,
    Duplicate,
    Delete,
}

/// What a block reported while being drawn.
pub(super) struct BlockOutput {
    /// The text was edited (groups into one undo step while typing).
    pub(super) text_changed: bool,
    /// Type or checkbox changed.
    pub(super) other_changed: bool,
    pub(super) action: Option<BlockAction>,
    /// Present when this block's text field has focus.
    pub(super) caret: Option<CaretInfo>,
}

pub(super) fn block_field(
    ui: &mut egui::Ui,
    generation: u64,
    block: &mut Block,
    number: Option<usize>,
    scroll_into_view: bool,
    selected: bool,
) -> BlockOutput {
    let id = field_id(generation, "block", &block.id);
    let fill = if selected {
        ui.visuals().selection.bg_fill.gamma_multiply(0.35)
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame = egui::Frame::new().fill(fill).corner_radius(4.0);
    frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let action = block_menu(ui);
                let mut other_changed = kind_selector(ui, block);
                ui.add_space(f32::from(block.indent) * INDENT_WIDTH);
                match block.kind {
                    BlockKind::Todo => {
                        other_changed |= ui.checkbox(&mut block.checked, "").changed();
                    }
                    BlockKind::Bulleted => {
                        ui.label("•");
                    }
                    BlockKind::Numbered => {
                        ui.label(format!("{}.", number.unwrap_or(1)));
                    }
                    BlockKind::Quote => {
                        let (bar, _) =
                            ui.allocate_exact_size(egui::vec2(3.0, 20.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(bar, 1.0, ui.visuals().weak_text_color());
                    }
                    _ => {}
                }
                let edit = match block.kind {
                    BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3 => {
                        TextEdit::singleline(&mut block.content)
                            .font(egui::FontId::proportional(heading_size(block.kind)))
                    }
                    BlockKind::Code => TextEdit::multiline(&mut block.content)
                        .code_editor()
                        .desired_rows(1),
                    _ => TextEdit::multiline(&mut block.content).desired_rows(1),
                };
                let output = edit.id(id).desired_width(f32::INFINITY).show(ui);
                let text_changed = output.response.changed();
                if scroll_into_view {
                    output.response.scroll_to_me(None);
                }
                let caret = output
                    .cursor_range
                    .filter(|_| output.response.has_focus())
                    .map(|range| {
                        let row = output.galley.layout_from_cursor(range.primary).row;
                        CaretInfo {
                            block_id: block.id.clone(),
                            char: range.primary.index.0,
                            selection: range.primary.index != range.secondary.index,
                            first_row: row == 0,
                            last_row: row + 1 >= output.galley.rows.len(),
                        }
                    });
                BlockOutput {
                    text_changed,
                    other_changed,
                    action,
                    caret,
                }
            })
            .inner
        })
        .inner
}

/// The slash menu list; returns the type clicked with the mouse, if any.
pub(super) fn slash_menu_ui(ui: &mut egui::Ui, menu: &SlashMenu) -> Option<BlockKind> {
    let mut clicked = None;
    egui::Frame::popup(ui.style()).show(ui, |ui| {
        ui.weak("Convertir en… (↑ ↓ Intro, Esc para cerrar)");
        for (i, kind) in menu.matches.iter().enumerate() {
            if ui
                .selectable_label(i == menu.selected, kind.label())
                .clicked()
            {
                clicked = Some(*kind);
            }
        }
    });
    clicked
}

/// Removes the first input event matching `wanted`; returns whether found.
pub(super) fn take_event(
    ctx: &egui::Context,
    mut wanted: impl FnMut(&egui::Event) -> bool,
) -> bool {
    ctx.input_mut(|input| {
        let mut taken = false;
        input.events.retain(|event| {
            let matches = !taken && wanted(event);
            taken |= matches;
            !matches
        });
        taken
    })
}

/// Takes a key press with exactly these modifiers (egui's `consume_key`
/// ignores extra Shift/Alt, which would swallow Shift+Enter).
pub(super) fn take_exact_key(
    ctx: &egui::Context,
    key: egui::Key,
    modifiers: egui::Modifiers,
) -> bool {
    ctx.input_mut(|input| {
        let mut taken = false;
        input.events.retain(|event| {
            let matches = !taken
                && matches!(event, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                    if *k == key && *m == modifiers);
            taken |= matches;
            !matches
        });
        taken
    })
}

/// Tracks IME composition from this frame's events: a non-empty preedit
/// starts it; a commit or an empty preedit ends it.
pub(super) fn ime_composing_after(ctx: &egui::Context, mut composing: bool) -> bool {
    ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::Ime(ime) = event {
                composing = match ime {
                    egui::ImeEvent::Preedit { text, .. } => !text.is_empty(),
                    _ => false,
                };
            }
        }
    });
    composing
}

/// Removes the character at `char_index` (if any).
pub(super) fn delete_char(text: &mut String, char_index: usize) {
    if let Some((byte, _)) = text.char_indices().nth(char_index) {
        text.remove(byte);
    }
}

pub(super) fn heading_size(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading1 => 24.0,
        BlockKind::Heading2 => 20.0,
        _ => 16.0,
    }
}

/// The "…" menu in front of each block.
pub(super) fn block_menu(ui: &mut egui::Ui) -> Option<BlockAction> {
    let mut action = None;
    ui.menu_button("…", |ui| {
        for (label, chosen) in [
            ("Mover arriba (Alt+Mayús+↑)", BlockAction::MoveUp),
            ("Mover abajo (Alt+Mayús+↓)", BlockAction::MoveDown),
            ("Duplicar", BlockAction::Duplicate),
            ("Eliminar", BlockAction::Delete),
        ] {
            if ui.button(label).clicked() {
                action = Some(chosen);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Acciones del bloque");
    action
}

/// Small type menu in front of each block; returns true if the type changed.
pub(super) fn kind_selector(ui: &mut egui::Ui, block: &mut Block) -> bool {
    let before = block.kind;
    egui::ComboBox::from_id_salt(("silex-kind", &block.id))
        .width(110.0)
        .selected_text(block.kind.label())
        .show_ui(ui, |ui| {
            for kind in BlockKind::ALL {
                ui.selectable_value(&mut block.kind, kind, kind.label());
            }
        });
    if block.kind == before {
        return false;
    }
    if block.kind != BlockKind::Todo {
        block.checked = false;
    }
    true
}

impl SilexApp {
    pub(super) fn block_field_id(&self, block_id: &str) -> egui::Id {
        field_id(self.doc_generation, "block", block_id)
    }

    /// Decides, before any widget runs, whether a key is a block-editor
    /// command for the focused block, and if so takes it away from the text
    /// field. Nothing is intercepted during an IME composition, with a text
    /// selection, or when the cursor is not where the command applies.
    pub(super) fn take_editor_key(&mut self, ctx: &egui::Context) {
        let Some(caret) = self.caret.clone() else {
            // No block is being edited: a composition elsewhere (e.g. the
            // search field) is not ours, and a stale flag must not linger.
            self.ime_composing = false;
            return;
        };
        self.ime_composing = ime_composing_after(ctx, self.ime_composing);
        if self.ime_composing {
            // Enter confirms the composition; the IME sends the text as a
            // commit, so the key itself must not also insert a newline.
            take_exact_key(ctx, egui::Key::Enter, egui::Modifiers::NONE);
            return;
        }
        let Some(block) = self.blocks.iter().find(|b| b.id == caret.block_id) else {
            return;
        };
        // egui clears focus on Escape at the start of the frame, before this
        // runs; for Escape the caret from the end of the last frame is enough.
        let escape = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. }
                    if modifiers.is_none())
            })
        });
        let focused = ctx.memory(|m| m.has_focus(self.block_field_id(&block.id)));
        if (!focused && !escape) || caret.selection {
            return;
        }
        let code = block.kind == BlockKind::Code;
        // Several lines pasted into a (non-code) block become several blocks.
        // A single line is left to the text field.
        if !code {
            let mut pasted = None;
            take_event(ctx, |event| match event {
                egui::Event::Paste(text) if text.contains(['\n', '\r']) => {
                    pasted = Some(text.clone());
                    true
                }
                _ => false,
            });
            if let Some(text) = pasted {
                self.pending_paste = Some((block.id.clone(), text));
                self.slash = None;
                return;
            }
        }
        let at_start = caret.char == 0;
        let at_end = caret.char >= editor::block_char_len(block);
        use egui::{Key, Modifiers as M};
        // The slash menu, when open, owns ↑ ↓ Enter and Escape.
        if let Some(menu) = self.slash.as_mut().filter(|m| m.block_id == block.id) {
            let last = menu.matches.len().saturating_sub(1);
            if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
                menu.selected = (menu.selected + 1).min(last);
            }
            if take_exact_key(ctx, Key::ArrowUp, M::NONE) {
                menu.selected = menu.selected.saturating_sub(1);
            }
            if take_exact_key(ctx, Key::Escape, M::NONE) {
                self.slash_dismissed = Some((block.id.clone(), block.content.clone()));
                self.slash = None;
                return;
            }
            if let Some(kind) = menu.matches.get(menu.selected).copied()
                && take_exact_key(ctx, Key::Enter, M::NONE)
            {
                self.pending_key = Some((EditorKey::Slash(kind), block.id.clone()));
            }
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(M::COMMAND, Key::D))) {
            self.pending_key = Some((EditorKey::Duplicate, block.id.clone()));
            return;
        }
        let candidates = [
            (EditorKey::MoveUp, Key::ArrowUp, M::ALT | M::SHIFT, true),
            (EditorKey::MoveDown, Key::ArrowDown, M::ALT | M::SHIFT, true),
            (EditorKey::Select, Key::Escape, M::NONE, true),
            (
                EditorKey::Enter,
                Key::Enter,
                M::NONE,
                !editor::enter_inserts_newline(block.kind),
            ),
            (EditorKey::Backspace, Key::Backspace, M::NONE, at_start),
            (EditorKey::Delete, Key::Delete, M::NONE, at_end),
            (EditorKey::Indent, Key::Tab, M::NONE, !code),
            (EditorKey::Outdent, Key::Tab, M::SHIFT, !code),
            (EditorKey::Up, Key::ArrowUp, M::NONE, caret.first_row),
            (EditorKey::Down, Key::ArrowDown, M::NONE, caret.last_row),
        ];
        for (command, key, modifiers, applies) in candidates {
            if applies && take_exact_key(ctx, key, modifiers) {
                self.pending_key = Some((command, block.id.clone()));
                return;
            }
        }
    }

    /// Runs the key taken this frame, after the text field processed any text
    /// typed earlier in the same frame (so the cursor position is current).
    /// Runs a multi-line paste taken this frame, at the current cursor.
    pub(super) fn run_paste(&mut self, ctx: &egui::Context) {
        let Some((block_id, text)) = self.pending_paste.take() else {
            return;
        };
        let Some(index) = self.blocks.iter().position(|b| b.id == block_id) else {
            return;
        };
        let cursor = egui::text_edit::TextEditState::load(ctx, self.block_field_id(&block_id))
            .and_then(|s| s.cursor.char_range())
            .map_or(0, |r| r.primary.index.0);
        let caret = editor::paste_lines(&mut self.blocks, index, cursor, &text, || {
            uuid::Uuid::new_v4().to_string()
        });
        self.mark_dirty();
        if let Some(caret) = caret
            && let Some(target) = self.blocks.get(caret.block)
        {
            self.pending_focus = Some((target.id.clone(), caret.char));
            ctx.request_repaint();
        }
    }

    pub(super) fn run_editor_key(&mut self, ctx: &egui::Context) {
        let Some((command, block_id)) = self.pending_key.take() else {
            return;
        };
        let Some(index) = self.blocks.iter().position(|b| b.id == block_id) else {
            return;
        };
        let field = self.block_field_id(&block_id);
        let cursor = egui::text_edit::TextEditState::load(ctx, field)
            .and_then(|s| s.cursor.char_range())
            .map_or(0, |r| r.primary.index.0);
        let caret = match command {
            EditorKey::Enter => editor::enter(&mut self.blocks, index, cursor, || {
                uuid::Uuid::new_v4().to_string()
            }),
            EditorKey::Backspace if cursor > 0 => {
                // Text was typed before Backspace in the same frame: plain delete.
                delete_char(&mut self.blocks[index].content, cursor - 1);
                Some(editor::Caret {
                    block: index,
                    char: cursor - 1,
                })
            }
            EditorKey::Backspace => editor::backspace_at_start(&mut self.blocks, index),
            EditorKey::Delete if cursor < editor::block_char_len(&self.blocks[index]) => {
                delete_char(&mut self.blocks[index].content, cursor);
                Some(editor::Caret {
                    block: index,
                    char: cursor,
                })
            }
            EditorKey::Delete => editor::delete_at_end(&mut self.blocks, index),
            EditorKey::Indent | EditorKey::Outdent => {
                let delta = if command == EditorKey::Indent { 1 } else { -1 };
                editor::change_indent(&mut self.blocks, index, delta);
                Some(editor::Caret {
                    block: index,
                    char: cursor,
                })
            }
            EditorKey::Up => index.checked_sub(1).map(|i| editor::Caret {
                block: i,
                char: editor::block_char_len(&self.blocks[i]),
            }),
            EditorKey::Down => (index + 1 < self.blocks.len()).then(|| editor::Caret {
                block: index + 1,
                char: 0,
            }),
            EditorKey::Select => {
                self.selection = Some(BlockSelection {
                    anchor: block_id.clone(),
                    head: block_id.clone(),
                });
                ctx.memory_mut(|m| m.surrender_focus(field));
                self.pending_focus = None;
                // The caret drawn this frame is stale now: no text field has focus.
                self.caret = None;
                return;
            }
            EditorKey::Slash(kind) => {
                self.slash = None;
                editor::apply_slash(&mut self.blocks, index, cursor, kind)
            }
            EditorKey::MoveUp | EditorKey::MoveDown => {
                let up = command == EditorKey::MoveUp;
                editor::move_range(&mut self.blocks, index, index, up).then(|| editor::Caret {
                    block: if up { index - 1 } else { index + 1 },
                    char: cursor,
                })
            }
            EditorKey::Duplicate => editor::duplicate_range(&mut self.blocks, index, index, || {
                uuid::Uuid::new_v4().to_string()
            })
            .map(|(copy, _)| editor::Caret {
                block: copy,
                char: cursor,
            }),
        };
        if !matches!(command, EditorKey::Up | EditorKey::Down) {
            // One undo step per command (egui's per-field undo is not used, ADR 0004).
            self.mark_dirty();
        }
        if let Some(caret) = caret
            && let Some(target) = self.blocks.get(caret.block)
        {
            self.pending_focus = Some((target.id.clone(), caret.char));
            ctx.request_repaint();
        }
    }

    /// Index range of the selection (inclusive), if both ends still exist.
    pub(super) fn selection_range(&self) -> Option<(usize, usize)> {
        let sel = self.selection.as_ref()?;
        let a = self.blocks.iter().position(|b| b.id == sel.anchor)?;
        let h = self.blocks.iter().position(|b| b.id == sel.head)?;
        Some((a.min(h), a.max(h)))
    }

    /// Keys while blocks are selected (no text field has focus):
    /// ↑/↓ move, Shift+↑/↓ extend, Backspace/Delete delete, Cmd+C/Cmd+X copy
    /// or cut, Enter edits, Escape clears.
    pub(super) fn handle_selection_keys(&mut self, ctx: &egui::Context) {
        let Some(sel) = self.selection.clone() else {
            return;
        };
        let Some((first, last)) = self.selection_range() else {
            self.selection = None;
            return;
        };
        let Some(head) = self.blocks.iter().position(|b| b.id == sel.head) else {
            self.selection = None;
            return;
        };
        use egui::{Key, Modifiers as M};
        let id_at = |i: usize| self.blocks.get(i).map(|b| b.id.clone());
        let step = |key: Key, modifiers: M| take_exact_key(ctx, key, modifiers);
        if step(Key::ArrowUp, M::SHIFT) {
            if let Some(id) = head.checked_sub(1).and_then(id_at) {
                self.selection = Some(BlockSelection { head: id, ..sel });
            }
        } else if step(Key::ArrowDown, M::SHIFT) {
            if let Some(id) = id_at(head + 1) {
                self.selection = Some(BlockSelection { head: id, ..sel });
            }
        } else if step(Key::ArrowUp, M::NONE) {
            if let Some(id) = first
                .checked_sub(1)
                .and_then(id_at)
                .or_else(|| id_at(first))
            {
                self.selection = Some(BlockSelection {
                    anchor: id.clone(),
                    head: id,
                });
            }
        } else if step(Key::ArrowDown, M::NONE) {
            if let Some(id) = id_at(last + 1).or_else(|| id_at(last)) {
                self.selection = Some(BlockSelection {
                    anchor: id.clone(),
                    head: id,
                });
            }
        } else if step(Key::ArrowUp, M::ALT | M::SHIFT) {
            if editor::move_range(&mut self.blocks, first, last, true) {
                self.mark_dirty();
            }
        } else if step(Key::ArrowDown, M::ALT | M::SHIFT) {
            if editor::move_range(&mut self.blocks, first, last, false) {
                self.mark_dirty();
            }
        } else if ctx
            .input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(M::COMMAND, Key::D)))
        {
            if let Some((a, b)) = editor::duplicate_range(&mut self.blocks, first, last, || {
                uuid::Uuid::new_v4().to_string()
            }) {
                self.selection = Some(BlockSelection {
                    anchor: self.blocks[a].id.clone(),
                    head: self.blocks[b].id.clone(),
                });
                self.mark_dirty();
            }
        } else if step(Key::Enter, M::NONE) {
            if let Some(block) = self.blocks.get(head) {
                self.pending_focus = Some((block.id.clone(), editor::block_char_len(block)));
            }
            self.selection = None;
        } else if step(Key::Escape, M::NONE) {
            self.selection = None;
        } else {
            let copy = take_event(ctx, |e| matches!(e, egui::Event::Copy));
            let cut = take_event(ctx, |e| matches!(e, egui::Event::Cut));
            if copy || cut {
                ctx.copy_text(editor::selection_text(&self.blocks, first, last));
            }
            if cut || step(Key::Backspace, M::NONE) || step(Key::Delete, M::NONE) {
                self.delete_selection(first, last);
            }
        }
    }

    pub(super) fn delete_selection(&mut self, first: usize, last: usize) {
        let deleted = editor::delete_range(&mut self.blocks, first, last);
        if deleted.removed.is_empty() {
            return;
        }
        self.notice = Some(format!(
            "Se eliminaron {} bloque(s). {}+Z para deshacer.",
            deleted.removed.len(),
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Ctrl"
            }
        ));
        self.selection = self
            .blocks
            .get(first.min(self.blocks.len().saturating_sub(1)))
            .map(|b| BlockSelection {
                anchor: b.id.clone(),
                head: b.id.clone(),
            });
        self.mark_dirty();
    }

    /// After layout: open, update or close the slash menu from the caret, and
    /// drop the block selection if the user clicked into a text field.
    pub(super) fn refresh_slash_and_selection(&mut self) {
        if self.caret.is_some() {
            self.selection = None;
        }
        let Some(caret) = self.caret.as_ref().filter(|c| !c.selection) else {
            self.slash = None;
            return;
        };
        let Some(block) = self.blocks.iter().find(|b| b.id == caret.block_id) else {
            self.slash = None;
            return;
        };
        let dismissed = self
            .slash_dismissed
            .as_ref()
            .is_some_and(|(id, content)| *id == block.id && *content == block.content);
        if !dismissed {
            self.slash_dismissed = None;
        }
        let matches = editor::slash_query(&block.content, caret.char)
            .filter(|_| !dismissed && block.kind != BlockKind::Code)
            .map(editor::slash_matches)
            .filter(|m| !m.is_empty());
        self.slash = matches.map(|matches| {
            let selected = self
                .slash
                .as_ref()
                .filter(|m| m.block_id == block.id)
                .map_or(0, |m| m.selected.min(matches.len() - 1));
            SlashMenu {
                block_id: block.id.clone(),
                matches,
                selected,
            }
        });
    }

    /// Runs an action from a block's "…" menu.
    pub(super) fn block_action(&mut self, action: BlockAction, id: &str) {
        let Some(index) = self.blocks.iter().position(|b| b.id == id) else {
            return;
        };
        let changed = match action {
            BlockAction::MoveUp => editor::move_range(&mut self.blocks, index, index, true),
            BlockAction::MoveDown => editor::move_range(&mut self.blocks, index, index, false),
            BlockAction::Duplicate => {
                editor::duplicate_range(&mut self.blocks, index, index, || {
                    uuid::Uuid::new_v4().to_string()
                })
                .is_some()
            }
            BlockAction::Delete => {
                self.delete_selection(index, index);
                self.selection = None;
                return;
            }
        };
        if changed {
            self.mark_dirty();
        }
    }

    /// Focuses the block chosen by the last editor command, with its cursor.
    pub(super) fn apply_pending_focus(&mut self, ctx: &egui::Context) {
        let Some((block_id, char)) = self.pending_focus.clone() else {
            return;
        };
        let id = self.block_field_id(&block_id);
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(char),
            )));
        state.store(ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
        // Keep it until the target block has focus, so it can scroll into view.
        if self.caret.as_ref().is_some_and(|c| c.block_id == block_id) {
            self.pending_focus = None;
        }
    }

    /// Subpage and move controls for the open page.
    pub(super) fn page_actions(&mut self, ui: &mut egui::Ui, page_id: &str) {
        let mut action: Option<Option<String>> = None;
        let mut subpage = false;
        let current_parent = self
            .pages
            .iter()
            .find(|p| p.id == page_id)
            .and_then(|p| p.parent_id.clone());
        let blocked = descendants(&self.pages, page_id);
        // Live siblings in display order (same parent as stored in the database).
        let siblings: Vec<&str> = self
            .pages
            .iter()
            .filter(|p| p.parent_id == current_parent)
            .map(|p| p.id.as_str())
            .collect();
        let index = siblings.iter().position(|id| *id == page_id);
        let can_up = index.is_some_and(|i| i > 0);
        let can_down = index.is_some_and(|i| i + 1 < siblings.len());
        let mut shift = None;
        let mut trash = false;
        ui.horizontal(|ui| {
            subpage = ui.button("+ Subpágina").clicked();
            if ui
                .add_enabled(can_up, egui::Button::new("Subir"))
                .on_hover_text("Subir entre sus hermanas")
                .clicked()
            {
                shift = Some(Shift::Up);
            }
            if ui
                .add_enabled(can_down, egui::Button::new("Bajar"))
                .on_hover_text("Bajar entre sus hermanas")
                .clicked()
            {
                shift = Some(Shift::Down);
            }
            egui::ComboBox::from_id_salt("silex_move_target")
                .selected_text("Mover a…")
                .show_ui(ui, |ui| {
                    if current_parent.is_some() && ui.selectable_label(false, "Raíz").clicked() {
                        action = Some(None);
                    }
                    // Only destinations that cannot create a cycle.
                    for page in &self.pages {
                        let valid = page.id != page_id
                            && !blocked.contains(&page.id)
                            && current_parent.as_deref() != Some(page.id.as_str());
                        if valid
                            && ui
                                .selectable_label(false, display_title(&page.title))
                                .clicked()
                        {
                            action = Some(Some(page.id.clone()));
                        }
                    }
                });
        });
        let is_favorite = self.favorites.iter().any(|p| p.id == page_id);
        let mut toggle_favorite = false;
        let mut export = false;
        ui.horizontal(|ui| {
            let label = if is_favorite {
                "Quitar de favoritos"
            } else {
                "Añadir a favoritos"
            };
            toggle_favorite = ui.button(label).clicked();
            trash = ui
                .button("Enviar a la papelera")
                .on_hover_text("Se mueve con sus subpáginas; se puede restaurar")
                .clicked();
            export = ui
                .button("Exportar a Markdown")
                .on_hover_text("Guarda la página como archivo .md en la carpeta de exportaciones")
                .clicked();
        });
        if export {
            let result = self.export_current_page();
            self.report(result);
        }
        if subpage {
            let result = self.create_subpage(page_id);
            self.report(result);
        }
        if toggle_favorite {
            let result = self.set_favorite(page_id, !is_favorite);
            self.report(result);
        }
        if trash {
            let result = self.trash_page(page_id);
            self.report(result);
            return;
        }
        if let Some(shift) = shift {
            let result = self.reorder_page(page_id, shift);
            self.report(result);
        }
        if let Some(target) = action {
            let result = self.move_page(page_id, target.as_deref());
            self.report(result);
        }
    }

    pub(super) fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(page_id) = self.selected.clone() else {
            ui.label("Crea una página para empezar.");
            return;
        };
        ui.horizontal(|ui| {
            ui.heading("Documento");
            if ui.button("Guardar").clicked() {
                // A save error is already shown by the status bar.
                let _ = self.save_now();
            }
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("Deshacer"))
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("Rehacer"))
                .clicked()
            {
                self.redo();
            }
            self.status_bar(ui);
        });
        self.notice_bar(ui);
        self.export_bar(ui);
        self.page_actions(ui, &page_id);
        if self.selected.as_deref() != Some(page_id.as_str()) {
            // The page was trashed (or another opened) this frame.
            return;
        }
        ui.separator();
        let title = ui.add(
            TextEdit::singleline(&mut self.title)
                .id(field_id(self.doc_generation, "title", &page_id))
                .font(egui::TextStyle::Heading)
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.focus_title) {
            title.request_focus();
        }
        let changed = title.changed();
        if changed {
            self.changed(EditKind::Text("title".into()));
        }
        ui.add_space(14.0);
        let mut edit: Option<EditKind> = None;
        let mut block_action: Option<(BlockAction, String)> = None;
        egui::ScrollArea::vertical()
            .id_salt("silex_editor_blocks")
            .show(ui, |ui| {
                let numbers = list_numbers(&self.blocks);
                let scroll_to = self.pending_focus.as_ref().map(|(id, _)| id.clone());
                self.caret = None;
                let selected = self.selection_range();
                let mut clicked_kind = None;
                for (index, (block, number)) in self.blocks.iter_mut().zip(numbers).enumerate() {
                    let scroll = scroll_to.as_deref() == Some(block.id.as_str());
                    let is_selected = selected.is_some_and(|(a, b)| (a..=b).contains(&index));
                    let out =
                        block_field(ui, self.doc_generation, block, number, scroll, is_selected);
                    let kind = if out.other_changed {
                        Some(EditKind::Other)
                    } else {
                        out.text_changed.then(|| EditKind::Text(block.id.clone()))
                    };
                    if let Some(kind) = kind {
                        // Two fields changed in one frame (rare): one non-text step.
                        edit = Some(if edit.is_some() {
                            EditKind::Other
                        } else {
                            kind
                        });
                    }
                    if let Some(action) = out.action {
                        block_action = Some((action, block.id.clone()));
                    }
                    if out.caret.is_some() {
                        self.caret = out.caret;
                    }
                    if let Some(menu) = self.slash.as_ref().filter(|m| m.block_id == block.id)
                        && let Some(kind) = slash_menu_ui(ui, menu)
                    {
                        clicked_kind = Some((kind, block.id.clone()));
                    }
                    ui.add_space(6.0);
                }
                if let Some((kind, id)) = clicked_kind {
                    self.pending_key = Some((EditorKey::Slash(kind), id));
                }
            });
        if let Some(kind) = edit {
            self.changed(kind);
        }
        if let Some((action, id)) = block_action {
            self.block_action(action, &id);
        }
        ui.separator();
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("+ Texto", BlockKind::Paragraph),
                ("+ Título", BlockKind::Heading1),
                ("+ Tarea", BlockKind::Todo),
            ] {
                if ui.button(label).clicked() {
                    // In memory, like every other edit: undoable and saved by the autosave.
                    let block = Block {
                        id: uuid::Uuid::new_v4().to_string(),
                        page_id: page_id.clone(),
                        kind,
                        content: String::new(),
                        checked: false,
                        position: 0,
                        indent: 0,
                    };
                    self.pending_focus = Some((block.id.clone(), 0));
                    self.blocks.push(block);
                    self.mark_dirty();
                }
            }
        });
    }
}
