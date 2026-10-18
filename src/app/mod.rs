//! The desktop app (egui). Split by area; all modules extend `SilexApp`.
use std::{collections::HashSet, path::PathBuf, time::Instant};

use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    db::{Database, Shift, backup::BackupJob},
    history::{History, Snapshot},
    model::{Block, Page, TreeRow, ancestors, build_tree},
    save_state::SaveState,
};

mod blocks;
mod dialogs;
mod document;
pub mod icons;
mod jobs;
mod sidebar;
#[cfg(test)]
mod snapshots;
#[cfg(test)]
mod tests;
pub mod theme;

use self::{blocks::*, dialogs::*, sidebar::*};

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

    fn restore_from_trash(&mut self, id: &str) -> Result<()> {
        self.save_now()?;
        self.db.restore_page(id)?;
        self.reload_pages()?;
        self.open_page(id.to_owned())
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

    fn create_root_page(&mut self, title: &str) -> Result<()> {
        self.save_now()?;
        let id = self.db.create_page(title, None)?;
        self.reload_pages()?;
        self.open_page(id)
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

    fn report<T>(&mut self, result: Result<T>) {
        if let Err(err) = result {
            self.notice = Some(format!("{err:#}"));
        }
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

    /// One frame: shortcuts, layout, dialogs, background work and autosave.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // The root Ui has no background of its own: paint the theme canvas
        // over the whole window (otherwise the window clear color shows).
        ui.painter()
            .rect_filled(ctx.content_rect(), 0.0, theme::current(&ctx).bg);
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
