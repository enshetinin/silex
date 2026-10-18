use std::{collections::HashSet, path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::{
        Database, Shift,
        backup::{self, BackupJob},
    },
    editor,
    history::{EditKind, History, Snapshot, first_difference},
    model::{
        Block, BlockKind, Page, TrashItem, TreeRow, ancestors, build_tree, descendants,
        filter_tree, list_numbers,
    },
    save_state::{SaveState, SaveStatus},
};

pub struct SilexApp {
    db: Database,
    pages: Vec<Page>,
    /// Visible sidebar rows, rebuilt when pages or `expanded` change (not every frame).
    tree: Vec<TreeRow>,
    /// Pages whose children are shown; kept for the session only.
    expanded: HashSet<String>,
    trash_count: usize,
    trash_dialog: Option<TrashDialog>,
    favorites: Vec<Page>,
    /// Search results, recomputed only when the query or the pages change.
    filtered: Vec<TreeRow>,
    content_matches: Vec<Page>,
    searched_query: Option<String>,
    rename: Option<Rename>,
    selected: Option<String>,
    title: String,
    blocks: Vec<Block>,
    new_page_title: String,
    query: String,
    save: SaveState,
    /// Errors from actions other than saving (search, create, open).
    notice: Option<String>,
    close_dialog: bool,
    /// Set once the user may leave: everything saved or changes discarded.
    allow_close: bool,
    backup_job: Option<BackupJob>,
    backup_label: String,
    /// The startup backup check needs an egui context, so it runs on the first frame.
    startup_backup_pending: bool,
    restore_dialog: Option<RestoreDialog>,
    /// Bumped when the whole document set is replaced (restore), so text
    /// fields start with a fresh undo history instead of pre-restore text.
    doc_generation: u64,
    /// Cursor of the focused block as of the last frame.
    caret: Option<CaretInfo>,
    /// An IME composition is in progress: editor keys are left alone.
    ime_composing: bool,
    /// Editor key taken this frame, run after layout.
    pending_key: Option<(EditorKey, String)>,
    /// Block to focus (and cursor position) on the next frame.
    pending_focus: Option<(String, usize)>,
    /// Multi-line paste taken this frame (block id, text), run after layout.
    pending_paste: Option<(String, String)>,
    /// Last exported file, offered with "Abrir carpeta".
    last_export: Option<PathBuf>,
    /// Markdown files being read on a worker thread.
    import_job: Option<silex_desktop::import::ImportJob>,
    /// Non-Markdown files dropped with the current import (reported at the end).
    import_ignored: usize,
    /// Whole-workspace export running on a worker thread.
    export_job: Option<silex_desktop::export::WorkspaceExportJob>,
    slash: Option<SlashMenu>,
    /// Escape closed the menu for this block until its text changes.
    slash_dismissed: Option<(String, String)>,
    selection: Option<BlockSelection>,
    /// Document undo/redo for the open page (ADR 0004).
    history: History,
    /// The document as of the last recorded change (the "before" of the next one).
    shadow: Snapshot,
}

/// Cursor state of the focused block text field.
#[derive(Debug, Clone)]
struct CaretInfo {
    block_id: String,
    char: usize,
    selection: bool,
    first_row: bool,
    last_row: bool,
}

/// Structural keys handled by the block editor (see `src/editor.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorKey {
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
struct SlashMenu {
    block_id: String,
    matches: Vec<BlockKind>,
    selected: usize,
}

/// Selected blocks (by id): `anchor` stays, `head` moves with Shift+↑/↓.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockSelection {
    anchor: String,
    head: String,
}

/// Inline rename in the sidebar.
struct Rename {
    id: String,
    text: String,
    /// Focus the text field on the next frame it is drawn.
    focus: bool,
}

/// Something the user asked for on a page row (click, menu, keyboard).
enum RowAction {
    Open(String),
    Toggle(String),
    StartRename(String),
    CommitRename,
    CancelRename,
    Subpage(String),
    Favorite(String, bool),
    Reorder(String, Shift),
    Move(String, Option<String>),
    Trash(String),
}

struct TrashDialog {
    items: Vec<TrashItem>,
    error: Option<String>,
}

/// Backup picker; restoring needs an explicit second click on a selected copy.
struct RestoreDialog {
    backups: Vec<PathBuf>,
    selected: Option<usize>,
    error: Option<String>,
}

impl SilexApp {
    pub fn new(db: Database) -> Result<Self> {
        let mut pages = db.list_pages()?;
        if pages.is_empty() {
            db.create_page("Mi primera página", None)?;
            pages = db.list_pages()?;
        }
        let mut app = Self {
            db,
            pages,
            tree: Vec::new(),
            expanded: HashSet::new(),
            trash_count: 0,
            trash_dialog: None,
            favorites: Vec::new(),
            filtered: Vec::new(),
            content_matches: Vec::new(),
            searched_query: None,
            rename: None,
            selected: None,
            title: String::new(),
            blocks: Vec::new(),
            new_page_title: String::new(),
            query: String::new(),
            save: SaveState::default(),
            notice: None,
            close_dialog: false,
            allow_close: false,
            backup_job: None,
            backup_label: String::new(),
            startup_backup_pending: true,
            restore_dialog: None,
            doc_generation: 0,
            caret: None,
            ime_composing: false,
            pending_key: None,
            pending_focus: None,
            pending_paste: None,
            last_export: None,
            import_job: None,
            import_ignored: 0,
            export_job: None,
            slash: None,
            slash_dismissed: None,
            selection: None,
            history: History::default(),
            shadow: Snapshot {
                title: String::new(),
                blocks: Vec::new(),
            },
        };
        app.backup_label = app.describe_last_backup();
        app.trash_count = app.db.trash_count()?;
        app.favorites = app.db.list_favorites()?;
        app.rebuild_tree();
        if let Some(row) = app.tree.first() {
            app.open_page(row.page.id.clone())?;
        }
        Ok(app)
    }

    fn open_page(&mut self, id: String) -> Result<()> {
        self.save_now()?;
        let page = self.db.get_page(&id)?;
        if let Some(page) = page {
            self.slash = None;
            self.slash_dismissed = None;
            self.selection = None;
            self.blocks = self.db.list_blocks(&id)?;
            self.title = page.title;
            self.reset_history();
            // Make the open page visible in the tree.
            self.expanded.extend(ancestors(&self.pages, &id));
            self.selected = Some(id);
            self.rebuild_tree();
        }
        Ok(())
    }

    fn reload_pages(&mut self) -> Result<()> {
        self.pages = self.db.list_pages()?;
        self.trash_count = self.db.trash_count()?;
        self.favorites = self.db.list_favorites()?;
        self.rebuild_tree();
        self.searched_query = None;
        Ok(())
    }

    fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<()> {
        self.db.set_favorite(id, favorite)?;
        self.favorites = self.db.list_favorites()?;
        Ok(())
    }

    /// Favorites section; hidden when there are none.
    fn favorite_rows(&mut self, ui: &mut egui::Ui) {
        if self.favorites.is_empty() {
            return;
        }
        ui.label("Favoritos");
        let rows: Vec<TreeRow> = self
            .favorites
            .iter()
            .map(|page| TreeRow {
                page: page.clone(),
                depth: 0,
                has_children: false,
            })
            .collect();
        self.page_rows(ui, &rows, false, "fav");
        ui.separator();
    }

    /// Opens the first page in the tree, or shows the empty state.
    fn open_first_page(&mut self) -> Result<()> {
        match self.tree.first().map(|r| r.page.id.clone()) {
            Some(id) => self.open_page(id),
            None => {
                self.selected = None;
                self.title.clear();
                self.blocks.clear();
                self.reset_history();
                Ok(())
            }
        }
    }

    fn trash_page(&mut self, id: &str) -> Result<()> {
        self.save_now()?;
        let title = self
            .pages
            .iter()
            .find(|p| p.id == id)
            .map(|p| display_title(&p.title).to_owned())
            .unwrap_or_default();
        let trashed = self.db.trash_page(id)?;
        self.reload_pages()?;
        // Only leave the open page if it went to the trash with this branch.
        let open_still_live = self
            .selected
            .as_ref()
            .is_some_and(|sel| self.pages.iter().any(|p| &p.id == sel));
        if !open_still_live {
            self.open_first_page()?;
        }
        self.notice = Some(match trashed.saturating_sub(1) {
            0 => format!("«{title}» se movió a la papelera."),
            n => format!("«{title}» y {n} subpágina(s) se movieron a la papelera."),
        });
        Ok(())
    }

    fn open_trash_dialog(&mut self) {
        match self.db.list_trash() {
            Ok(items) => self.trash_dialog = Some(TrashDialog { items, error: None }),
            Err(err) => self.notice = Some(format!("No se puede abrir la papelera: {err:#}")),
        }
    }

    fn restore_from_trash(&mut self, id: &str) -> Result<()> {
        self.save_now()?;
        self.db.restore_page(id)?;
        self.reload_pages()?;
        self.open_page(id.to_owned())
    }

