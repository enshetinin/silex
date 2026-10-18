//! Editor area: block rendering, editor keys, selection, slash menu, paste.

use eframe::egui::{self, TextEdit};
use silex_desktop::{
    editor,
    history::EditKind,
    model::{Block, BlockKind, list_numbers},
};

use super::*;
use super::{icons, theme};

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

/// Actions in a block's handle menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockAction {
    Convert(BlockKind),
    MoveUp,
    MoveDown,
    Duplicate,
    Delete,
}

/// What a block reported while being drawn.
pub(super) struct BlockOutput {
    /// The text was edited (groups into one undo step while typing).
    pub(super) text_changed: bool,
    /// Checkbox changed.
    pub(super) other_changed: bool,
    pub(super) action: Option<BlockAction>,
    /// Present when this block's text field has focus.
    pub(super) caret: Option<CaretInfo>,
    /// The whole row (to know which block the pointer is over).
    pub(super) rect: egui::Rect,
}

/// Width of the left gutter where a block's handle appears on hover.
const GUTTER: f32 = 28.0;

/// Vertical offset of the text inside a text field (its default margin), so
/// markers line up with the first line of text.
const TEXT_TOP: f32 = 2.0;

/// Slot for a list marker (checkbox, bullet, number, quote bar). With the
/// item spacing after it, list text starts one indent level in, so a nested
/// item's marker sits under its parent's text.
const MARKER_WIDTH: f32 = INDENT_WIDTH - 8.0;

/// Extra space above a heading that follows another block.
pub(super) fn heading_space(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading1 => theme::space::LG,
        BlockKind::Heading2 => theme::space::MD,
        BlockKind::Heading3 => theme::space::SM,
        _ => 0.0,
    }
}

/// Indents a row (title, menus, buttons) to where block text starts.
fn gutter_space(ui: &mut egui::Ui) {
    ui.add_space(GUTTER + ui.spacing().item_spacing.x);
}

/// Reserves the marker slot, as tall as the first line of text. Returns its
/// rect; markers are painted centered on that line.
fn marker_slot(ui: &mut egui::Ui, line: f32, sense: egui::Sense) -> (egui::Rect, egui::Response) {
    ui.allocate_exact_size(egui::vec2(MARKER_WIDTH, line), sense)
}

/// To-do checkbox: a rounded box, filled with the accent and ticked when
/// done, centered on the first line of text.
fn todo_checkbox(ui: &mut egui::Ui, line: f32, checked: &mut bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, mut response) = marker_slot(ui, line, egui::Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let on = *checked;
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, ""));
    let b = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    let painter = ui.painter();
    if on {
        painter.rect_filled(b, radius, p.accent);
        let point = |x: f32, y: f32| b.min + egui::vec2(x, y);
        painter.line(
            vec![point(4.0, 8.3), point(6.8, 11.0), point(12.0, 5.3)],
            egui::Stroke::new(1.8, p.on_accent),
        );
    } else {
        let color = if response.hovered() {
            p.text_muted
        } else {
            p.text_faint
        };
        painter.rect_stroke(
            b,
            radius,
            egui::Stroke::new(1.5, color),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            b.expand(2.0),
            egui::CornerRadius::same(theme::radius::MD),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Outside,
        );
    }
    response
}

/// Id of a block handle's menu (stable, so the handle can stay visible
/// while its menu is open even if the pointer leaves the row).
pub(super) fn handle_menu_id(block_id: &str) -> egui::Id {
    egui::Id::unique(("silex-block-handle", block_id))
}

/// Icon for each block type (menus).
pub(super) fn kind_icon(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => icons::TEXT,
        BlockKind::Heading1 => icons::HEADING_1,
        BlockKind::Heading2 => icons::HEADING_2,
        BlockKind::Heading3 => icons::HEADING_3,
        BlockKind::Todo => icons::TODO,
        BlockKind::Bulleted => icons::LIST_BULLETS,
        BlockKind::Numbered => icons::LIST_NUMBERS,
        BlockKind::Quote => icons::QUOTE,
        BlockKind::Code => icons::CODE,
    }
}

/// How a block is drawn this frame.
pub(super) struct BlockView {
    pub(super) number: Option<usize>,
    pub(super) scroll_into_view: bool,
    pub(super) selected: bool,
    /// Show the handle (pointer over the row, or its menu open).
    pub(super) show_handle: bool,
    pub(super) focused: bool,
}

