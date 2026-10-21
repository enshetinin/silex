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
mod board;
mod cells;
mod database;
mod date_picker;
mod dialogs;
mod document;
mod files;
mod frame;
pub mod icons;
mod jobs;
mod links;
mod page_history;
mod palette;
mod privacy;
mod read_only;
mod row_page;
mod search;
mod side_panel;
mod sidebar;
#[cfg(test)]
mod snapshots;
mod space;
mod startup;
mod tab_bar;
mod table_undo;
mod templates;
#[cfg(test)]
mod tests;
pub mod theme;
mod view_menus;
mod view_tabs;

pub use self::startup::StartupError;
use self::{
    blocks::*, cells::*, database::*, date_picker::*, dialogs::*, files::*, links::*,
    page_history::*, privacy::*, read_only::*, row_page::*, search::*, side_panel::*, sidebar::*,
    space::*, tab_bar::*, table_undo::*, templates::*, view_menus::*, view_tabs::*,
};

/// Window id on Linux (Wayland app id, X11 class). Matches the desktop
/// entry the Linux packages install (`silex-desktop.desktop`, ADR 0005).
pub const APP_ID: &str = "silex-desktop";

/// Smallest window at 100 % (logical points): the sidebar at its narrowest
/// (220) and a page at its narrowest (`MIN_PAGE_WIDTH`, 480) with its
/// margins. Scaled with the interface size, so a zoomed page still fits.
pub const MIN_WINDOW: egui::Vec2 = egui::vec2(760.0, 480.0);

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
    /// The view last open in each database (database id → view id), so it
    /// opens again on that view; for this session only.
    last_views: std::collections::HashMap<String, String>,
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
    /// `SILEX_STARTUP_TIMING` (H9.3b): when the process started, to print
    /// how long the window took to show the app; frames drawn since.
    startup: Option<(Instant, u8)>,
    /// `SILEX_STARTUP_TIMING=exit`: close once it is printed.
    startup_exit: bool,
    /// Block under the pointer last frame (shows its handle).
    hovered_block: Option<String>,
    /// Theme and interface size (stored in the database).
    settings: Settings,
    save: SaveState,
    /// Errors from actions other than saving (search, create, open).
    notice: Option<String>,
    /// Notices still to come from one drop (import and attach): the second
    /// is joined to the first instead of hiding it.
    notice_group: u8,
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
    /// Whole-workspace export running on a worker thread.
    export_job: Option<silex_desktop::export::WorkspaceExportJob>,
    /// A page with attachments being exported on a worker thread (H7.9),
    /// and its title for the notice.
    page_export: Option<(silex_desktop::export::PageExportJob, String)>,
    /// Attachments of the open page and the work on files (ADR 0007).
    files: Files,
    /// «Espacio» and «Liberar espacio» (H7.4).
    space: Space,
    /// Links of the open page and the page picker (H7.5).
    links: Links,
    /// «Historial de versiones» of the open page, while open (H7.8).
    page_history: Option<PageHistory>,
    /// Live templates (ADR 0010), the «Plantillas» section and its picker.
    templates: Templates,
    /// Open pages (ADR 0011); the active one is `selected`.
    tabs: silex_desktop::tabs::Tabs,
    /// What a tab keeps while it is not shown: its undo and scroll.
    parked: std::collections::HashMap<String, Parked>,
    /// The tabs as last stored, to write them only when they change.
    saved_tabs: String,
    /// Scroll of the open page's blocks as of the last frame.
    editor_scroll: f32,
    /// Scroll to restore on the next frame (a tab shown again).
    pending_scroll: Option<f32>,
    /// Modifiers held during this frame's clicks in the page: Cmd/Ctrl
    /// opens a link in a new tab, Alt beside (ADR 0011).
    click_modifiers: egui::Modifiers,
    /// The page shown read-only beside the open one (H8.2).
    side: Option<SidePanel>,
    /// Width left of the window after the sidebar, as of the last frame.
    main_width: f32,
    /// Interface size the window's minimum size was last set for.
    min_window_zoom: u16,
    /// The database is in a synced folder: warned at start (ADR 0012).
    synced_warning: Option<SyncedWarning>,
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
    /// Prints, on the second frame (the first one is on screen by then),
    /// how long it took since `started` (H9.3b). `exit` closes the app then.
    pub fn time_startup(&mut self, started: Instant, exit: bool) {
        self.startup = Some((started, 0));
        self.startup_exit = exit;
    }

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
            last_views: std::collections::HashMap::new(),
            shortcuts_dialog: false,
            rename: None,
            selected: None,
            title: String::new(),
            blocks: Vec::new(),
            focus_title: false,
            theme_installed: false,
            startup: None,
            startup_exit: false,
            hovered_block: None,
            settings: Settings::default(),
            save: SaveState::default(),
            notice: None,
            notice_group: 0,
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
            export_job: None,
            page_export: None,
            files: Files::default(),
            space: Space::default(),
            links: Links::default(),
            page_history: None,
            templates: Templates::default(),
            tabs: silex_desktop::tabs::Tabs::default(),
            parked: std::collections::HashMap::new(),
            saved_tabs: String::new(),
            editor_scroll: 0.0,
            pending_scroll: None,
            click_modifiers: egui::Modifiers::NONE,
            side: None,
            main_width: 0.0,
            min_window_zoom: 100,
            synced_warning: None,
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
        app.templates.list = app.db.list_templates()?;
        app.rebuild_tree();
        // The tabs of last time (ADR 0011), those still live; else the
        // first page. A damaged value only loses the tabs.
        if let Some(json) = app.db.open_tabs().ok().flatten() {
            app.saved_tabs = json.clone();
            app.tabs = silex_desktop::tabs::Tabs::from_json(&json);
            app.close_tabs_not_live();
        }
        if !app.tabs.is_empty() {
            app.show_active_tab(true)?;
        } else if let Some(row) = app.tree.first() {
            app.open_page(row.page.id.clone())?;
        }
        // Preferences are not worth refusing to start over: report and use defaults.
        match app.db.load_settings() {
            Ok(settings) => app.settings = settings,
            Err(err) => app.notice = Some(format!("No se pudieron leer los ajustes: {err:#}")),
        }
        app.protect_local_data();
        Ok(app)
    }

    /// Opens a page in the active tab (a click), or shows the tab that
    /// already has it (ADR 0011). A page that is not live is ignored.
    fn open_page(&mut self, id: String) -> Result<()> {
        if self.db.get_page(&id)?.is_none() {
            return Ok(());
        }
        if let Some(left) = self.tabs.open_here(&id) {
            self.parked.remove(&left);
        }
        self.show_active_tab(true)
    }

    /// Shows the active tab's page. The page that was open is saved, and
    /// its undo and scroll are kept if it is still in a tab. `reload`
    /// reads the page again even if it is already the one shown.
    fn show_active_tab(&mut self, reload: bool) -> Result<()> {
        let Some(id) = self.tabs.active_id().map(str::to_owned) else {
            return Ok(());
        };
        self.save_tabs();
        let same = self.selected.as_deref() == Some(id.as_str());
        if same && !reload {
            return Ok(());
        }
        self.save_now()?;
        if let Some(old) = self.selected.clone()
            && !same
            && self.tabs.position(&old).is_some()
        {
            let history = std::mem::take(&mut self.history);
            self.parked.insert(
                old,
                Parked {
                    history,
                    scroll: self.editor_scroll,
                },
            );
        }
        if !self.load_page(id.clone())? {
            // Gone meanwhile: its tab closes, and the next one is shown.
            self.tabs.retain(|t| t != id);
            self.parked.remove(&id);
            self.save_tabs();
            return if self.tabs.is_empty() {
                Ok(())
            } else {
                self.show_active_tab(true)
            };
        }
        if !same {
            let parked = self.parked.remove(&id);
            self.pending_scroll = Some(parked.as_ref().map_or(0.0, |p| p.scroll));
            if let Some(parked) = parked {
                self.history = parked.history;
            }
        }
        Ok(())
    }

    /// Reads a page into the editor; false if it is not live.
    fn load_page(&mut self, id: String) -> Result<bool> {
        let page = self.db.get_page(&id)?;
        let found = page.is_some();
        if let Some(page) = page {
            self.slash = None;
            self.slash_dismissed = None;
            self.selection = None;
            self.blocks = self.db.list_blocks(&id)?;
            self.load_attachments(&id)?;
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
            self.links.picker = None;
            self.load_links()?;
            self.rebuild_tree();
            self.refresh_side();
        }
        Ok(found)
    }

    /// Stores the tabs if they changed (to reopen them next time). A
    /// failure is only a notice: the tabs still work.
    fn save_tabs(&mut self) {
        let json = self.tabs.to_json();
        if json != self.saved_tabs {
            let result = self.db.save_open_tabs(&json);
            if result.is_ok() {
                self.saved_tabs = json;
            }
            self.report(result);
        }
    }

    /// Closes the tabs of pages no longer live (sent to the trash, or
    /// gone after restoring a backup).
    fn close_tabs_not_live(&mut self) {
        let live: HashSet<&str> = self
            .pages
            .iter()
            .chain(&self.templates.list)
            .map(|p| p.id.as_str())
            .collect();
        let closed = self.tabs.retain(|id| live.contains(id));
        for id in closed {
            self.parked.remove(&id);
        }
    }

    fn reload_pages(&mut self) -> Result<()> {
        self.pages = self.db.list_pages()?;
        self.templates.list = self.db.list_templates()?;
        self.close_tabs_not_live();
        self.save_tabs();
        self.refresh_side();
        // A linked page may have gone to the trash, or come back.
        self.load_links()?;
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

    /// Shows the active tab, else the first page in the tree, or the
    /// empty state.
    fn open_first_page(&mut self) -> Result<()> {
        if !self.tabs.is_empty() {
            return self.show_active_tab(true);
        }
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
            .find_page(id)
            .map(|p| display_title(&p.title).to_owned())
            .unwrap_or_default();
        let trashed = self.db.trash_page(id)?;
        self.reload_pages()?;
        // Only leave the open page if it went to the trash with this branch.
        let open_still_live = self
            .selected
            .as_ref()
            .is_some_and(|sel| self.find_page(sel).is_some());
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

    /// A live page or template (ADR 0010) by id.
    fn find_page(&self, id: &str) -> Option<&Page> {
        self.pages
            .iter()
            .chain(&self.templates.list)
            .find(|p| p.id == id)
    }

    /// Whether the open page is a template.
    fn template_open(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|id| self.templates.list.iter().any(|t| &t.id == id))
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
        // Tabs (ADR 0011), not while a dialog has the keyboard. Cmd+T is
        // a new tab: the search window, to choose its page.
        let dialog = self.links.picker.is_some()
            || self.page_history.is_some()
            || self.templates.picker.is_some();
        if !dialog && pressed(Sc::new(M::COMMAND, Key::T)) {
            self.open_search(ctx);
            if let Some(window) = &mut self.search {
                window.new_tab = true;
            }
            return;
        }
        let close_tab = !dialog && pressed(Sc::new(M::COMMAND, Key::W));
        let previous_tab = !dialog && pressed(Sc::new(M::CTRL | M::SHIFT, Key::Tab));
        let next_tab = !dialog && !previous_tab && pressed(Sc::new(M::CTRL, Key::Tab));
        const NUMBERS: [Key; 9] = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        let numbered = if dialog {
            None
        } else {
            NUMBERS
                .iter()
                .position(|key| pressed(Sc::new(M::COMMAND, *key)))
        };
        let new_subpage = pressed(Sc::new(M::COMMAND | M::SHIFT, Key::N));
        let new_page = !new_subpage && pressed(Sc::new(M::COMMAND, Key::N));
        // Exact Alt only: Alt+Shift+↑/↓ moves blocks instead.
        let previous = take_exact_key(ctx, Key::ArrowUp, M::ALT);
        let next = take_exact_key(ctx, Key::ArrowDown, M::ALT);
        // In a database's table (not typing in a field), Cmd+Z undoes the
        // table's changes (H6.4); elsewhere, the document's.
        let table = self.table_has_focus(ctx);
        let document = table || self.document_has_focus(ctx);
        let redo = document
            && (pressed(Sc::new(M::COMMAND | M::SHIFT, Key::Z))
                || pressed(Sc::new(M::COMMAND, Key::Y)));
        let undo = document && !redo && pressed(Sc::new(M::COMMAND, Key::Z));
        let rename = pressed(Sc::new(M::NONE, Key::F2));

        let result = (|| -> Result<()> {
            if close_tab {
                self.close_tab(self.tabs.active())?;
            }
            if previous_tab || next_tab {
                self.cycle_tab(next_tab)?;
            }
            // Cmd+9 is the last tab, as in a browser.
            if let Some(number) = numbered {
                let index = if number == 8 {
                    self.tabs.len().saturating_sub(1)
                } else {
                    number
                };
                self.select_tab(index)?;
            }
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
            match (undo, redo, table) {
                (true, _, true) => self.undo_table(),
                (true, _, false) => self.undo(),
                (_, true, true) => self.redo_table(),
                (_, true, false) => self.redo(),
                _ => {}
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
        // The window's minimum size follows the interface size.
        if zoom != self.min_window_zoom {
            self.min_window_zoom = zoom;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
                MIN_WINDOW * f32::from(zoom) / 100.0,
            ));
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
        egui::Panel::top("silex_top_bar")
            .exact_size(frame::TOP_BAR)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.bg))
            .show(ui, |ui| self.window_bar(ui));
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
        self.main_width = ui.available_width();
        self.side_panel(ui);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 14)),
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
        if self.search.is_none()
            && self.links.picker.is_none()
            && self.page_history.is_none()
            && self.templates.picker.is_none()
        {
            self.take_pasted_image(&ctx);
            self.take_editor_key(&ctx);
            self.apply_pending_focus(&ctx);
            self.handle_selection_keys(&ctx);
        }
        self.layout(ui);
        self.run_paste(&ctx);
        self.run_editor_key(&ctx);
        self.refresh_slash_and_selection();
        self.take_double_bracket();
        // Save errors are shown by the status bar, so results are not reported twice.
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            let _ = self.save_now();
        }
        self.tick_backups(&ctx);
        self.tick_import();
        self.tick_files(&ctx);
        self.tick_free_space();
        self.tick_workspace_export();
        self.tick_page_export();
        Self::drop_hint(&ctx);
        let now = Instant::now();
        if self.save.is_due(now) {
            let _ = self.save_now();
        }
        self.handle_close_request(&ctx);
        if let Some((started, frames)) = &mut self.startup {
            *frames += 1;
            if *frames == 2 {
                eprintln!("Silex: ventana lista en {:?}", started.elapsed());
                self.startup = None;
                if self.startup_exit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        self.search_window(&ctx);
        self.shortcuts_dialog(&ctx);
        self.close_dialog(&ctx);
        self.restore_dialog(&ctx);
        self.trash_dialog(&ctx);
        self.free_space_dialog(&ctx);
        self.link_picker(&ctx);
        self.link_restore_dialog(&ctx);
        self.template_picker(&ctx);
        self.synced_warning_dialog(&ctx);
        self.page_history_window(&ctx);
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
        self.clear_opened_copies();
    }
}

/// Safety net in case `on_exit` did not run; a second save is a no-op.
impl Drop for SilexApp {
    fn drop(&mut self) {
        if let Err(err) = self.save_now() {
            eprintln!("Silex: no se guardaron cambios al cerrar: {err:#}");
        }
        self.clear_opened_copies();
    }
}