    fn trash_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.trash_dialog.as_ref() else {
            return;
        };
        let mut restore = None;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::unique("silex_trash")).show(ctx, |ui| {
            ui.heading("Papelera");
            if dialog.items.is_empty() {
                ui.label("La papelera está vacía.");
            }
            egui::ScrollArea::vertical()
                .id_salt("silex_trash_list")
                .max_height(300.0)
                .show(ui, |ui| {
                    for item in &dialog.items {
                        ui.horizontal(|ui| {
                            ui.label(display_title(&item.page.title));
                            if item.subpages > 0 {
                                ui.weak(format!("+{} subpágina(s)", item.subpages));
                            }
                            ui.weak(describe_timestamp(&item.deleted_at));
                            if ui.button("Restaurar").clicked() {
                                restore = Some(item.page.id.clone());
                            }
                        });
                    }
                });
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            close = ui.button("Cerrar").clicked();
        });
        if close || (restore.is_none() && response.should_close()) {
            self.trash_dialog = None;
            return;
        }
        let Some(id) = restore else {
            return;
        };
        match self.restore_from_trash(&id) {
            Ok(()) => self.open_trash_dialog(),
            Err(err) => {
                if let Some(dialog) = self.trash_dialog.as_mut() {
                    dialog.error = Some(format!("{err:#}"));
                }
            }
        }
    }

    fn rebuild_tree(&mut self) {
        self.tree = build_tree(&self.pages, &self.expanded);
    }

    fn toggle_expanded(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
        self.rebuild_tree();
    }

    fn create_subpage(&mut self, parent: &str) -> Result<()> {
        self.save_now()?;
        let id = self.db.create_page("Sin título", Some(parent))?;
        self.expanded.insert(parent.to_owned());
        self.reload_pages()?;
        self.open_page(id)
    }

    fn move_page(&mut self, id: &str, new_parent: Option<&str>) -> Result<()> {
        self.save_now()?;
        self.db.move_page(id, new_parent)?;
        if let Some(parent) = new_parent {
            self.expanded.insert(parent.to_owned());
            self.expanded.extend(ancestors(&self.pages, parent));
        }
        self.reload_pages()
    }

    fn reorder_page(&mut self, id: &str, shift: Shift) -> Result<()> {
        self.save_now()?;
        if self.db.reorder_page(id, shift)? {
            self.reload_pages()?;
        }
        Ok(())
    }

    /// Page rows (tree, favorites or search): indentation, optional
    /// expand arrow, title or inline rename field, and a context menu.
    /// `section` keeps widget ids distinct when a page appears twice.
    fn page_rows(&mut self, ui: &mut egui::Ui, rows: &[TreeRow], arrows: bool, section: &str) {
        let mut actions = Vec::new();
        for row in rows {
            ui.push_id((section, &row.page.id), |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 14.0);
                    if arrows {
                        let (_, response) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                        if row.has_children {
                            let openness = if self.expanded.contains(&row.page.id) {
                                1.0
                            } else {
                                0.0
                            };
                            let response = response
                                .on_hover_text("Mostrar u ocultar subpáginas")
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            egui::collapsing_header::paint_default_icon(ui, openness, &response);
                            if response.clicked() {
                                actions.push(RowAction::Toggle(row.page.id.clone()));
                            }
                        }
                    }
                    if let Some(action) = self.row_label(ui, &row.page, section) {
                        actions.push(action);
                    }
                });
            });
        }
        for action in actions {
            let result = self.apply(action);
            self.report(result);
        }
    }

    /// Title (with context menu) or, while renaming, the inline text field.
    fn row_label(&mut self, ui: &mut egui::Ui, page: &Page, section: &str) -> Option<RowAction> {
        if let Some(rename) = self.rename.as_mut().filter(|r| r.id == page.id) {
            let response = ui.add(
                TextEdit::singleline(&mut rename.text)
                    .id(egui::Id::unique(("silex-rename", section, &page.id)))
                    .desired_width(f32::INFINITY),
            );
            if rename.focus {
                response.request_focus();
                rename.focus = false;
            }
            if response.lost_focus() {
                return Some(if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    RowAction::CancelRename
                } else {
                    RowAction::CommitRename
                });
            }
            return None;
        }
        let selected = self.selected.as_deref() == Some(page.id.as_str());
        let response = ui.selectable_label(selected, display_title(&page.title));
        let mut action = response.clicked().then(|| RowAction::Open(page.id.clone()));
        response.context_menu(|ui| {
            if let Some(chosen) = self.page_menu(ui, &page.id) {
                action = Some(chosen);
                ui.close();
            }
        });
        action
    }

    /// Context menu for a page.
    fn page_menu(&self, ui: &mut egui::Ui, id: &str) -> Option<RowAction> {
        let page = self.pages.iter().find(|p| p.id == id)?;
        let siblings: Vec<&str> = self
            .pages
            .iter()
            .filter(|p| p.parent_id == page.parent_id)
            .map(|p| p.id.as_str())
            .collect();
        let index = siblings.iter().position(|s| *s == id);
        let is_favorite = self.favorites.iter().any(|p| p.id == id);
        let owned = || id.to_owned();
        let mut action = None;
        if ui.button("Abrir").clicked() {
            action = Some(RowAction::Open(owned()));
        }
        if ui.button("Renombrar").clicked() {
            action = Some(RowAction::StartRename(owned()));
        }
        if ui.button("Nueva subpágina").clicked() {
            action = Some(RowAction::Subpage(owned()));
        }
        let favorite_label = if is_favorite {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        };
        if ui.button(favorite_label).clicked() {
            action = Some(RowAction::Favorite(owned(), !is_favorite));
        }
        ui.separator();
        if ui
            .add_enabled(index.is_some_and(|i| i > 0), egui::Button::new("Subir"))
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Up));
        }
        if ui
            .add_enabled(
                index.is_some_and(|i| i + 1 < siblings.len()),
                egui::Button::new("Bajar"),
            )
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Down));
        }
        ui.menu_button("Mover a…", |ui| {
            let blocked = descendants(&self.pages, id);
            egui::ScrollArea::vertical()
                .id_salt("silex_menu_move")
                .max_height(240.0)
                .show(ui, |ui| {
                    if page.parent_id.is_some() && ui.button("Raíz").clicked() {
                        action = Some(RowAction::Move(owned(), None));
                    }
                    for target in &self.pages {
                        let valid = target.id != id
                            && !blocked.contains(&target.id)
                            && page.parent_id.as_deref() != Some(target.id.as_str());
                        if valid && ui.button(display_title(&target.title)).clicked() {
                            action = Some(RowAction::Move(owned(), Some(target.id.clone())));
                        }
                    }
                });
        });
        ui.separator();
        if ui.button("Enviar a la papelera").clicked() {
            action = Some(RowAction::Trash(owned()));
        }
        action
    }

    fn apply(&mut self, action: RowAction) -> Result<()> {
        match action {
            RowAction::Open(id) => self.open_page(id),
            RowAction::Toggle(id) => {
                self.toggle_expanded(&id);
                Ok(())
            }
            RowAction::StartRename(id) => {
                self.start_rename(&id);
                Ok(())
            }
            RowAction::CommitRename => self.commit_rename(),
            RowAction::CancelRename => {
                self.rename = None;
                Ok(())
            }
            RowAction::Subpage(id) => self.create_subpage(&id),
            RowAction::Favorite(id, favorite) => self.set_favorite(&id, favorite),
            RowAction::Reorder(id, shift) => self.reorder_page(&id, shift),
            RowAction::Move(id, parent) => self.move_page(&id, parent.as_deref()),
            RowAction::Trash(id) => self.trash_page(&id),
        }
    }

    fn start_rename(&mut self, id: &str) {
        let current = if self.selected.as_deref() == Some(id) {
            // The open page may have an unsaved title in the editor.
            Some(self.title.clone())
        } else {
            self.pages
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.title.clone())
        };
        if let Some(text) = current {
            self.rename = Some(Rename {
                id: id.to_owned(),
                text,
                focus: true,
            });
        }
    }

    fn commit_rename(&mut self) -> Result<()> {
        let Some(rename) = self.rename.take() else {
            return Ok(());
        };
        let title = match rename.text.trim() {
            "" => "Sin título".to_owned(),
            t => t.to_owned(),
        };
        if self.selected.as_deref() == Some(rename.id.as_str()) {
            // Keep the editor and the database in agreement: save through the document.
            if title != self.title {
                self.title = title;
                self.mark_dirty();
                self.save_now()?;
            }
            Ok(())
        } else {
            self.db.rename_page(&rename.id, &title)?;
            self.reload_pages()
        }
    }

    /// Recomputes search results when the query or the pages changed.
    fn refresh_search(&mut self) {
        if self.searched_query.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.searched_query = Some(self.query.clone());
        self.filtered = filter_tree(&self.pages, &self.query);
        let shown: HashSet<&str> = self.filtered.iter().map(|r| r.page.id.as_str()).collect();
        self.content_matches = match self.db.search_pages(&self.query) {
            Ok(found) => found
                .into_iter()
                .filter(|p| !shown.contains(p.id.as_str()))
                .collect(),
            Err(err) => {
                self.notice = Some(format!("Error de búsqueda: {err:#}"));
                Vec::new()
            }
        };
    }

    /// Ids of the page rows currently visible, top to bottom (for keyboard navigation).
    fn visible_page_ids(&self) -> Vec<String> {
        if self.query.trim().is_empty() {
            self.tree.iter().map(|r| r.page.id.clone()).collect()
        } else {
            self.filtered
                .iter()
                .map(|r| &r.page)
                .chain(&self.content_matches)
                .map(|p| p.id.clone())
                .collect()
        }
    }

    fn open_adjacent(&mut self, step: isize) -> Result<()> {
        let ids = self.visible_page_ids();
        if ids.is_empty() {
            return Ok(());
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|sel| ids.iter().position(|id| id == sel));
        let next = match current {
            Some(i) => i.saturating_add_signed(step).min(ids.len() - 1),
            None => 0,
        };
        if Some(next) == current {
            return Ok(());
        }
        self.open_page(ids[next].clone())
    }

    fn create_root_page(&mut self, title: &str) -> Result<()> {
        self.save_now()?;
        let id = self.db.create_page(title, None)?;
        self.reload_pages()?;
        self.open_page(id)
    }

    fn block_field_id(&self, block_id: &str) -> egui::Id {
        field_id(self.doc_generation, "block", block_id)
    }

    /// Decides, before any widget runs, whether a key is a block-editor
    /// command for the focused block, and if so takes it away from the text
    /// field. Nothing is intercepted during an IME composition, with a text
    /// selection, or when the cursor is not where the command applies.
    fn take_editor_key(&mut self, ctx: &egui::Context) {
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
    fn run_paste(&mut self, ctx: &egui::Context) {
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

    fn run_editor_key(&mut self, ctx: &egui::Context) {
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
    fn selection_range(&self) -> Option<(usize, usize)> {
        let sel = self.selection.as_ref()?;
        let a = self.blocks.iter().position(|b| b.id == sel.anchor)?;
        let h = self.blocks.iter().position(|b| b.id == sel.head)?;
        Some((a.min(h), a.max(h)))
    }

    /// Keys while blocks are selected (no text field has focus):
    /// ↑/↓ move, Shift+↑/↓ extend, Backspace/Delete delete, Cmd+C/Cmd+X copy
    /// or cut, Enter edits, Escape clears.
    fn handle_selection_keys(&mut self, ctx: &egui::Context) {
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

    fn delete_selection(&mut self, first: usize, last: usize) {
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
    fn refresh_slash_and_selection(&mut self) {
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

    /// Exports the open page as it is on screen (saved first when possible).
    fn export_current_page(&mut self) -> Result<()> {
        let dir = silex_desktop::export::export_root()?;
        self.export_current_page_to(&dir)
    }

    fn export_current_page_to(&mut self, dir: &std::path::Path) -> Result<()> {
        // A save error is shown by the status bar; export what the user sees anyway.
        let _ = self.save_now();
        let path = silex_desktop::export::export_page(dir, &self.title, &self.blocks)?;
        self.last_export = Some(path);
        Ok(())
    }

    /// "Exportada a …" with a button to open the folder.
    fn export_bar(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.last_export.clone() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(format!("Exportada a {}", path.display()));
            // A workspace export is a folder: open it; for a page, its folder.
            let folder = if path.is_dir() {
                Some(path.as_path())
            } else {
                path.parent()
            };
            if ui.small_button("Abrir carpeta").clicked()
                && let Some(dir) = folder
            {
                let result = silex_desktop::export::open_in_file_manager(dir);
                self.report(result);
            }
            if ui.small_button("Cerrar").clicked() {
                self.last_export = None;
            }
        });
    }

    /// Runs an action from a block's "…" menu.
    fn block_action(&mut self, action: BlockAction, id: &str) {
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
    fn apply_pending_focus(&mut self, ctx: &egui::Context) {
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

    /// Keyboard shortcuts, read before any widget so text fields do not see them.
    /// More specific shortcuts first: consume_shortcut ignores extra Shift/Alt.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as Sc, Modifiers as M};
        let pressed = |sc: Sc| ctx.input_mut(|i| i.consume_shortcut(&sc));
        let new_subpage = pressed(Sc::new(M::COMMAND | M::SHIFT, Key::N));
        let new_page = !new_subpage && pressed(Sc::new(M::COMMAND, Key::N));
        let search = pressed(Sc::new(M::COMMAND, Key::K));
        // Exact Alt only: Alt+Shift+↑/↓ moves blocks instead.
        let previous = take_exact_key(ctx, Key::ArrowUp, M::ALT);
        let next = take_exact_key(ctx, Key::ArrowDown, M::ALT);
        let document = self.document_has_focus(ctx);
        let redo = document
            && (pressed(Sc::new(M::COMMAND | M::SHIFT, Key::Z))
                || pressed(Sc::new(M::COMMAND, Key::Y)));
        let undo = document && !redo && pressed(Sc::new(M::COMMAND, Key::Z));
        let rename = pressed(Sc::new(M::NONE, Key::F2));
        let search_focused = ctx.memory(|m| m.has_focus(search_id()));
        let clear_search =
            search_focused && !self.query.is_empty() && pressed(Sc::new(M::NONE, Key::Escape));

        let result = (|| -> Result<()> {
            if new_subpage && let Some(id) = self.selected.clone() {
                self.create_subpage(&id)?;
            }
            if new_page {
                self.create_root_page("Sin título")?;
            }
            if search {
                ctx.memory_mut(|m| m.request_focus(search_id()));
            }
            if previous {
                self.open_adjacent(-1)?;
            }
            if next {
                self.open_adjacent(1)?;
            }
            if rename && let Some(id) = self.selected.clone() {
                self.start_rename(&id);
            }
            if clear_search {
                self.query.clear();
            }
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
            Ok(())
        })();
        self.report(result);
    }

    /// Subpage and move controls for the open page.
    fn page_actions(&mut self, ui: &mut egui::Ui, page_id: &str) {
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

    /// A structural or non-text change to the open document.
    fn mark_dirty(&mut self) {
        self.changed(EditKind::Other);
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            title: self.title.clone(),
            blocks: self.blocks.clone(),
        }
    }

    /// Records the change in the undo history (unless nothing actually
    /// changed) and schedules the autosave.
    fn changed(&mut self, kind: EditKind) {
        let now = Instant::now();
        let current = self.snapshot();
        if current != self.shadow {
            let before = std::mem::replace(&mut self.shadow, current);
            self.history.record(before, kind, now);
        }
        self.save.mark_dirty(now);
    }

    fn reset_history(&mut self) {
        self.history.clear();
        self.shadow = self.snapshot();
    }

    fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(previous) = self.history.undo(current.clone()) {
            self.apply_snapshot(previous, &current);
        }
    }

    fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(next) = self.history.redo(current.clone()) {
            self.apply_snapshot(next, &current);
        }
    }

    /// Replaces the document with `target` and puts the cursor at the end of
    /// the first block that changed.
    fn apply_snapshot(&mut self, target: Snapshot, from: &Snapshot) {
        let focus = first_difference(&from.blocks, &target.blocks);
        self.title = target.title;
        self.blocks = target.blocks;
        self.shadow = self.snapshot();
        // New field ids: egui must not keep cursors or text state from before.
        self.doc_generation += 1;
        self.selection = None;
        self.slash = None;
        self.pending_focus = focus
            .and_then(|i| self.blocks.get(i.min(self.blocks.len().saturating_sub(1))))
            .map(|b| (b.id.clone(), editor::block_char_len(b)));
        self.save.mark_dirty(Instant::now());
    }

    /// True when Cmd+Z should act on the document: no field has focus (e.g.
    /// blocks are selected) or the title or a block has it. The search and
    /// rename fields keep their own text undo.
    fn document_has_focus(&self, ctx: &egui::Context) -> bool {
        let Some(focused) = ctx.memory(|m| m.focused()) else {
            return true;
        };
        let page = self.selected.as_deref().unwrap_or_default();
        focused == field_id(self.doc_generation, "title", page)
            || self
                .blocks
                .iter()
                .any(|b| focused == self.block_field_id(&b.id))
    }

    /// Saves pending edits now. On failure the edits stay in memory, the
    /// error is shown in the status bar and a retry is scheduled.
    fn save_now(&mut self) -> Result<()> {
        if !self.save.is_dirty() {
            return Ok(());
        }
        let Some(id) = self.selected.clone() else {
            self.save.record_success();
            return Ok(());
        };
        match self.db.save_document(&id, &self.title, &self.blocks) {
            Ok(()) => {
                self.save.record_success();
                // The save already succeeded; a failed list refresh is only a notice.
                let refreshed = self.reload_pages();
                self.report(refreshed);
                Ok(())
            }
            Err(err) => {
                self.save.record_failure(Instant::now(), format!("{err:#}"));
                Err(err)
            }
        }
    }

    fn describe_last_backup(&self) -> String {
        let Some(path) = self.db.path() else {
            return "Copias no disponibles (base en memoria)".into();
        };
        match backup::newest_stamp(&backup::backups_dir(&path)) {
            Ok(Some(stamp)) => format!("Última copia: {}", backup::describe_stamp(&stamp)),
            Ok(None) => "Aún no hay copias".into(),
            Err(err) => format!("Copias: {err:#}"),
        }
    }

    fn start_backup(&mut self, ctx: &egui::Context) {
        if self.backup_job.is_some() {
            return;
        }
        let Some(source) = self.db.path() else {
            return;
        };
        let dir = backup::backups_dir(&source);
        let ctx = ctx.clone();
        match BackupJob::spawn(source, dir, move || ctx.request_repaint()) {
            Ok(job) => {
                self.backup_job = Some(job);
                self.backup_label = "Creando copia…".into();
            }
            Err(err) => self.notice = Some(format!("Copia de seguridad: {err:#}")),
        }
    }

    /// Automatic backup at startup when the newest one is too old, then
    /// collects results from the worker without blocking the UI.
    fn tick_backups(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.startup_backup_pending)
            && let Some(path) = self.db.path()
        {
            match self.db.backup_due(&backup::backups_dir(&path)) {
                Ok(true) => self.start_backup(ctx),
                Ok(false) => {}
                Err(err) => self.notice = Some(format!("Copia de seguridad: {err:#}")),
            }
        }
        let Some(result) = self.backup_job.as_ref().and_then(BackupJob::poll) else {
            return;
        };
        self.backup_job = None;
        match result {
            Ok(_) => self.backup_label = self.describe_last_backup(),
            Err(err) => {
                self.backup_label = "La última copia falló".into();
                self.notice = Some(format!("Copia de seguridad fallida: {err:#}"));
            }
        }
    }

    fn open_restore_dialog(&mut self) {
        let Some(path) = self.db.path() else {
            return;
        };
        match backup::list_backups(&backup::backups_dir(&path)) {
            Ok(backups) => {
                self.restore_dialog = Some(RestoreDialog {
                    backups,
                    selected: None,
                    error: None,
                })
            }
            Err(err) => self.notice = Some(format!("No se pueden listar las copias: {err:#}")),
        }
    }

    /// Saves pending edits (so they are part of the pre-restore copy), restores
    /// and reloads the UI. Reopens the same page if it exists in the copy.
    fn restore(&mut self, backup_file: &std::path::Path) -> Result<PathBuf> {
        self.save_now()
            .context("Hay cambios sin guardar; guárdalos antes de restaurar")?;
        let safety = self.db.restore_from_backup(backup_file)?;
        self.doc_generation += 1;
        let previous = self.selected.take();
        self.title.clear();
        self.blocks.clear();
        self.reload_pages()?;
        let reopen = previous
            .filter(|id| self.pages.iter().any(|p| &p.id == id))
            .or_else(|| self.pages.first().map(|p| p.id.clone()));
        if let Some(id) = reopen {
            self.open_page(id)?;
        }
        self.backup_label = self.describe_last_backup();
        Ok(safety)
    }

    fn restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.restore_dialog.as_mut() else {
            return;
        };
        let mut confirm = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("silex_restore")).show(ctx, |ui| {
            ui.heading("Restaurar una copia");
            if dialog.backups.is_empty() {
                ui.label("Aún no hay copias de seguridad.");
            }
            egui::ScrollArea::vertical()
                .id_salt("silex_restore_list")
                .max_height(260.0)
                .show(ui, |ui| {
                    for (i, path) in dialog.backups.iter().enumerate() {
                        let label = backup::stamp_of(path)
                            .map(backup::describe_stamp)
                            .unwrap_or_else(|| path.display().to_string());
                        if ui
                            .selectable_label(dialog.selected == Some(i), label)
                            .clicked()
                        {
                            dialog.selected = Some(i);
                            dialog.error = None;
                        }
                    }
                });
            if let Some(path) = dialog.selected.and_then(|i| dialog.backups.get(i)) {
                ui.add_space(8.0);
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Todo el contenido actual se reemplazará por esta copia. \
                     Antes se guardará una copia del estado actual, \
                     que podrás restaurar si cambias de opinión.",
                );
                if ui.button("Restaurar esta copia").clicked() {
                    confirm = Some(path.clone());
                }
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            if ui.button("Cancelar").clicked() {
                cancel = true;
            }
        });
        if cancel || (confirm.is_none() && response.should_close()) {
            self.restore_dialog = None;
            return;
        }
        let Some(path) = confirm else {
            return;
        };
        match self.restore(&path) {
            Ok(safety) => {
                self.restore_dialog = None;
                let describe = |p: &std::path::Path| {
                    backup::stamp_of(p)
                        .map(backup::describe_stamp)
                        .unwrap_or_else(|| p.display().to_string())
                };
                self.notice = Some(format!(
                    "Copia del {} restaurada. El estado anterior se guardó como copia del {}.",
                    describe(&path),
                    describe(&safety)
                ));
            }
            Err(err) => {
                if let Some(dialog) = self.restore_dialog.as_mut() {
                    dialog.error = Some(format!("{err:#}"));
                }
            }
        }
    }

    fn report<T>(&mut self, result: Result<T>) {
        if let Err(err) = result {
            self.notice = Some(format!("{err:#}"));
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.heading("Silex");
        ui.label("Tu espacio local");
        ui.separator();
        ui.label("Buscar (título y contenido)");
        ui.add(
            TextEdit::singleline(&mut self.query)
                .id(search_id())
                .hint_text(format!("Buscar… ({}+K)", cmd_label(ui.ctx()))),
        );
        ui.separator();
        if self.query.trim().is_empty() {
            self.favorite_rows(ui);
            ui.label("Páginas");
            let rows = self.tree.clone();
            egui::ScrollArea::vertical()
                .id_salt("silex_sidebar_tree")
                .max_height(400.0)
                .show(ui, |ui| self.page_rows(ui, &rows, true, "tree"));
        } else {
            self.search_results(ui);
        }
        ui.separator();
        ui.add(TextEdit::singleline(&mut self.new_page_title).hint_text("Nombre de página"));
        if ui.button("+ Crear página").clicked() {
            let title = match self.new_page_title.trim() {
                "" => "Sin título".to_owned(),
                t => t.to_owned(),
            };
            let result = self.create_root_page(&title);
            if result.is_ok() {
                self.new_page_title.clear();
            }
            self.report(result);
        }
        if ui
            .button(format!("Papelera ({})", self.trash_count))
            .clicked()
        {
            self.open_trash_dialog();
        }
        let can_export = self.export_job.is_none() && self.db.path().is_some();
        let label = if self.export_job.is_some() {
            "Exportando…"
        } else {
            "Exportar todo a Markdown"
        };
        if ui
            .add_enabled(can_export, egui::Button::new(label))
            .on_hover_text("Todas las páginas (sin la papelera) en una carpeta nueva")
            .clicked()
        {
            let result = silex_desktop::export::export_root()
                .and_then(|root| self.start_workspace_export(ui.ctx(), root));
            self.report(result);
        }
        Self::shortcuts_help(ui);
        ui.separator();
        ui.small(&self.backup_label);
        let can_backup = self.backup_job.is_none() && self.db.path().is_some();
        if ui
            .add_enabled(can_backup, egui::Button::new("Crear copia ahora"))
            .clicked()
        {
            self.start_backup(ui.ctx());
        }
        if ui
            .add_enabled(can_backup, egui::Button::new("Restaurar una copia…"))
            .clicked()
        {
            self.open_restore_dialog();
        }
    }

    /// Search: title matches as a filtered tree, then pages that only match
    /// in their content (FTS, which also ignores accents).
    fn search_results(&mut self, ui: &mut egui::Ui) {
        self.refresh_search();
        let filtered = self.filtered.clone();
        let content: Vec<TreeRow> = self
            .content_matches
            .iter()
            .map(|page| TreeRow {
                page: page.clone(),
                depth: 0,
                has_children: false,
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("silex_sidebar_search")
            .max_height(400.0)
            .show(ui, |ui| {
                ui.label("Páginas");
                if filtered.is_empty() {
                    ui.weak("Ningún título coincide.");
                }
                self.page_rows(ui, &filtered, false, "filter");
                if !content.is_empty() {
                    ui.add_space(6.0);
                    ui.label("En el contenido");
                    self.page_rows(ui, &content, false, "content");
                }
            });
    }

    fn shortcuts_help(ui: &mut egui::Ui) {
        let cmd = cmd_label(ui.ctx());
        ui.collapsing("Atajos de teclado", |ui| {
            for (keys, what) in [
                (format!("{cmd}+N"), "Nueva página"),
                (format!("{cmd}+Mayús+N"), "Nueva subpágina"),
                (format!("{cmd}+K"), "Buscar"),
                ("Alt+↑ / Alt+↓".to_owned(), "Página anterior / siguiente"),
                ("F2".to_owned(), "Renombrar la página abierta"),
                (format!("{cmd}+S"), "Guardar ahora"),
                (
                    format!("{cmd}+Z / {cmd}+Mayús+Z"),
                    "Deshacer / rehacer texto",
                ),
                ("Clic derecho".to_owned(), "Acciones de una página"),
            ] {
                ui.horizontal(|ui| {
                    ui.monospace(keys);
                    ui.label(what);
                });
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
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
        let changed = ui
            .add(
                TextEdit::singleline(&mut self.title)
                    .id(field_id(self.doc_generation, "title", &page_id))
                    .font(egui::TextStyle::Heading)
                    .desired_width(f32::INFINITY),
            )
            .changed();
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

    /// Sidebar and editor columns.
    fn layout(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(230.0);
                self.sidebar(ui);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(500.0);
                self.editor(ui);
            });
        });
    }

    /// Starts exporting every page to a new folder under `root`, in the background.
    fn start_workspace_export(&mut self, ctx: &egui::Context, root: PathBuf) -> Result<()> {
        // A save error is shown by the status bar; the export reads the database.
        let _ = self.save_now();
        let db_path = self
            .db
            .path()
            .context("No se puede exportar una base en memoria")?;
        let repaint = ctx.clone();
        self.export_job = Some(silex_desktop::export::WorkspaceExportJob::spawn(
            db_path,
            root,
            move || repaint.request_repaint(),
        )?);
        Ok(())
    }

    fn tick_workspace_export(&mut self) {
        let Some(result) = self.export_job.as_ref().and_then(|j| j.poll()) else {
            return;
        };
        self.export_job = None;
        match result {
            Ok(export) => {
                self.notice = Some(format!(
                    "Espacio de trabajo exportado ({} página(s)).",
                    export.pages
                ));
                self.last_export = Some(export.dir);
            }
            Err(err) => self.notice = Some(format!("No se pudo exportar: {err:#}")),
        }
    }

    /// Files dropped on the window: `.md` files are imported in the background.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let (markdown, other): (Vec<PathBuf>, Vec<PathBuf>) = dropped
            .into_iter()
            .partition(|p| silex_desktop::import::is_markdown_path(p));
        if markdown.is_empty() {
            self.notice = Some(format!(
                "Solo se importan archivos .md ({} ignorado(s)).",
                other.len()
            ));
            return;
        }
        if self.import_job.is_some() {
            self.notice = Some("Ya hay una importación en curso.".into());
            return;
        }
        // Reported together with the result, so one message does not hide the other.
        self.import_ignored = other.len();
        let repaint = ctx.clone();
        match silex_desktop::import::ImportJob::spawn(markdown, move || repaint.request_repaint()) {
            Ok(job) => self.import_job = Some(job),
            Err(err) => self.notice = Some(format!("Importación: {err:#}")),
        }
    }

    /// Creates the pages read by the import job (one transaction each) and
    /// opens the last one.
    fn tick_import(&mut self) {
        let Some(results) = self.import_job.as_ref().and_then(|j| j.poll()) else {
            return;
        };
        self.import_job = None;
        let mut created = Vec::new();
        let mut errors = Vec::new();
        for (path, result) in results {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match result.and_then(|page| {
                self.db
                    .create_page_with_blocks(&page.title, None, &page.blocks)
            }) {
                Ok(id) => created.push(id),
                Err(err) => errors.push(format!("{name}: {err:#}")),
            }
        }
        let outcome = (|| -> Result<()> {
            self.reload_pages()?;
            if let Some(last) = created.last() {
                self.open_page(last.clone())?;
            }
            Ok(())
        })();
        self.report(outcome);
        let mut message = format!("Importada(s) {} página(s).", created.len());
        let ignored = std::mem::take(&mut self.import_ignored);
        if ignored > 0 {
            message.push_str(&format!(
                " Solo se importan archivos .md ({ignored} ignorado(s))."
            ));
        }
        if !errors.is_empty() {
            message.push_str(&format!(" No se importaron: {}", errors.join("; ")));
        }
        self.notice = Some(message);
    }

    /// Centered hint while files are dragged over the window.
    fn drop_hint(ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        egui::Area::new(egui::Id::unique("silex_drop_hint"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.heading("Suelta archivos .md para importarlos");
                });
            });
    }

    /// Always-visible save state; an error offers an immediate retry.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let failed = match self.save.status() {
            SaveStatus::Saved => {
                ui.label("Guardado");
                None
            }
            SaveStatus::Dirty => {
                ui.label("Cambios sin guardar…");
                None
            }
            SaveStatus::Failed { message } => Some(message.to_owned()),
        };
        if let Some(message) = failed {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Error al guardar: {message}"),
            );
            if ui.button("Reintentar").clicked() {
                let _ = self.save_now();
            }
        }
    }

    fn notice_bar(&mut self, ui: &mut egui::Ui) {
        let Some(notice) = self.notice.clone() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, notice);
            if ui.small_button("Cerrar aviso").clicked() {
                self.notice = None;
            }
        });
    }

    /// Intercepts window close: save first; if that fails, keep the window
    /// open and let the user retry or explicitly discard.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if self.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let _ = self.save_now();
        if !self.save.can_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_dialog = true;
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        if !self.close_dialog {
            return;
        }
        let error = match self.save.status() {
            SaveStatus::Failed { message } => message.to_owned(),
            _ => "Hay cambios pendientes de guardar.".to_owned(),
        };
        let mut action = None;
        let response = egui::Modal::new(egui::Id::unique("silex_close_unsaved")).show(ctx, |ui| {
            ui.heading("No se pudieron guardar los cambios");
            ui.label(format!("Error: {error}"));
            ui.label("Si sales ahora, se perderán los cambios no guardados de esta página.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Reintentar").clicked() {
                    action = Some(CloseAction::Retry);
                }
                if ui.button("Salir sin guardar").clicked() {
                    action = Some(CloseAction::Discard);
                }
                if ui.button("Cancelar").clicked() {
                    action = Some(CloseAction::Cancel);
                }
            });
        });
        if action.is_none() && response.should_close() {
            action = Some(CloseAction::Cancel);
        }
        match action {
            Some(CloseAction::Retry) => {
                if self.save_now().is_ok() {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Some(CloseAction::Discard) => {
                self.save.discard();
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(CloseAction::Cancel) => self.close_dialog = false,
            None => {}
        }
    }
}

enum CloseAction {
    Retry,
    Discard,
    Cancel,
}

impl SilexApp {
    /// One frame: shortcuts, layout, dialogs, background work and autosave.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);
        self.take_editor_key(&ctx);
        self.apply_pending_focus(&ctx);
        self.handle_selection_keys(&ctx);
        self.layout(ui);
        self.run_paste(&ctx);
        self.run_editor_key(&ctx);
        self.refresh_slash_and_selection();
        // Save errors are shown by the status bar, so results are not reported twice.
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            let _ = self.save_now();
        }
        self.tick_backups(&ctx);
        self.tick_import();
        self.tick_workspace_export();
        Self::drop_hint(&ctx);
        let now = Instant::now();
        if self.save.is_due(now) {
            let _ = self.save_now();
        }
        self.handle_close_request(&ctx);
        self.close_dialog(&ctx);
        self.restore_dialog(&ctx);
        self.trash_dialog(&ctx);
        // The UI may be idle between edits; wake up for the next autosave or retry.
        if let Some(at) = self.save.next_attempt() {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
    }
}