/// One block, borderless: a hover handle in the left gutter (type and
/// actions menu), indent, marker (checkbox, bullet, number or quote bar)
/// and the text field styled for its type.
pub(super) fn block_field(
    ui: &mut egui::Ui,
    generation: u64,
    block: &mut Block,
    view: &BlockView,
) -> BlockOutput {
    let p = theme::current(ui.ctx());
    let id = field_id(generation, "block", &block.id);
    let fill = if view.selected {
        p.accent_soft
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame = egui::Frame::new()
        .fill(fill)
        .corner_radius(theme::radius::MD)
        .inner_margin(egui::Margin::symmetric(0, 1));
    let inner = frame.show(ui, |ui| {
        // Top-aligned: the text field is taller than its first line, so
        // centering would push markers below the text.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let (gutter, _) =
                ui.allocate_exact_size(egui::vec2(GUTTER, 24.0), egui::Sense::hover());
            let action = if view.show_handle {
                block_handle(ui, gutter, block)
            } else {
                None
            };
            ui.add_space(f32::from(block.indent) * INDENT_WIDTH);
            // Height of the first line of body text (text margin included):
            // markers center on it, and it is the minimum height of a block
            // (the theme's 28 px interact height made every block too tall).
            let body = egui::TextStyle::Body.resolve(ui.style());
            let line = ui.fonts_mut(|f| f.row_height(&body)) + 2.0 * TEXT_TOP;
            ui.spacing_mut().interact_size.y = line;
            let mut other_changed = false;
            let mut quote_slot = None;
            match block.kind {
                BlockKind::Todo => {
                    other_changed |= todo_checkbox(ui, line, &mut block.checked)
                        .accessible_name("Tarea completada")
                        .changed();
                }
                BlockKind::Bulleted => {
                    let (slot, _) = marker_slot(ui, line, egui::Sense::hover());
                    ui.painter().circle_filled(slot.center(), 2.5, p.text_muted);
                }
                BlockKind::Numbered => {
                    let (slot, _) = marker_slot(ui, line, egui::Sense::hover());
                    // Right-aligned, so "9." and "10." end at the same place.
                    ui.painter().text(
                        slot.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        format!("{}.", view.number.unwrap_or(1)),
                        body.clone(),
                        p.text_muted,
                    );
                }
                BlockKind::Quote => {
                    // The bar is painted once the text's height is known.
                    quote_slot = Some(marker_slot(ui, line, egui::Sense::hover()).0);
                }
                _ => {}
            }
            let placeholder = match block.kind {
                BlockKind::Paragraph if view.focused => Some("Escribe / para comandos"),
                kind if kind.is_heading() => Some(kind.label()),
                _ => None,
            };
            let style = match block.kind {
                BlockKind::Heading1 => egui::TextStyle::Name(theme::text::H1.into()),
                BlockKind::Heading2 => egui::TextStyle::Name(theme::text::H2.into()),
                BlockKind::Heading3 => egui::TextStyle::Name(theme::text::H3.into()),
                BlockKind::Code => egui::TextStyle::Monospace,
                _ => egui::TextStyle::Body,
            };
            // A done to-do: muted and struck through.
            let mut strikethrough = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                let format = egui::TextFormat {
                    font_id: egui::TextStyle::Body.resolve(ui.style()),
                    color: p.text_muted,
                    strikethrough: egui::Stroke::new(1.0, p.text_muted),
                    ..Default::default()
                };
                let mut job =
                    egui::text::LayoutJob::single_section(text.as_str().to_owned(), format);
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let mut edit = if block.kind.is_heading() {
                TextEdit::singleline(&mut block.content)
            } else {
                TextEdit::multiline(&mut block.content).desired_rows(1)
            }
            .id(id)
            .font(style)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY);
            if block.kind == BlockKind::Code {
                edit = edit.code_editor().frame(
                    egui::Frame::new()
                        .fill(p.code_bg)
                        .corner_radius(theme::radius::MD)
                        .inner_margin(egui::Margin::same(10)),
                );
            }
            if block.kind == BlockKind::Todo && block.checked {
                edit = edit.text_color(p.text_muted).layouter(&mut strikethrough);
            }
            if let Some(text) = placeholder {
                edit = edit.hint_text(egui::RichText::new(text).color(p.text_faint));
            }
            let output = edit.show(ui);
            if let Some(slot) = quote_slot {
                let bar = egui::Rect::from_min_max(
                    slot.left_top() + egui::vec2(2.0, 2.0),
                    egui::pos2(slot.left() + 5.0, output.response.rect.bottom() - 2.0),
                );
                ui.painter().rect_filled(bar, 1.5, p.border);
            }
            // Screen readers announce the block type ("Tarea", "Título 1"…).
            let response =
                egui::Response::clone(&output.response).accessible_name(block.kind.label());
            let text_changed = response.changed();
            if view.scroll_into_view {
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
            (text_changed, other_changed, action, caret)
        })
        .inner
    });
    let (text_changed, other_changed, action, caret) = inner.inner;
    BlockOutput {
        text_changed,
        other_changed,
        action,
        caret,
        rect: inner.response.rect,
    }
}

