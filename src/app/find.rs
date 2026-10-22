//! Find and replace in the open page (H11.7): the bar above the page, its
//! keys and what it does to the blocks. The search itself is in
//! `silex_desktop::editor` (`find_in_blocks`, `replace_matches`).
use eframe::egui::{self};
use silex_desktop::editor::{Match, find_in_blocks, match_from, replace_matches};

use super::*;
use super::{icons, theme};

/// The fields of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FindField {
    Query,
    Replacement,
}

impl FindField {
    pub(super) fn id(self) -> egui::Id {
        match self {
            FindField::Query => egui::Id::unique("silex-find-query"),
            FindField::Replacement => egui::Id::unique("silex-find-replacement"),
        }
    }
}

/// The find bar while it is open.
#[derive(Debug, Default)]
pub(super) struct FindBar {
    pub(super) query: String,
    pub(super) replacement: String,
    /// The «Reemplazar» row is shown (Cmd+Opción+F, Ctrl+H).
    pub(super) replace: bool,
    /// Every match in the page, in page order, found this frame.
    pub(super) matches: Vec<Match>,
    /// The match shown, an index into `matches`.
    pub(super) current: usize,
    /// The query `current` was chosen for: when the query changes, the
    /// first match from `from` is shown again.
    searched: Option<String>,
    /// Where the search starts: the caret (block, char) when the bar
    /// opened, or after the last replacement.
    from: Option<(usize, usize)>,
    /// Give the keyboard to this field on the next frame.
    pub(super) focus: Option<FindField>,
    /// Scroll the page to the match shown on the next frame.
    pub(super) reveal: bool,
    /// Said after «Reemplazar todo» until the query changes.
    pub(super) status: Option<String>,
    /// The bar's buttons last frame: with the keyboard on one of them (or
    /// on a field), Cmd+Z undoes the page.
    controls: Vec<egui::Id>,
    /// The bar had the keyboard at the end of last frame. egui drops the
    /// focus on Esc before the app runs, so Esc is told by this.
    keyboard: bool,
    /// An IME composition is going on in one of its fields: Enter confirms
    /// it, and must not also jump or replace.
    composing: bool,
}

impl FindBar {
    /// Finds the matches in `blocks` again (they may have changed). A new
    /// query starts at the first match from where the search began.
    pub(super) fn refresh(&mut self, blocks: &[Block]) {
        self.matches = find_in_blocks(blocks, &self.query);
        if self.searched.as_deref() != Some(self.query.as_str()) {
            self.current = match_from(&self.matches, self.from);
            self.searched = Some(self.query.clone());
            self.reveal = !self.matches.is_empty();
            self.status = None;
        } else if self.current >= self.matches.len() {
            self.current = 0;
        }
    }

    /// The match shown, if any.
    pub(super) fn shown(&self) -> Option<Match> {
        self.matches.get(self.current).copied()
    }

    /// Shows the next match (`forward`) or the previous one, going round.
    pub(super) fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
        self.reveal = true;
    }

    /// «3 de 12», «Sin resultados», or nothing with no query.
    pub(super) fn counter(&self) -> String {
        if self.query.is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            "Sin resultados".to_owned()
        } else {
            format!("{} de {}", self.current + 1, self.matches.len())
        }
    }
}

/// What the bar asks for this frame.
pub(super) enum FindAction {
    Step(bool),
    Replace,
    ReplaceAll,
    ToggleReplace,
    Close,
}