impl eframe::App for SilexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update_ui(ui);
    }

    /// Last-chance save when the app shuts down without a close request.
    fn on_exit(&mut self) {
        if let Err(err) = self.save_now() {
            eprintln!("Silex: no se guardaron cambios al cerrar: {err:#}");
        }
    }
}

/// Safety net in case `on_exit` did not run; a second save is a no-op.
impl Drop for SilexApp {
    fn drop(&mut self) {
        if let Err(err) = self.save_now() {
            eprintln!("Silex: no se guardaron cambios al cerrar: {err:#}");
        }
    }
}

/// Text field ids follow the page/block id, not the position on screen, so
/// each field keeps its own undo history (egui: Cmd/Ctrl+Z, Shift+Cmd+Z or
/// Ctrl+Y) when blocks are added or pages switched. See ADR 0002.
fn field_id(generation: u64, kind: &str, id: &str) -> egui::Id {
    egui::Id::unique(("silex-field", generation, kind, id))
}

/// Horizontal space per indent level.
const INDENT_WIDTH: f32 = 24.0;

/// Renders one block: type selector, indent, marker (checkbox, bullet,
/// number or quote bar) and its text field. `number` is the list number for
/// numbered blocks. Returns true when anything in the block changed.
/// Actions in a block's "…" menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockAction {
    MoveUp,
    MoveDown,
    Duplicate,
    Delete,
}

