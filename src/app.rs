use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::{
        Database,
        backup::{self, BackupJob},
    },
    model::{Block, Page},
    save_state::{SaveState, SaveStatus},
};

pub struct SilexApp {
    db: Database,
    pages: Vec<Page>,
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
        if let Some(page) = app.pages.first() {
            app.open_page(page.id.clone())?;
        }
        Ok(app)
    }

    fn open_page(&mut self, id: String) -> Result<()> {
        self.save_now()?;
        let page = self.db.get_page(&id)?;
        if let Some(page) = page {
            self.blocks = self.db.list_blocks(&id)?;
            self.title = page.title;
            self.selected = Some(id);
        }
        Ok(())
    }

    fn reload_pages(&mut self) -> Result<()> {
        self.pages = self.db.list_pages()?;
        Ok(())
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
        ui.add(TextEdit::singleline(&mut self.query).hint_text("Buscar…"));
        ui.separator();
        ui.label("Páginas");
        let found = if self.query.is_empty() {
            Ok(self.pages.clone())
        } else {
            self.db.search_pages(&self.query)
        };
        match found {
            Ok(pages) => {
                let mut open = None;
                egui::ScrollArea::vertical()
                    .id_salt("silex_sidebar_pages")
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for page in &pages {
                            let selected = self.selected.as_deref() == Some(&page.id);
                            if ui.selectable_label(selected, &page.title).clicked() {
                                open = Some(page.id.clone());
                            }
                        }
                    });
                if let Some(id) = open {
                    let result = self.open_page(id);
                    self.report(result);
                }
            }
            Err(err) => self.notice = Some(format!("Error de búsqueda: {err:#}")),
        }
        ui.separator();
        ui.add(TextEdit::singleline(&mut self.new_page_title).hint_text("Nombre de página"));
        if ui.button("+ Crear página").clicked() {
            let result = (|| -> Result<()> {
                self.save_now()?;
                let title = self.new_page_title.trim();
                let title = if title.is_empty() {
                    "Sin título"
                } else {
                    title
                };
                let id = self.db.create_page(title, None)?;
                self.new_page_title.clear();
                self.reload_pages()?;
                self.open_page(id)
            })();
            self.report(result);
        }
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

impl eframe::App for SilexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.layout(ui);
        let ctx = ui.ctx().clone();
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
        // The UI may be idle between edits; wake up for the next autosave or retry.
        if let Some(at) = self.save.next_attempt() {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
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
