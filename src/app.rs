use std::{collections::HashSet, path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::{
        Database, Shift,
        backup::{self, BackupJob},
    },
    model::{Block, Page, TrashItem, TreeRow, ancestors, build_tree, descendants, filter_tree},
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
            self.blocks = self.db.list_blocks(&id)?;
            self.title = page.title;
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

    /// Keyboard shortcuts, read before any widget so text fields do not see them.
    /// More specific shortcuts first: consume_shortcut ignores extra Shift/Alt.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as Sc, Modifiers as M};
        let pressed = |sc: Sc| ctx.input_mut(|i| i.consume_shortcut(&sc));
        let new_subpage = pressed(Sc::new(M::COMMAND | M::SHIFT, Key::N));
        let new_page = !new_subpage && pressed(Sc::new(M::COMMAND, Key::N));
        let search = pressed(Sc::new(M::COMMAND, Key::K));
        let previous = pressed(Sc::new(M::ALT, Key::ArrowUp));
        let next = pressed(Sc::new(M::ALT, Key::ArrowDown));
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
        });
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

    fn mark_dirty(&mut self) {
        self.save.mark_dirty(Instant::now());
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
            self.status_bar(ui);
        });
        self.notice_bar(ui);
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
            self.mark_dirty();
        }
        ui.add_space(14.0);
        let mut modified = false;
        egui::ScrollArea::vertical()
            .id_salt("silex_editor_blocks")
            .show(ui, |ui| {
                for block in &mut self.blocks {
                    modified |= block_field(ui, self.doc_generation, block);
                    ui.add_space(8.0);
                }
            });
        if modified {
            self.mark_dirty();
        }
        ui.separator();
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("+ Texto", "paragraph"),
                ("+ Título", "heading"),
                ("+ Tarea", "todo"),
            ] {
                if ui.button(label).clicked() {
                    let result = (|| -> Result<()> {
                        self.save_now()?;
                        self.db.add_block(&page_id, kind)?;
                        self.blocks = self.db.list_blocks(&page_id)?;
                        Ok(())
                    })();
                    self.report(result);
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
        self.layout(ui);
        // Save errors are shown by the status bar, so results are not reported twice.
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S)) {
            let _ = self.save_now();
        }
        self.tick_backups(&ctx);
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

/// Renders one block; returns true when its content or checkbox changed.
fn block_field(ui: &mut egui::Ui, generation: u64, block: &mut Block) -> bool {
    let id = field_id(generation, "block", &block.id);
    ui.horizontal(|ui| {
        let mut changed = false;
        if block.is_todo() {
            changed |= ui.checkbox(&mut block.checked, "").changed();
        }
        let edit = if block.kind == "heading" {
            TextEdit::singleline(&mut block.content).font(egui::TextStyle::Heading)
        } else {
            TextEdit::multiline(&mut block.content).desired_rows(2)
        };
        changed |= ui.add(edit.id(id).desired_width(f32::INFINITY)).changed();
        changed
    })
    .inner
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
            kind: "paragraph".into(),
            content: String::new(),
            checked: false,
            position: 0,
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
                changed |= block_field(ui, 0, block);
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
        for kind in ["heading", "paragraph", "todo"] {
            app.db.add_block(&page, kind)?;
        }
        app.blocks = app.db.list_blocks(&page)?;
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
        let block = app.db.add_block(&notas, "paragraph")?;
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