impl SilexApp {
    /// Cmd+F (`replace` false) or Cmd+Opción+F / Ctrl+H: opens the bar on
    /// the open page, or gives its field the keyboard if it is open. Not
    /// on a database (its table has no blocks).
    pub(super) fn open_find(&mut self, ctx: &egui::Context, replace: bool) {
        if self.selected.is_none() || self.database.is_some() {
            return;
        }
        let caret = self.caret.as_ref().and_then(|c| {
            let block = self.blocks.iter().position(|b| b.id == c.block_id)?;
            Some((block, c.char))
        });
        let bar = self.find.get_or_insert_with(|| FindBar {
            from: caret,
            ..FindBar::default()
        });
        bar.replace |= replace;
        let field = if replace && !bar.query.is_empty() {
            FindField::Replacement
        } else {
            FindField::Query
        };
        bar.focus = Some(field);
        // What is in the field is chosen, so typing replaces it.
        let text_len = match field {
            FindField::Query => bar.query.chars().count(),
            FindField::Replacement => bar.replacement.chars().count(),
        };
        let mut state = egui::text_edit::TextEditState::load(ctx, field.id()).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text_len),
            )));
        state.store(ctx, field.id());
    }

    /// Closes the bar. The cursor goes to the match shown, with it chosen,
    /// so typing replaces it; with no match, the page keeps its focus.
    pub(super) fn close_find(&mut self) {
        let Some(bar) = self.find.take() else {
            return;
        };
        let Some(shown) = bar.shown() else {
            return;
        };
        if let Some(block) = self.blocks.get(shown.block) {
            self.pending_focus = Some((block.id.clone(), shown.end));
            self.pending_anchor = Some((block.id.clone(), shown.start));
        }
    }

    /// Whether one of the bar's fields has the keyboard.
    pub(super) fn find_has_focus(&self, ctx: &egui::Context) -> bool {
        self.find.is_some()
            && ctx.memory(|m| {
                m.focused().is_some_and(|id| {
                    id == FindField::Query.id() || id == FindField::Replacement.id()
                })
            })
    }

    /// Whether the bar (a field or a button) has the keyboard: then Cmd+Z
    /// and Cmd+Mayús+Z undo and redo the page, so a replacement is undone
    /// right after it was made.
    pub(super) fn find_bar_has_focus(&self, ctx: &egui::Context) -> bool {
        let Some(bar) = &self.find else {
            return false;
        };
        self.find_has_focus(ctx)
            || ctx.memory(|m| m.focused().is_some_and(|id| bar.controls.contains(&id)))
    }

    /// The bar's keys, taken before its fields see them: Intro and
    /// Mayús+Intro (or ↓ and ↑) go to the next and previous match, Intro
    /// in «Reemplazar por» replaces the one shown, Esc closes. Nothing is
    /// taken during an IME composition.
    pub(super) fn find_keys(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers as M};
        if let Some(bar) = self.find.as_mut()
            && bar.keyboard
        {
            let was_composing = bar.composing;
            bar.composing = ime_composing_after(ctx, bar.composing);
            if was_composing || bar.composing {
                // The IME sends the text as a commit; the key must not
                // also leave the field.
                take_exact_key(ctx, Key::Enter, M::NONE);
                return;
            }
        }
        if self.find.as_ref().is_some_and(|bar| bar.keyboard)
            && take_exact_key(ctx, Key::Escape, M::NONE)
        {
            self.close_find();
            return;
        }
        if !self.find_has_focus(ctx) {
            return;
        }
        let in_replacement = ctx.memory(|m| m.has_focus(FindField::Replacement.id()));
        let action = if take_exact_key(ctx, Key::Enter, M::SHIFT)
            || take_exact_key(ctx, Key::ArrowUp, M::NONE)
        {
            Some(FindAction::Step(false))
        } else if take_exact_key(ctx, Key::Enter, M::NONE) {
            Some(if in_replacement {
                FindAction::Replace
            } else {
                FindAction::Step(true)
            })
        } else if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
            Some(FindAction::Step(true))
        } else {
            None
        };
        if let Some(action) = action {
            self.run_find(action);
        }
    }

    pub(super) fn run_find(&mut self, action: FindAction) {
        if let Some(bar) = self.find.as_mut() {
            bar.refresh(&self.blocks);
        }
        match action {
            FindAction::Step(forward) => {
                if let Some(bar) = self.find.as_mut() {
                    bar.step(forward);
                }
            }
            FindAction::Replace => self.replace_shown(),
            FindAction::ReplaceAll => self.replace_all(),
            FindAction::ToggleReplace => {
                if let Some(bar) = self.find.as_mut() {
                    bar.replace = !bar.replace;
                    bar.focus = Some(if bar.replace {
                        FindField::Replacement
                    } else {
                        FindField::Query
                    });
                }
            }
            FindAction::Close => self.close_find(),
        }
    }

    /// Replaces the match shown and shows the next one. One undo step.
    fn replace_shown(&mut self) {
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let Some(shown) = bar.shown() else {
            return;
        };
        let replacement = bar.replacement.clone();
        if replace_matches(&mut self.blocks, &[shown], &replacement) == 0 {
            return;
        }
        // The next match starts after the new text, even if it contains
        // the query (replacing «a» with «aa» must not loop).
        let after = (shown.block, shown.start + replacement.chars().count());
        bar.matches = find_in_blocks(&self.blocks, &bar.query);
        bar.current = bar
            .matches
            .iter()
            .position(|m| (m.block, m.start) >= after)
            .unwrap_or(0);
        bar.reveal = !bar.matches.is_empty();
        bar.status = None;
        self.mark_dirty();
    }

    /// Replaces every match. One undo step.
    fn replace_all(&mut self) {
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        let replacement = bar.replacement.clone();
        let replaced = replace_matches(&mut self.blocks, &bar.matches, &replacement);
        if replaced == 0 {
            return;
        }
        bar.matches = find_in_blocks(&self.blocks, &bar.query);
        bar.current = 0;
        bar.status = Some(if replaced == 1 {
            "1 reemplazada".to_owned()
        } else {
            format!("{replaced} reemplazadas")
        });
        self.mark_dirty();
    }

    /// The bar, above the page and in line with its column.
    pub(super) fn find_bar(&mut self, ui: &mut egui::Ui, column: f32) {
        use egui::{Key, KeyboardShortcut as Sc, Modifiers as M};
        let Some(bar) = self.find.as_mut() else {
            return;
        };
        bar.refresh(&self.blocks);
        let p = theme::current(ui.ctx());
        let mut action = None;
        let mut controls = Vec::new();
        let mut typed = false;
        let width = ui.available_width().min(column);
        let side = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.add_space(theme::space::SM);
        ui.horizontal(|ui| {
            ui.add_space(side);
            egui::Frame::new()
                .fill(p.surface)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::radius::MD)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(width - 16.0);
                        let field_width = (width - 300.0).clamp(140.0, 320.0);
                        ui.horizontal(|ui| {
                            icons::decoration(ui, icons::SEARCH, theme::icon::MD, p.text_muted);
                            let query = ui
                                .add(
                                    egui::TextEdit::singleline(&mut bar.query)
                                        .id(FindField::Query.id())
                                        .hint_text(
                                            egui::RichText::new("Buscar en la página")
                                                .color(p.text_faint),
                                        )
                                        .desired_width(field_width),
                                )
                                .accessible_name("Buscar en la página");
                            typed = query.changed();
                            if bar.focus == Some(FindField::Query) {
                                query.request_focus();
                                bar.focus = None;
                            }
                            let counter = bar.status.clone().unwrap_or_else(|| bar.counter());
                            ui.label(egui::RichText::new(counter).color(p.text_muted));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let close = icons::button(
                                        ui,
                                        true,
                                        icons::CLOSE,
                                        p.text_muted,
                                        "Cerrar la búsqueda",
                                        Some(Sc::new(M::NONE, Key::Escape)),
                                    );
                                    controls.push(close.id);
                                    if close.clicked() {
                                        action = Some(FindAction::Close);
                                    }
                                    let toggle = ui
                                        .selectable_label(bar.replace, "Reemplazar")
                                        .on_hover_text(
                                            ui.ctx().format_shortcut(&replace_shortcut(ui.ctx())),
                                        )
                                        .accessible_name(if bar.replace {
                                            "Ocultar reemplazar"
                                        } else {
                                            "Mostrar reemplazar"
                                        });
                                    controls.push(toggle.id);
                                    if toggle.clicked() {
                                        action = Some(FindAction::ToggleReplace);
                                    }
                                    let any = !bar.matches.is_empty();
                                    let next = icons::button(
                                        ui,
                                        any,
                                        icons::CARET_DOWN,
                                        p.text_muted,
                                        "Coincidencia siguiente",
                                        Some(Sc::new(M::NONE, Key::Enter)),
                                    );
                                    controls.push(next.id);
                                    if next.clicked() {
                                        action = Some(FindAction::Step(true));
                                    }
                                    let previous = icons::button(
                                        ui,
                                        any,
                                        icons::CARET_UP,
                                        p.text_muted,
                                        "Coincidencia anterior",
                                        Some(Sc::new(M::SHIFT, Key::Enter)),
                                    );
                                    controls.push(previous.id);
                                    if previous.clicked() {
                                        action = Some(FindAction::Step(false));
                                    }
                                },
                            );
                        });
                        if bar.replace {
                            ui.horizontal(|ui| {
                                // In line with the query field, after the icon.
                                icons::decoration(
                                    ui,
                                    icons::SEARCH,
                                    theme::icon::MD,
                                    egui::Color32::TRANSPARENT,
                                );
                                let field = ui
                                    .add(
                                        egui::TextEdit::singleline(&mut bar.replacement)
                                            .id(FindField::Replacement.id())
                                            .hint_text(
                                                egui::RichText::new("Reemplazar por")
                                                    .color(p.text_faint),
                                            )
                                            .desired_width(field_width),
                                    )
                                    .accessible_name("Reemplazar por");
                                if bar.focus == Some(FindField::Replacement) {
                                    field.request_focus();
                                    bar.focus = None;
                                }
                                let any = !bar.matches.is_empty();
                                let one = ui
                                    .add_enabled(any, egui::Button::new("Reemplazar"))
                                    .on_hover_text("Intro en «Reemplazar por»");
                                controls.push(one.id);
                                if one.clicked() {
                                    action = Some(FindAction::Replace);
                                }
                                let all = ui.add_enabled(any, egui::Button::new("Reemplazar todo"));
                                controls.push(all.id);
                                if all.clicked() {
                                    action = Some(FindAction::ReplaceAll);
                                }
                            });
                        }
                    });
                });
        });
        bar.keyboard = ui.ctx().memory(|m| {
            m.focused().is_some_and(|id| {
                id == FindField::Query.id()
                    || id == FindField::Replacement.id()
                    || controls.contains(&id)
            })
        });
        bar.controls = controls;
        // Again with what was typed in the field this frame, so the blocks
        // below are painted with it.
        if typed {
            bar.refresh(&self.blocks);
        }
        if let Some(action) = action {
            self.run_find(action);
        }
    }
}

