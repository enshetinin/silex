//! The desktop app (egui). Split by area; all modules extend `SilexApp`.
use std::{collections::HashSet, path::PathBuf, time::Instant};

use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    db::{Database, Shift, backup::BackupJob},
    history::{History, Snapshot},
    model::{Block, Page, PageKind, TreeRow, ancestors, build_tree, without_database_rows},
    save_state::SaveState,
    settings::{Settings, ThemeMode},
};

mod blocks;
mod cells;
mod database;
mod dialogs;
mod document;
pub mod icons;
mod jobs;
mod palette;
mod row_page;
mod search;
mod sidebar;
#[cfg(test)]
mod snapshots;
mod startup;
#[cfg(test)]
mod tests;
pub mod theme;
mod view_menus;

pub use self::startup::StartupError;
use self::{
    blocks::*, cells::*, database::*, dialogs::*, row_page::*, search::*, sidebar::*, view_menus::*,
};

/// Window id on Linux (Wayland app id, X11 class). Matches the desktop
/// entry the Linux packages install (`silex-desktop.desktop`, ADR 0005).
pub const APP_ID: &str = "silex-desktop";

/// The app icon for the window, taskbar and (unbundled) macOS Dock.
/// `None` if the embedded image cannot be read: the app starts anyway.
pub fn window_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../../assets/app-icon/silex-512.png")).ok()
}

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
    /// The search window (Cmd+K), while open.
    search: Option<SearchWindow>,
    /// The open page's rows and views, when it is a database (ADR 0006).
    database: Option<DatabaseState>,
    /// The open page's properties, when it is a row of a database.
    row_page: Option<RowPage>,
    /// The keyboard shortcuts window (from the command palette).
    shortcuts_dialog: bool,
    rename: Option<Rename>,
    selected: Option<String>,
    title: String,
    blocks: Vec<Block>,
    /// Focus the page title on the next frame (a new page was just created).
    focus_title: bool,
    /// Fonts and styles installed (done on the first frame).
    theme_installed: bool,
    /// Block under the pointer last frame (shows its handle).
    hovered_block: Option<String>,
    /// Theme and interface size (stored in the database).
    settings: Settings,
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
            search: None,
            database: None,
            row_page: None,
            shortcuts_dialog: false,
            rename: None,
            selected: None,
            title: String::new(),
            blocks: Vec::new(),
            focus_title: false,
            theme_installed: false,
            hovered_block: None,
            settings: Settings::default(),
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
        // Preferences are not worth refusing to start over: report and use defaults.
        match app.db.load_settings() {
            Ok(settings) => app.settings = settings,
            Err(err) => app.notice = Some(format!("No se pudieron leer los ajustes: {err:#}")),
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
            self.database = None;
            if page.kind == PageKind::Database {
                self.load_database(&id)?;
            }
            self.load_row_page(&page)?;
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
        self.refresh_database()
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
                self.database = None;
                self.row_page = None;
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

    /// Sidebar rows: every page except the rows of databases.
    fn rebuild_tree(&mut self) {
        self.tree = build_tree(&without_database_rows(&self.pages), &self.expanded);
    }

    fn is_database(&self, id: &str) -> bool {
        self.pages
            .iter()
            .any(|p| p.id == id && p.kind == PageKind::Database)
    }

    fn toggle_expanded(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
        self.rebuild_tree();
    }

    /// A new page inside `parent`; inside a database, a new row.
    fn create_subpage(&mut self, parent: &str) -> Result<()> {
        if self.is_database(parent) {
            return self.add_row(parent);
        }
        self.save_now()?;
        let id = self.db.create_page("Sin título", Some(parent))?;
        self.expanded.insert(parent.to_owned());
        self.reload_pages()?;
        self.open_page(id)?;
        self.focus_title = true;
        Ok(())
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

    /// A new database (with its «Tabla» view), opened with its title ready.
    fn create_database(&mut self, parent: Option<&str>) -> Result<()> {
        self.save_now()?;
        let id = self.db.create_database("", parent)?;
        self.reload_pages()?;
        self.open_page(id)?;
        self.focus_title = true;
        Ok(())
    }

    fn create_root_page(&mut self, title: &str) -> Result<()> {
        self.save_now()?;
        let id = self.db.create_page(title, None)?;
        self.reload_pages()?;
        self.open_page(id)?;
        self.focus_title = true;
        Ok(())
    }

    /// Keyboard shortcuts, read before any widget so text fields do not see them.
    /// More specific shortcuts first: consume_shortcut ignores extra Shift/Alt.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as Sc, Modifiers as M};
        let pressed = |sc: Sc| ctx.input_mut(|i| i.consume_shortcut(&sc));
        // Cmd+K opens and closes the search window. While it is open no
        // other shortcut runs: the window has the keyboard.
        if pressed(Sc::new(M::COMMAND, Key::K)) {
            if self.search.take().is_none() {
                self.open_search(ctx);
            }
            return;
        }
        if self.search.is_some() {
            return;
        }
        let new_subpage = pressed(Sc::new(M::COMMAND | M::SHIFT, Key::N));
        let new_page = !new_subpage && pressed(Sc::new(M::COMMAND, Key::N));
        // Exact Alt only: Alt+Shift+↑/↓ moves blocks instead.
        let previous = take_exact_key(ctx, Key::ArrowUp, M::ALT);
        let next = take_exact_key(ctx, Key::ArrowDown, M::ALT);
        let document = self.document_has_focus(ctx);
        let redo = document
            && (pressed(Sc::new(M::COMMAND | M::SHIFT, Key::Z))
                || pressed(Sc::new(M::COMMAND, Key::Y)));
        let undo = document && !redo && pressed(Sc::new(M::COMMAND, Key::Z));
        let rename = pressed(Sc::new(M::NONE, Key::F2));

        let result = (|| -> Result<()> {
            if new_subpage && let Some(id) = self.selected.clone() {
                self.create_subpage(&id)?;
            }
            if new_page {
                self.create_root_page("Sin título")?;
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

    /// Applies the stored theme and size. At startup "Sistema" leaves egui's
    /// theme alone (it already follows the OS, and tests set their own).
    fn apply_settings(&self, ctx: &egui::Context, startup: bool) {
        let preference = match self.settings.theme {
            ThemeMode::System => egui::ThemePreference::System,
            ThemeMode::Light => egui::ThemePreference::Light,
            ThemeMode::Dark => egui::ThemePreference::Dark,
        };
        if self.settings.theme != ThemeMode::System || !startup {
            ctx.set_theme(preference);
        }
        let zoom = f32::from(self.settings.zoom_percent) / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(zoom);
        }
    }

    /// Changes settings, applies them and stores them.
    fn set_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        if settings == self.settings {
            return;
        }
        self.settings = settings;
        self.apply_settings(ctx, false);
        let result = self.db.save_settings(&self.settings);
        self.report(result);
    }

    /// Keyboard zoom (Cmd +/−/0, handled by egui) changes the size too:
    /// keep it within range and remember it.
    fn sync_zoom(&mut self, ctx: &egui::Context) {
        let factor = ctx.zoom_factor();
        let zoom = Settings::zoom_from_factor(factor);
        if (factor * 100.0 - f32::from(zoom)).abs() > 0.5 {
            ctx.set_zoom_factor(f32::from(zoom) / 100.0);
        }
        if zoom != self.settings.zoom_percent {
            let settings = Settings {
                zoom_percent: zoom,
                ..self.settings
            };
            self.set_settings(ctx, settings);
        }
    }

    fn report<T>(&mut self, result: Result<T>) {
        if let Err(err) = result {
            self.notice = Some(format!("{err:#}"));
        }
    }

    /// Sidebar and editor columns.
    fn layout(&mut self, ui: &mut egui::Ui) {
        let p = theme::current(ui.ctx());
        egui::Panel::left("silex_sidebar")
            .resizable(true)
            .default_size(260.0)
            .size_range(220.0..=420.0)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar_bg)
                    .inner_margin(egui::Margin::symmetric(10, 14)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| self.editor(ui));
    }

    /// One frame: shortcuts, layout, dialogs, background work and autosave.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !std::mem::replace(&mut self.theme_installed, true) {
            // Before anything is laid out, so even the first frame uses them.
            theme::install(&ctx);
            // This frame's root Ui copied the old style when it was created.
            ui.set_style(ctx.global_style());
            self.apply_settings(&ctx, true);
        }
        self.sync_zoom(&ctx);
        // The root Ui has no background of its own: paint the theme canvas
        // over the whole window (otherwise the window clear color shows).
        ui.painter()
            .rect_filled(ctx.content_rect(), 0.0, theme::current(&ctx).bg);
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);
        // The search window has the keyboard while it is open.
        if self.search.is_none() {
            self.take_editor_key(&ctx);
            self.apply_pending_focus(&ctx);
            self.handle_selection_keys(&ctx);
        }
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
        self.search_window(&ctx);
        self.shortcuts_dialog(&ctx);
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