/// What a block reported while being drawn.
struct BlockOutput {
    /// The text was edited (groups into one undo step while typing).
    text_changed: bool,
    /// Type or checkbox changed.
    other_changed: bool,
    action: Option<BlockAction>,
    /// Present when this block's text field has focus.
    caret: Option<CaretInfo>,
}

fn block_field(
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
fn slash_menu_ui(ui: &mut egui::Ui, menu: &SlashMenu) -> Option<BlockKind> {
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
fn take_event(ctx: &egui::Context, mut wanted: impl FnMut(&egui::Event) -> bool) -> bool {
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
fn take_exact_key(ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) -> bool {
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
fn ime_composing_after(ctx: &egui::Context, mut composing: bool) -> bool {
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
fn delete_char(text: &mut String, char_index: usize) {
    if let Some((byte, _)) = text.char_indices().nth(char_index) {
        text.remove(byte);
    }
}

fn heading_size(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading1 => 24.0,
        BlockKind::Heading2 => 20.0,
        _ => 16.0,
    }
}

/// The "…" menu in front of each block.
fn block_menu(ui: &mut egui::Ui) -> Option<BlockAction> {
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
fn kind_selector(ui: &mut egui::Ui, block: &mut Block) -> bool {
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

fn search_id() -> egui::Id {
    egui::Id::unique("silex-search")
}

/// "Cmd" on macOS, "Ctrl" elsewhere (egui's COMMAND modifier).
fn cmd_label(ctx: &egui::Context) -> &'static str {
    if ctx.os() == egui::os::OperatingSystem::Mac {
        "Cmd"
    } else {
        "Ctrl"
    }
}

/// `2026-10-18T09:30:05.123Z` → `2026-10-18 09:30 UTC`.
fn describe_timestamp(at: &str) -> String {
    match (at.get(0..10), at.get(11..16)) {
        (Some(day), Some(time)) => format!("{day} {time} UTC"),
        _ => at.to_owned(),
    }
}

fn display_title(title: &str) -> &str {
    if title.trim().is_empty() {
        "Sin título"
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str) -> Block {
        Block {
            id: id.into(),
            page_id: "p".into(),
            kind: BlockKind::Paragraph,
            content: String::new(),
            checked: false,
            position: 0,
            indent: 0,
        }
    }

    /// Headless egui: runs one frame with the given events at `time` seconds.
    fn frame(
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        blocks: &mut [Block],
        focus: Option<egui::Id>,
    ) -> bool {
        let input = egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut changed = false;
        let mut output = ctx.run_ui(input, |ui| {
            if let Some(id) = focus {
                ui.memory_mut(|m| m.request_focus(id));
            }
            for block in blocks.iter_mut() {
                changed |= block_field(ui, 0, block, None, false, false).text_changed;
            }
        });
        // Headless: nothing renders the font texture, so drop it explicitly.
        output.textures_delta.clear();
        changed
    }

    fn type_text(text: &str) -> Vec<egui::Event> {
        vec![egui::Event::Text(text.into())]
    }

    fn undo() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]
    }

    /// Collects all text painted in a frame (egui paints ID-clash warnings as text).
    fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    #[test]
    fn main_layout_has_no_id_clashes() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let page = app.selected.clone().expect("first page is open");
        for kind in BlockKind::ALL {
            app.db.add_block(&page, kind)?;
        }
        app.blocks = app.db.list_blocks(&page)?;
        app.reset_history();
        // A nested, expanded tree: page ─ child ─ grandchild, plus a second child.
        let child = app.db.create_page("Hija", Some(&page))?;
        app.db.create_page("Nieta", Some(&child))?;
        app.db.create_page("Hija 2", Some(&page))?;
        app.db.set_favorite(&child, true)?;
        app.expanded.extend([page.clone(), child]);
        app.reload_pages()?;
        assert_eq!(app.favorites.len(), 1);
        let depths: Vec<usize> = app.tree.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 1, 2, 1]);

        let ctx = egui::Context::default();
        let mut warnings = Vec::new();
        // Two frames: egui reports clashes against the previous frame's widgets too.
        for time in [0.0, 0.1] {
            let input = egui::RawInput {
                time: Some(time),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| app.layout(ui));
            output.textures_delta.clear();
            warnings = painted_text(&output.shapes)
                .into_iter()
                .filter(|t| t.contains(" use of ") && t.contains(" ID "))
                .collect();
        }
        assert!(warnings.is_empty(), "egui ID clashes: {warnings:?}");
        Ok(())
    }

    #[test]
    fn trash_and_restore_from_the_app() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let first = app.selected.clone().expect("first page is open");
        app.create_subpage(&first)?;
        let child = app.selected.clone().expect("subpage is open");
        app.db.create_page("Otra", None)?;
        app.reload_pages()?;
        app.open_page(first.clone())?;

        // Unsaved edits are saved before trashing.
        app.title = "Primera editada".into();
        app.mark_dirty();
        app.trash_page(&first)?;
        assert_eq!(app.trash_count, 1);
        assert!(app.pages.iter().all(|p| p.id != first && p.id != child));
        assert_ne!(app.selected.as_deref(), Some(first.as_str()));
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("1 subpágina"))
        );

        app.restore_from_trash(&first)?;
        assert_eq!(app.trash_count, 0);
        assert_eq!(app.selected.as_deref(), Some(first.as_str()));
        assert_eq!(app.title, "Primera editada");
        assert!(app.pages.iter().any(|p| p.id == child));
        Ok(())
    }

    #[test]
    fn trashing_the_last_page_shows_the_empty_state() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let only = app.selected.clone().expect("first page is open");
        app.trash_page(&only)?;
        assert_eq!(app.selected, None);
        assert!(app.tree.is_empty());
        Ok(())
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Runs one full app frame (shortcuts, layout, dialogs) with `events`.
    fn app_frame(app: &mut SilexApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
        output.textures_delta.clear();
        painted_text(&output.shapes)
    }

    fn page_titles(app: &SilexApp) -> Vec<String> {
        app.pages.iter().map(|p| p.title.clone()).collect()
    }

    #[test]
    fn keyboard_shortcuts_create_and_navigate() -> anyhow::Result<()> {
        use egui::{Key, Modifiers as M};
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let ctx = egui::Context::default();
        let first = app.selected.clone().expect("first page");

        app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND)]);
        let second = app.selected.clone().expect("new page is open");
        assert_ne!(second, first);
        assert_eq!(app.pages.len(), 2);

        // Cmd+Shift+N makes a subpage only (not also a root page).
        app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND | M::SHIFT)]);
        let child = app.selected.clone().expect("subpage is open");
        assert_eq!(app.pages.len(), 3);
        let parent = app
            .pages
            .iter()
            .find(|p| p.id == child)
            .and_then(|p| p.parent_id.clone());
        assert_eq!(parent, Some(second.clone()));

        // Visible tree: first, second, child. Alt+Up walks back up.
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
        assert_eq!(app.selected.as_deref(), Some(second.as_str()));
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
        assert_eq!(app.selected.as_deref(), Some(first.as_str()));
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
        assert_eq!(
            app.selected.as_deref(),
            Some(first.as_str()),
            "stays at the top"
        );
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::ALT)]);
        assert_eq!(app.selected.as_deref(), Some(second.as_str()));

        // Cmd+K focuses the search field; F2 starts renaming the open page.
        app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
        app_frame(&mut app, &ctx, vec![]);
        assert!(ctx.memory(|m| m.has_focus(search_id())));
        app_frame(&mut app, &ctx, vec![key(Key::F2, M::NONE)]);
        assert_eq!(
            app.rename.as_ref().map(|r| r.id.as_str()),
            Some(second.as_str())
        );
        Ok(())
    }

    #[test]
    fn rename_from_sidebar_keeps_editor_and_database_in_sync() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let open = app.selected.clone().expect("first page");
        let other = app.db.create_page("Otra", None)?;
        app.reload_pages()?;

        // Renaming another page goes straight to the database.
        app.start_rename(&other);
        if let Some(r) = app.rename.as_mut() {
            r.text = "  Renombrada  ".into();
        }
        app.commit_rename()?;
        assert_eq!(
            app.db.get_page(&other)?.map(|p| p.title).as_deref(),
            Some("Renombrada")
        );
        assert_eq!(app.selected.as_deref(), Some(open.as_str()));

        // The open page starts from the editor's unsaved title and saves through it.
        app.title = "Sin guardar".into();
        app.mark_dirty();
        app.start_rename(&open);
        assert_eq!(
            app.rename.as_ref().map(|r| r.text.as_str()),
            Some("Sin guardar")
        );
        if let Some(r) = app.rename.as_mut() {
            r.text = "Definitivo".into();
        }
        app.commit_rename()?;
        assert_eq!(app.title, "Definitivo");
        assert!(app.save.can_close(), "saved");
        assert_eq!(
            app.db.get_page(&open)?.map(|p| p.title).as_deref(),
            Some("Definitivo")
        );

        // Empty becomes "Sin título"; cancelling changes nothing.
        app.start_rename(&other);
        if let Some(r) = app.rename.as_mut() {
            r.text = "   ".into();
        }
        app.commit_rename()?;
        assert_eq!(
            app.db.get_page(&other)?.map(|p| p.title).as_deref(),
            Some("Sin título")
        );
        app.start_rename(&other);
        app.apply(RowAction::CancelRename)?;
        assert!(app.rename.is_none());
        assert_eq!(
            app.db.get_page(&other)?.map(|p| p.title).as_deref(),
            Some("Sin título")
        );
        Ok(())
    }

    #[test]
    fn trashing_another_page_keeps_the_open_one() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let open = app.selected.clone().expect("first page");
        let other = app.db.create_page("Para borrar", None)?;
        app.reload_pages()?;
        app.apply(RowAction::Trash(other))?;
        assert_eq!(app.selected.as_deref(), Some(open.as_str()));
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("Para borrar"))
        );
        Ok(())
    }

    #[test]
    fn search_filters_titles_and_lists_content_matches() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let recetas = app.db.create_page("Recetas", None)?;
        app.db.create_page("Pan de centeno", Some(&recetas))?;
        let notas = app.db.create_page("Notas", None)?;
        let block = app.db.add_block(&notas, BlockKind::Paragraph)?;
        let mut blocks = app.db.list_blocks(&notas)?;
        blocks[0].content = "comprar pan".into();
        assert_eq!(blocks[0].id, block);
        app.db.save_document(&notas, "Notas", &blocks)?;
        app.reload_pages()?;

        app.query = "pan".into();
        app.refresh_search();
        let filtered: Vec<(String, usize)> = app
            .filtered
            .iter()
            .map(|r| (r.page.title.clone(), r.depth))
            .collect();
        assert_eq!(
            filtered,
            vec![("Recetas".into(), 0), ("Pan de centeno".into(), 1)]
        );
        let content: Vec<&str> = app
            .content_matches
            .iter()
            .map(|p| p.title.as_str())
            .collect();
        assert_eq!(content, vec!["Notas"]);
        assert_eq!(app.visible_page_ids().len(), 3);
        assert_eq!(page_titles(&app).len(), 4);
        Ok(())
    }

    #[test]
    fn sidebar_states_have_no_id_clashes() -> anyhow::Result<()> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let open = app.selected.clone().expect("first page");
        // The same page in favorites and in the tree, plus an inline rename.
        app.db.set_favorite(&open, true)?;
        let child = app.db.create_page("Hija", Some(&open))?;
        app.expanded.insert(open.clone());
        app.reload_pages()?;
        app.start_rename(&child);
        let ctx = egui::Context::default();
        for query in ["", "Hija"] {
            app.query = query.into();
            for _ in 0..2 {
                let warnings: Vec<String> = app_frame(&mut app, &ctx, vec![])
                    .into_iter()
                    .filter(|t| t.contains(" use of ") && t.contains(" ID "))
                    .collect();
                assert!(warnings.is_empty(), "query {query:?}: {warnings:?}");
            }
        }
        Ok(())
    }

    /// App with one page whose blocks are `(kind, indent, text)`, the given
    /// block focused at `char`, after a frame so the caret is known.
    fn editor_app(
        specs: &[(BlockKind, u8, &str)],
        focus: usize,
        char: usize,
    ) -> anyhow::Result<(SilexApp, egui::Context)> {
        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let page = app.selected.clone().expect("page");
        for (kind, _, _) in specs {
            app.db.add_block(&page, *kind)?;
        }
        let mut blocks = app.db.list_blocks(&page)?;
        for (block, (_, indent, text)) in blocks.iter_mut().zip(specs) {
            block.indent = *indent;
            block.content = (*text).to_owned();
        }
        let title = app.title.clone();
        app.db.save_document(&page, &title, &blocks)?;
        app.blocks = blocks;
        // As open_page does: the loaded document is the starting point for undo.
        app.reset_history();
        let ctx = egui::Context::default();
        app.pending_focus = Some((app.blocks[focus].id.clone(), char));
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![]);
        Ok((app, ctx))
    }

    fn contents(app: &SilexApp) -> Vec<(BlockKind, u8, String)> {
        app.blocks
            .iter()
            .map(|b| (b.kind, b.indent, b.content.clone()))
            .collect()
    }

    fn focused_block(app: &SilexApp, ctx: &egui::Context) -> Option<usize> {
        app.blocks
            .iter()
            .position(|b| ctx.memory(|m| m.has_focus(app.block_field_id(&b.id))))
    }

    fn caret_char(app: &SilexApp) -> Option<usize> {
        app.caret.as_ref().map(|c| c.char)
    }

    use BlockKind::{Bulleted as Bul, Code as Cod, Paragraph as Par};
    use egui::{Key, Modifiers as M};

    #[test]
    fn enter_splits_and_focuses_the_new_block() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "hola🙂mundo")], 0, 5)?;
        assert_eq!(caret_char(&app), Some(5));
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(
            contents(&app),
            [(Par, 0, "hola🙂".into()), (Par, 0, "mundo".into())]
        );
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(1));
        assert_eq!(caret_char(&app), Some(0));

        // The split is persisted by the normal save path.
        assert!(!app.save.can_close());
        app.save_now()?;
        let page = app.selected.clone().expect("page");
        assert_eq!(app.db.list_blocks(&page)?.len(), 2);
        Ok(())
    }

    #[test]
    fn shift_enter_is_a_newline_inside_the_block() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 1)?;
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::SHIFT)]);
        assert_eq!(contents(&app), [(Par, 0, "a\nb".into())]);
        Ok(())
    }

    #[test]
    fn enter_in_code_block_is_a_newline() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Cod, 0, "fn x()")], 0, 6)?;
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(app.blocks.len(), 1);
        assert!(app.blocks[0].content.contains('\n'));
        Ok(())
    }

    #[test]
    fn text_typed_before_enter_in_the_same_frame_is_kept_in_order() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
        app_frame(
            &mut app,
            &ctx,
            vec![egui::Event::Text("X".into()), key(Key::Enter, M::NONE)],
        );
        assert_eq!(
            contents(&app),
            [(Par, 0, "abX".into()), (Par, 0, "".into())]
        );
        Ok(())
    }

    #[test]
    fn backspace_at_start_merges_and_mid_text_deletes() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "Café"), (Par, 0, " ☕")], 1, 0)?;
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        assert_eq!(contents(&app), [(Par, 0, "Café ☕".into())]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(0));
        assert_eq!(caret_char(&app), Some(4), "cursor at the seam");

        // Not at the start: a normal character delete by the text field.
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        assert_eq!(contents(&app), [(Par, 0, "Caf ☕".into())]);
        Ok(())
    }

    #[test]
    fn tab_and_shift_tab_change_indent_and_keep_focus() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Bul, 0, "uno"), (Bul, 0, "dos")], 1, 1)?;
        app_frame(&mut app, &ctx, vec![key(Key::Tab, M::NONE)]);
        assert_eq!(app.blocks[1].indent, 1);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(1));
        app_frame(&mut app, &ctx, vec![key(Key::Tab, M::SHIFT)]);
        assert_eq!(app.blocks[1].indent, 0);
        Ok(())
    }

    #[test]
    fn arrows_move_between_blocks_at_the_edges() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "arriba"), (Par, 0, "abajo")], 1, 2)?;
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(0));
        assert_eq!(caret_char(&app), Some(6), "end of the block above");
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(1));
        assert_eq!(caret_char(&app), Some(0));
        assert_eq!(app.blocks.len(), 2, "navigation changes nothing");
        assert!(app.save.can_close());
        Ok(())
    }

    #[test]
    fn keys_are_left_alone_during_ime_composition() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
        let preedit = |text: &str| {
            egui::Event::Ime(egui::ImeEvent::Preedit {
                text: text.into(),
                active_range_chars: None,
            })
        };
        app_frame(&mut app, &ctx, vec![preedit("に")]);
        assert!(app.ime_composing);
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(
            app.blocks.len(),
            1,
            "Enter confirms the composition, no split"
        );
        // Composition ends (empty preedit): Enter splits again.
        app_frame(&mut app, &ctx, vec![preedit("")]);
        assert!(!app.ime_composing);
        Ok(())
    }

    /// egui selects the preedit text, so the selection rule alone already
    /// blocks Enter above. This isolates the IME guard itself: a composition
    /// in progress with no selection must still not be intercepted.
    #[test]
    fn ime_guard_blocks_editor_keys_even_without_selection() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab"), (Par, 0, "cd")], 1, 0)?;
        assert!(app.caret.as_ref().is_some_and(|c| !c.selection));
        app.ime_composing = true;
        // Keys reach the text field as usual; only block structure must not change.
        let ids = |app: &SilexApp| app.blocks.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
        let before = ids(&app);
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(ids(&app), before, "no split while composing");
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        assert_eq!(ids(&app), before, "no merge while composing");
        let commit = egui::Event::Ime(egui::ImeEvent::Commit("に".into()));
        app_frame(&mut app, &ctx, vec![commit]);
        assert!(!app.ime_composing, "a commit ends the composition");
        Ok(())
    }

    /// Like `app_frame`, but returns the platform commands (e.g. copy).
    fn app_frame_commands(
        app: &mut SilexApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<egui::OutputCommand> {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
        output.textures_delta.clear();
        output.platform_output.commands
    }

    fn type_into(app: &mut SilexApp, ctx: &egui::Context, text: &str) {
        for c in text.chars() {
            app_frame(app, ctx, vec![egui::Event::Text(c.to_string())]);
        }
    }

    #[test]
    fn slash_command_changes_the_block_type() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
        type_into(&mut app, &ctx, "/h2");
        let menu = app.slash.clone().expect("menu open");
        assert_eq!(menu.matches, [BlockKind::Heading2]);
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(contents(&app), [(BlockKind::Heading2, 0, String::new())]);
        assert!(app.slash.is_none());
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(0), "keeps editing");
        Ok(())
    }

    #[test]
    fn slash_menu_arrows_pick_and_escape_closes() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
        type_into(&mut app, &ctx, "/titulo");
        assert_eq!(app.slash.as_ref().map(|m| m.matches.len()), Some(3));
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
        assert_eq!(app.slash.as_ref().map(|m| m.selected), Some(1));
        assert_eq!(app.blocks.len(), 1, "arrows stay in the menu");
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(app.blocks[0].kind, BlockKind::Heading2);

        // Escape closes the menu and leaves the text (no block selection).
        let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
        type_into(&mut app, &ctx, "/h1");
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert!(app.slash.is_none());
        assert!(app.selection.is_none());
        assert_eq!(app.blocks[0].content, "/h1");
        assert_eq!(app.blocks[0].kind, Par);
        // Coming back to the block does not reopen it until the text changes.
        app.pending_focus = Some((app.blocks[0].id.clone(), 3));
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(0));
        assert!(app.slash.is_none(), "stays dismissed");
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.blocks[0].content, "/h");
        assert!(app.slash.is_some(), "text changed: menu is back");
        Ok(())
    }

    #[test]
    fn enter_without_slash_matches_splits_as_usual() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
        type_into(&mut app, &ctx, "/zzz");
        assert!(app.slash.is_none());
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(app.blocks.len(), 2);
        Ok(())
    }

    #[test]
    fn block_selection_extend_copy_delete_and_undo() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(
            &[
                (Par, 0, "uno"),
                (Bul, 0, "dos"),
                (Bul, 1, "tres"),
                (Par, 0, "cuatro"),
            ],
            1,
            0,
        )?;
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.selection_range(), Some((1, 1)));
        assert_eq!(focused_block(&app, &ctx), None, "text field left");

        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
        assert_eq!(app.selection_range(), Some((1, 2)));
        let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Copy]);
        assert!(
            commands.contains(&egui::OutputCommand::CopyText("• dos\n  • tres".into())),
            "{commands:?}"
        );

        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
        let texts: Vec<String> = app.blocks.iter().map(|b| b.content.clone()).collect();
        assert_eq!(texts, ["uno", "cuatro"]);
        assert_eq!(app.selection_range(), Some((1, 1)), "selection moves on");
        assert!(!app.save.can_close());

        app.undo();
        let texts: Vec<String> = app.blocks.iter().map(|b| b.content.clone()).collect();
        assert_eq!(texts, ["uno", "dos", "tres", "cuatro"]);
        Ok(())
    }

    #[test]
    fn selection_moves_with_arrows_and_enter_goes_back_to_editing() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "bb"), (Par, 0, "c")], 0, 0)?;
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
        assert_eq!(app.selection_range(), Some((1, 1)));
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert!(app.selection.is_none());
        assert_eq!(focused_block(&app, &ctx), Some(1));
        assert_eq!(caret_char(&app), Some(2), "cursor at the end");
        Ok(())
    }

    #[test]
    fn cut_copies_and_deletes_and_page_switch_drops_undo() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b")], 0, 0)?;
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Cut]);
        assert!(commands.contains(&egui::OutputCommand::CopyText("a".into())));
        assert_eq!(app.blocks.len(), 1);
        assert!(app.history.can_undo());
        let other = app.db.create_page("Otra", None)?;
        app.reload_pages()?;
        app.open_page(other)?;
        assert!(!app.history.can_undo() && app.selection.is_none());
        Ok(())
    }

    fn texts(app: &SilexApp) -> Vec<String> {
        app.blocks.iter().map(|b| b.content.clone()).collect()
    }

    #[test]
    fn undo_and_redo_walk_typing_and_structure() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
        type_into(&mut app, &ctx, "cd"); // quick typing: one undo step
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(texts(&app), ["abcd", ""]);

        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        assert_eq!(texts(&app), ["abcd"], "undo the split");
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        assert_eq!(texts(&app), ["ab"], "undo the typing as one step");
        assert!(!app.history.can_undo());

        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND | M::SHIFT)]);
        assert_eq!(texts(&app), ["abcd"]);
        app_frame(&mut app, &ctx, vec![key(Key::Y, M::COMMAND)]);
        assert_eq!(texts(&app), ["abcd", ""], "Cmd+Y also redoes");

        // Undo results are saved like any edit.
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        app.save_now()?;
        let page = app.selected.clone().expect("page");
        assert_eq!(app.db.list_blocks(&page)?.len(), 1);
        Ok(())
    }

    #[test]
    fn undo_focuses_the_changed_block() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "uno"), (Par, 0, "dos")], 1, 3)?;
        type_into(&mut app, &ctx, "!");
        app.pending_focus = Some((app.blocks[0].id.clone(), 0));
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(texts(&app), ["uno", "dos"]);
        assert_eq!(
            focused_block(&app, &ctx),
            Some(1),
            "cursor goes to the undone block"
        );
        Ok(())
    }

    #[test]
    fn cmd_z_in_the_search_field_leaves_the_document_alone() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
        type_into(&mut app, &ctx, "c");
        assert!(app.history.can_undo());
        app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        assert_eq!(texts(&app), ["abc"]);
        assert!(app.history.can_undo(), "document history untouched");
        Ok(())
    }

    #[test]
    fn alt_shift_arrows_move_the_block_and_cmd_d_duplicates() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b"), (Par, 0, "c")], 0, 1)?;
        let page = app.selected.clone();
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::ALT | M::SHIFT)]);
        assert_eq!(texts(&app), ["b", "a", "c"]);
        assert_eq!(app.selected, page, "not the Alt+↓ page navigation");
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(
            focused_block(&app, &ctx),
            Some(1),
            "focus follows the block"
        );

        app_frame(&mut app, &ctx, vec![key(Key::D, M::COMMAND)]);
        assert_eq!(texts(&app), ["b", "a", "a", "c"]);
        assert_ne!(app.blocks[1].id, app.blocks[2].id);
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        assert_eq!(texts(&app), ["a", "b", "c"]);
        Ok(())
    }

    #[test]
    fn selection_moves_and_duplicates_as_a_group() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(
            &[(Par, 0, "a"), (Par, 0, "b"), (Par, 0, "c"), (Par, 0, "d")],
            2,
            0,
        )?;
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT | M::SHIFT)]);
        assert_eq!(texts(&app), ["a", "c", "d", "b"]);
        assert_eq!(app.selection_range(), Some((1, 2)), "selection follows");
        app_frame(&mut app, &ctx, vec![key(Key::D, M::COMMAND)]);
        assert_eq!(texts(&app), ["a", "c", "d", "c", "d", "b"]);
        assert_eq!(
            app.selection_range(),
            Some((3, 4)),
            "the copies are selected"
        );
        Ok(())
    }

    #[test]
    fn block_menu_actions_and_add_buttons_are_undoable() -> anyhow::Result<()> {
        let (mut app, _ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b")], 0, 0)?;
        let first = app.blocks[0].id.clone();
        app.block_action(BlockAction::MoveDown, &first);
        assert_eq!(texts(&app), ["b", "a"]);
        app.block_action(BlockAction::Duplicate, &first);
        assert_eq!(texts(&app), ["b", "a", "a"]);
        app.block_action(BlockAction::Delete, &first);
        assert_eq!(texts(&app), ["b", "a"]);
        for _ in 0..3 {
            app.undo();
        }
        assert_eq!(texts(&app), ["a", "b"]);
        Ok(())
    }

    fn paste(text: &str) -> egui::Event {
        egui::Event::Paste(text.into())
    }

    #[test]
    fn multi_line_paste_becomes_blocks_and_undoes_in_one_step() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "Hola mundo")], 0, 5)?;
        app_frame(&mut app, &ctx, vec![paste("uno\r\ndos\ntres ")]);
        assert_eq!(texts(&app), ["Hola uno", "dos", "tres mundo"]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(2));
        assert_eq!(
            caret_char(&app),
            Some(5),
            "cursor at the end of the pasted text"
        );
        app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
        assert_eq!(texts(&app), ["Hola mundo"]);
        Ok(())
    }

    #[test]
    fn single_line_paste_and_code_blocks_are_left_to_the_text_field() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 1)?;
        app_frame(&mut app, &ctx, vec![paste("XY")]);
        assert_eq!(texts(&app), ["aXYb"]);

        let (mut app, ctx) = editor_app(&[(Cod, 0, "")], 0, 0)?;
        app_frame(&mut app, &ctx, vec![paste("fn a() {\n}\n")]);
        assert_eq!(app.blocks.len(), 1);
        assert_eq!(app.blocks[0].content, "fn a() {\n}\n");
        Ok(())
    }

    #[test]
    fn copied_blocks_paste_back_with_their_structure() -> anyhow::Result<()> {
        let (mut app, ctx) =
            editor_app(&[(Bul, 0, "Pan"), (Bul, 1, "Integral"), (Par, 0, "")], 0, 0)?;
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
        let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Copy]);
        let Some(egui::OutputCommand::CopyText(copied)) = commands.into_iter().next() else {
            anyhow::bail!("nothing copied");
        };
        // Back to editing the empty last block, then paste.
        app.selection = None;
        app.pending_focus = Some((app.blocks[2].id.clone(), 0));
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![paste(&copied)]);
        assert_eq!(
            contents(&app)[2..],
            [(Bul, 0, "Pan".into()), (Bul, 1, "Integral".into())]
        );
        Ok(())
    }

    #[test]
    fn ime_composition_commits_text_without_splitting() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "Texto: ")], 0, 7)?;
        let preedit = |t: &str| {
            egui::Event::Ime(egui::ImeEvent::Preedit {
                text: t.into(),
                active_range_chars: None,
            })
        };
        for step in ["n", "に", "にほ", "にほn"] {
            app_frame(&mut app, &ctx, vec![preedit(step)]);
        }
        // The user confirms with Enter: the IME turns it into a commit.
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        app_frame(
            &mut app,
            &ctx,
            vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".into()))],
        );
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.blocks.len(), 1, "no split during the composition");
        assert_eq!(
            app.blocks[0].content, "Texto: 日本",
            "no stray newline, no preedit left"
        );
        assert!(!app.ime_composing);
        // After the commit, Enter splits again as usual.
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(app.blocks.len(), 2);
        Ok(())
    }

    #[test]
    fn a_stale_ime_flag_is_cleared_when_leaving_the_block() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
        app.ime_composing = true; // e.g. the IME never sent its end event
        app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert!(!app.ime_composing, "no block focused: flag reset");
        app.selection = None;
        app.pending_focus = Some((app.blocks[0].id.clone(), 2));
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        assert_eq!(app.blocks.len(), 2, "Enter works again");
        Ok(())
    }

    #[test]
    fn arrow_up_moves_inside_a_multi_line_block_first() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "arriba"), (Par, 0, "uno\ndos")], 1, 6)?;
        assert!(
            app.caret.as_ref().is_some_and(|c| !c.first_row),
            "on the second line"
        );
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(
            focused_block(&app, &ctx),
            Some(1),
            "still in the same block"
        );
        assert!(app.caret.as_ref().is_some_and(|c| c.first_row));
        app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
        app_frame(&mut app, &ctx, vec![]);
        assert_eq!(focused_block(&app, &ctx), Some(0), "now it jumps");
        Ok(())
    }

    #[test]
    fn tricky_unicode_typed_in_the_app_is_saved_exactly() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
        let text = "e\u{301} 👨‍👩‍👧 🇪🇸 مرحبا שלום 日本語";
        app_frame(&mut app, &ctx, vec![egui::Event::Text(text.into())]);
        app.save_now()?;
        let page = app.selected.clone().expect("page");
        assert_eq!(app.db.list_blocks(&page)?[0].content, text);
        assert_eq!(app.db.search_pages("日本語")?.len(), 1);
        Ok(())
    }

    #[test]
    fn exporting_writes_what_is_on_screen_and_offers_the_folder() -> anyhow::Result<()> {
        let (mut app, ctx) = editor_app(&[(Bul, 0, "uno"), (Bul, 1, "dos")], 0, 3)?;
        type_into(&mut app, &ctx, "!"); // unsaved edit
        let dir = std::env::temp_dir().join(format!("silex-app-export-{}", uuid::Uuid::new_v4()));
        app.export_current_page_to(&dir)?;
        let path = app.last_export.clone().expect("export offered");
        let text = std::fs::read_to_string(&path)?;
        assert!(text.contains("- uno\\!\n  - dos\n"), "{text}"); // "!" is escaped
        assert!(app.save.can_close(), "saved before exporting");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A file dropped on the window (test double for egui's handle).
    #[derive(Debug)]
    struct TestDrop(std::path::PathBuf);

    impl egui::DroppedFile for TestDrop {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }

    fn drop_frame(app: &mut SilexApp, ctx: &egui::Context, files: &[std::path::PathBuf]) {
        let input = egui::RawInput {
            dropped_files: files
                .iter()
                .map(|f| std::sync::Arc::new(TestDrop(f.clone())) as egui::DroppedFileHandle)
                .collect(),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
        output.textures_delta.clear();
    }

    #[test]
    fn dropping_markdown_files_imports_them_as_pages() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("silex-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let good = dir.join("receta.md");
        std::fs::write(
            &good,
            "# Pan casero\n\n- harina\n  - integral\n- [x] amasar\n",
        )?;
        let latin1 = dir.join("vieja.md");
        std::fs::write(&latin1, b"caf\xe9")?;
        let other = dir.join("foto.png");
        std::fs::write(&other, b"png")?;

        let mut app = SilexApp::new(Database::open_in_memory()?)?;
        let ctx = egui::Context::default();
        drop_frame(&mut app, &ctx, &[good, latin1, other.clone()]);
        let start = std::time::Instant::now();
        while app.import_job.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_frame(&mut app, &ctx, vec![]);
        }
        assert!(app.import_job.is_none(), "import finished");
        assert_eq!(app.title, "Pan casero", "the imported page is open");
        assert_eq!(
            contents(&app),
            [
                (Bul, 0, "harina".into()),
                (Bul, 1, "integral".into()),
                (BlockKind::Todo, 0, "amasar".into())
            ]
        );
        let notice = app.notice.clone().unwrap_or_default();
        assert!(notice.contains("Importada(s) 1"), "{notice}");
        assert!(
            notice.contains("vieja.md") && notice.contains("UTF-8"),
            "{notice}"
        );
        assert!(notice.contains("1 ignorado"), "{notice}");

        // Only non-Markdown files: nothing to import, just the message.
        drop_frame(&mut app, &ctx, &[other]);
        assert!(app.import_job.is_none());
        let notice = app.notice.clone().unwrap_or_default();
        assert!(notice.contains("1 ignorado"), "{notice}");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn workspace_export_runs_in_the_background_and_saves_first() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("silex-app-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        let mut app = SilexApp::new(Database::open(dir.join("silex.sqlite3"))?)?;
        app.title = "Título sin guardar".into();
        app.mark_dirty();
        let ctx = egui::Context::default();
        app.start_workspace_export(&ctx, dir.join("exportaciones"))?;
        assert!(app.save.can_close(), "saved before exporting");
        let start = std::time::Instant::now();
        while app.export_job.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_frame(&mut app, &ctx, vec![]);
        }
        let folder = app.last_export.clone().expect("export finished");
        assert!(folder.join("Título sin guardar.md").exists());
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.contains("1 página"))
        );

        // In-memory databases cannot be exported from another thread.
        let mut memory = SilexApp::new(Database::open_in_memory()?)?;
        assert!(memory.start_workspace_export(&ctx, dir.join("x")).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn field_ids_follow_block_and_generation() {
        assert_eq!(field_id(0, "block", "a"), field_id(0, "block", "a"));
        assert_ne!(field_id(0, "block", "a"), field_id(0, "block", "b"));
        assert_ne!(field_id(0, "block", "a"), field_id(1, "block", "a"));
        assert_ne!(field_id(0, "block", "a"), field_id(0, "title", "a"));
    }

    #[test]
    fn undo_stays_with_its_block_when_blocks_move() {
        let ctx = egui::Context::default();
        let a = field_id(0, "block", "a");
        let mut blocks = vec![block("a"), block("b")];

        frame(&ctx, 0.0, vec![], &mut blocks, Some(a));
        frame(&ctx, 0.1, vec![], &mut blocks, None);
        assert!(frame(&ctx, 0.2, type_text("hola"), &mut blocks, None));
        // Let egui's undoer record the new state.
        frame(&ctx, 2.0, vec![], &mut blocks, None);
        frame(&ctx, 4.0, vec![], &mut blocks, None);
        assert_eq!(blocks[0].content, "hola");

        // A new block appears before "a": positions shift by one.
        blocks.insert(0, block("nuevo"));
        frame(&ctx, 4.1, vec![], &mut blocks, Some(a));
        frame(&ctx, 4.2, vec![], &mut blocks, None);
        let changed = frame(&ctx, 4.3, undo(), &mut blocks, None);

        assert!(changed, "undo must mark the document dirty");
        assert_eq!(blocks[1].id, "a");
        assert_eq!(blocks[1].content, "", "undo reverts block a");
        assert_eq!(blocks[0].content, "", "the new block is untouched");
        assert_eq!(blocks[2].content, "", "block b is untouched");
    }
}