/// The shortcut that opens the bar with «Reemplazar»: Cmd+Opción+F on a
/// Mac, Ctrl+H elsewhere (Cmd/Ctrl+Alt+F works everywhere too).
pub(super) fn replace_shortcut(ctx: &egui::Context) -> egui::KeyboardShortcut {
    use egui::{Key, KeyboardShortcut as Sc, Modifiers as M};
    if ctx.os() == egui::os::OperatingSystem::Mac {
        Sc::new(M::COMMAND | M::ALT, Key::F)
    } else {
        Sc::new(M::CTRL, Key::H)
    }
}

/// The rectangles a match covers in `galley` (one per row it spans), placed
/// at `origin` (where the galley is painted).
pub(super) fn match_rects(
    galley: &egui::Galley,
    origin: egui::Pos2,
    start: usize,
    end: usize,
) -> Vec<egui::Rect> {
    let mut rects = Vec::new();
    let mut row_start = 0;
    for row in &galley.rows {
        let len = row.row.char_count_excluding_newline().0;
        let (a, b) = (start.max(row_start), end.min(row_start + len));
        if a < b {
            let x = |column: usize| {
                row.pos.x + row.row.x_offset(egui::text::CharIndex(column - row_start))
            };
            rects.push(
                egui::Rect::from_x_y_ranges(x(a)..=x(b), row.min_y()..=row.max_y())
                    .translate(origin.to_vec2()),
            );
        }
        row_start += row.char_count_including_newline().0;
        if row_start >= end {
            break;
        }
    }
    rects
}