/// The hover handle and its menu: convert to another type, move, duplicate,
/// delete.
fn block_handle(ui: &mut egui::Ui, gutter: egui::Rect, block: &Block) -> Option<BlockAction> {
    let p = theme::current(ui.ctx());
    // Interact + paint, not a widget: nothing is allocated, so showing the
    // handle can never move the block's text (`put` reserves its widget's
    // full size, which is wider than the gutter).
    let rect = egui::Rect::from_center_size(
        egui::pos2(gutter.center().x, gutter.top() + 12.0),
        egui::vec2(22.0, 22.0),
    );
    let handle = ui.interact(
        rect,
        egui::Id::unique(("silex-block-handle-button", &block.id)),
        egui::Sense::click(),
    );
    handle
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Acciones del bloque"));
    let open = egui::Popup::is_id_open(ui.ctx(), handle_menu_id(&block.id));
    if handle.hovered() || open {
        ui.painter().rect_filled(rect, theme::radius::SM, p.subtle);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icons::DRAG_HANDLE,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    let handle = handle.on_hover_text("Acciones del bloque");
    let mut action = None;
    egui::Popup::menu(&handle)
        .id(handle_menu_id(&block.id))
        .show(|ui| {
            ui.menu_button((icons::icon(icons::TEXT), " Convertir en"), |ui| {
                for kind in BlockKind::ALL {
                    let current = kind == block.kind;
                    if ui
                        .selectable_label(
                            current,
                            (icons::icon(kind_icon(kind)), format!(" {}", kind.label())),
                        )
                        .clicked()
                    {
                        action = Some(BlockAction::Convert(kind));
                        ui.close();
                    }
                }
            });
            ui.separator();
            for (icon, label, chosen) in [
                (icons::ARROW_UP, " Mover arriba", BlockAction::MoveUp),
                (icons::ARROW_DOWN, " Mover abajo", BlockAction::MoveDown),
                (icons::COPY, " Duplicar", BlockAction::Duplicate),
            ] {
                if ui.button((icons::icon(icon), label)).clicked() {
                    action = Some(chosen);
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button((
                    icons::icon(icons::TRASH).color(p.danger),
                    egui::RichText::new(" Eliminar").color(p.danger),
                ))
                .clicked()
            {
                action = Some(BlockAction::Delete);
                ui.close();
            }
        });
    action
}

/// The slash menu list; returns the type clicked with the mouse, if any.
/// The slash menu, floating under the block (above it when there is no
/// room below) so it never pushes the document or gets cut off. Returns the
/// type clicked with the mouse, if any.
pub(super) fn slash_menu_ui(
    ui: &mut egui::Ui,
    menu: &SlashMenu,
    block: egui::Rect,
) -> Option<BlockKind> {
    let p = theme::current(ui.ctx());
    // Text starts after the gutter; rows are ~30 px plus the header.
    let left = block.left() + GUTTER + ui.spacing().item_spacing.x;
    let height = 44.0 + 30.0 * menu.matches.len() as f32;
    let screen = ui.ctx().content_rect();
    let below = screen.bottom() - block.bottom() >= height || block.top() - screen.top() < height;
    let (pivot, anchor) = if below {
        (
            egui::Align2::LEFT_TOP,
            egui::pos2(left, block.bottom() + 4.0),
        )
    } else {
        (
            egui::Align2::LEFT_BOTTOM,
            egui::pos2(left, block.top() - 4.0),
        )
    };
    let mut clicked = None;
    egui::Area::new(egui::Id::unique(("silex-slash-menu", &menu.block_id)))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(anchor)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    ui.set_width(240.0);
                    ui.label(
                        egui::RichText::new("Convertir en… (↑ ↓ Intro, Esc para cerrar)")
                            .small()
                            .color(p.text_muted),
                    );
                    for (i, kind) in menu.matches.iter().enumerate() {
                        let option = ui.add(egui::Button::selectable(
                            i == menu.selected,
                            (icons::icon(kind_icon(*kind)), format!("  {}", kind.label())),
                        ));
                        if i == menu.selected {
                            // Arrow keys can move past the visible part.
                            option.scroll_to_me(None);
                        }
                        if option.clicked() {
                            clicked = Some(*kind);
                        }
                    }
                });
            });
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
            BlockAction::Convert(kind) => {
                let block = &mut self.blocks[index];
                let changed = block.kind != kind;
                block.kind = kind;
                if kind != BlockKind::Todo {
                    block.checked = false;
                }
                changed
            }
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

    /// The open page: top bar, notices and the centered document column.
    pub(super) fn editor(&mut self, ui: &mut egui::Ui) {
        let p = theme::current(ui.ctx());
        let Some(page_id) = self.selected.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.label(egui::RichText::new("No hay ninguna página abierta.").color(p.text_muted));
                ui.add_space(theme::space::SM);
                if ui
                    .button((icons::icon(icons::PLUS), " Nueva página"))
                    .clicked()
                {
                    let result = self.create_root_page("Sin título");
                    self.report(result);
                }
            });
            return;
        };
        self.top_bar(ui, &page_id);
        if self.selected.as_deref() != Some(page_id.as_str()) {
            // The page was trashed (or another opened) from the top bar.
            return;
        }
        self.notice_bar(ui);
        self.export_bar(ui);

        let mut edit: Option<EditKind> = None;
        let mut block_action: Option<(BlockAction, String)> = None;
        let mut hovered = None;
        let pointer = ui.ctx().pointer_hover_pos();
        egui::ScrollArea::vertical()
            .id_salt("silex_editor_blocks")
            .auto_shrink(false)
            .show(ui, |ui| {
                const COLUMN: f32 = 720.0;
                let width = ui.available_width().min(COLUMN + GUTTER);
                let side = ((ui.available_width() - width) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.add_space(theme::space::XL);
                        ui.horizontal(|ui| {
                            gutter_space(ui);
                            let title = ui.add(
                                TextEdit::singleline(&mut self.title)
                                    .id(field_id(self.doc_generation, "title", &page_id))
                                    .font(egui::TextStyle::Name(theme::text::TITLE.into()))
                                    .frame(egui::Frame::NONE)
                                    .hint_text(
                                        egui::RichText::new("Sin título").color(p.text_faint),
                                    )
                                    .desired_width(f32::INFINITY),
                            );
                            if std::mem::take(&mut self.focus_title) {
                                title.request_focus();
                            }
                            if title.changed() {
                                edit = Some(EditKind::Text("title".into()));
                            }
                        });
                        ui.add_space(theme::space::LG);

                        let numbers = list_numbers(&self.blocks);
                        let scroll_to = self.pending_focus.as_ref().map(|(id, _)| id.clone());
                        self.caret = None;
                        let selected = self.selection_range();
                        let mut clicked_kind = None;
                        let ctx = ui.ctx().clone();
                        let focused = ctx.memory(|m| m.focused());
                        for (index, (block, number)) in
                            self.blocks.iter_mut().zip(numbers).enumerate()
                        {
                            let field = field_id(self.doc_generation, "block", &block.id);
                            let view = BlockView {
                                number,
                                scroll_into_view: scroll_to.as_deref() == Some(block.id.as_str()),
                                selected: selected.is_some_and(|(a, b)| (a..=b).contains(&index)),
                                show_handle: self.hovered_block.as_deref()
                                    == Some(block.id.as_str())
                                    || egui::Popup::is_id_open(&ctx, handle_menu_id(&block.id)),
                                focused: focused == Some(field),
                            };
                            if index > 0 {
                                ui.add_space(heading_space(block.kind));
                            }
                            let out = block_field(ui, self.doc_generation, block, &view);
                            if pointer.is_some_and(|p| out.rect.contains(p)) {
                                hovered = Some(block.id.clone());
                            }
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
                            if let Some(menu) =
                                self.slash.as_ref().filter(|m| m.block_id == block.id)
                                && let Some(kind) = slash_menu_ui(ui, menu, out.rect)
                            {
                                clicked_kind = Some((kind, block.id.clone()));
                            }
                        }
                        if let Some((kind, id)) = clicked_kind {
                            self.pending_key = Some((EditorKey::Slash(kind), id));
                        }

                        ui.add_space(theme::space::SM);
                        ui.horizontal(|ui| {
                            // The "+" lines up with the text, not the button's edge.
                            ui.add_space(
                                GUTTER + ui.spacing().item_spacing.x
                                    - ui.spacing().button_padding.x,
                            );
                            let add = ui
                                .add(
                                    egui::Button::new((
                                        icons::icon(icons::PLUS).color(p.text_muted),
                                        egui::RichText::new(" Añadir bloque").color(p.text_muted),
                                    ))
                                    .frame_when_inactive(false),
                                )
                                .accessible_name("Añadir bloque");
                            if add.clicked() {
                                // In memory, like every other edit: undoable and saved by the autosave.
                                let block = Block {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    page_id: page_id.clone(),
                                    kind: BlockKind::Paragraph,
                                    content: String::new(),
                                    checked: false,
                                    position: 0,
                                    indent: 0,
                                };
                                self.pending_focus = Some((block.id.clone(), 0));
                                self.blocks.push(block);
                                self.mark_dirty();
                            }
                        });
                        ui.add_space(theme::space::XXL);
                    });
                });
            });
        self.hovered_block = hovered;
        if let Some(kind) = edit {
            self.changed(kind);
        }
        if let Some((action, id)) = block_action {
            self.block_action(action, &id);
        }
    }

    /// Breadcrumbs on the left; save state, undo/redo, favorite and the
    /// page menu on the right.
    fn top_bar(&mut self, ui: &mut egui::Ui, page_id: &str) {
        let p = theme::current(ui.ctx());
        let mut action: Option<RowAction> = None;
        let mut extra: Option<&'static str> = None;
        let ancestors: Vec<(String, String)> = ancestors(&self.pages, page_id)
            .into_iter()
            .rev()
            .filter_map(|id| {
                self.pages
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| (id, display_title(&p.title).to_owned()))
            })
            .collect();
        let is_favorite = self.favorites.iter().any(|p| p.id == page_id);
        let ghost = |text: egui::RichText| egui::Button::new(text).frame_when_inactive(false);
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);
            for (id, title) in &ancestors {
                if ui
                    .add(ghost(egui::RichText::new(title).color(p.text_muted)))
                    .clicked()
                {
                    action = Some(RowAction::Open(id.clone()));
                }
                ui.label(
                    icons::icon(icons::CARET_RIGHT)
                        .size(14.0)
                        .color(p.text_faint),
                );
            }
            ui.label(egui::RichText::new(display_title(&self.title)).color(p.text));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = ui
                    .add(ghost(icons::icon(icons::DOTS_THREE).color(p.text_muted)))
                    .on_hover_text("Más acciones")
                    .accessible_name("Más acciones de la página");
                egui::Popup::menu(&more).show(|ui| {
                    if let Some(chosen) = self.page_menu(ui, page_id) {
                        action = Some(chosen);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button((icons::icon(icons::EXPORT), " Exportar a Markdown"))
                        .clicked()
                    {
                        extra = Some("export");
                        ui.close();
                    }
                    if ui
                        .button((icons::icon(icons::SAVE), " Guardar ahora"))
                        .clicked()
                    {
                        extra = Some("save");
                        ui.close();
                    }
                });
                let star_color = if is_favorite { p.accent } else { p.text_muted };
                let star_tip = if is_favorite {
                    "Quitar de favoritos"
                } else {
                    "Añadir a favoritos"
                };
                if ui
                    .add(ghost(icons::icon(icons::STAR).color(star_color)))
                    .on_hover_text(star_tip)
                    .accessible_name(star_tip)
                    .clicked()
                {
                    action = Some(RowAction::Favorite(page_id.to_owned(), !is_favorite));
                }
                if ui
                    .add_enabled(
                        self.history.can_redo(),
                        ghost(icons::icon(icons::REDO).color(p.text_muted)),
                    )
                    .on_hover_text("Rehacer")
                    .accessible_name("Rehacer")
                    .clicked()
                {
                    self.redo();
                }
                if ui
                    .add_enabled(
                        self.history.can_undo(),
                        ghost(icons::icon(icons::UNDO).color(p.text_muted)),
                    )
                    .on_hover_text("Deshacer")
                    .accessible_name("Deshacer")
                    .clicked()
                {
                    self.undo();
                }
                ui.add_space(theme::space::SM);
                self.status_bar(ui);
            });
        });
        let line = ui.available_rect_before_wrap();
        ui.painter()
            .hline(line.x_range(), line.top(), egui::Stroke::new(1.0, p.border));
        match extra {
            Some("export") => {
                let result = self.export_current_page();
                self.report(result);
            }
            Some("save") => {
                // A save error is already shown by the status indicator.
                let _ = self.save_now();
            }
            _ => {}
        }
        if let Some(action) = action {
            // Renaming the open page is done in its title.
            let action = match action {
                RowAction::StartRename(id) if id == page_id => {
                    self.focus_title = true;
                    return;
                }
                other => other,
            };
            let result = self.apply(action);
            self.report(result);
        }
    }
}
