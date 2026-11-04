//! Background work: backups, Markdown import and exports.
use std::path::PathBuf;

use anyhow::{Context, Result};
use eframe::egui::{self};
use silex_desktop::db::backup::{self, BackupJob};

use super::*;

impl SilexApp {
    /// Exports the open page as it is on screen (saved first when possible).
    pub(super) fn export_current_page(&mut self, ctx: &egui::Context) -> Result<()> {
        let dir = silex_desktop::export::export_root()?;
        self.export_current_page_to(ctx, &dir)
    }

    /// Exports the open page as Markdown into `dir`. A page with files, in
    /// a database on disk, is exported on a worker thread (H7.9): its
    /// attachments can take a while to write. The rest takes no time.
    pub(super) fn export_current_page_to(
        &mut self,
        ctx: &egui::Context,
        dir: &std::path::Path,
    ) -> Result<()> {
        // A save error is shown by the status bar; export what the user sees anyway.
        let _ = self.save_now();
        let has_files = self
            .blocks
            .iter()
            .any(|b| b.kind == silex_desktop::model::BlockKind::File);
        if self.database.is_none()
            && has_files
            && let Some(db_path) = self.db.path()
        {
            if self.page_export.is_some() {
                self.notice = Some("Ya se está exportando una página.".into());
                return Ok(());
            }
            let repaint = ctx.clone();
            let job = silex_desktop::export::PageExportJob::spawn(
                db_path,
                dir.to_path_buf(),
                self.title.clone(),
                self.open_icon(),
                self.blocks.clone(),
                move || repaint.request_repaint(),
            )?;
            self.page_export = Some((job, display_title(&self.title).to_owned()));
            self.last_export = None;
            return Ok(());
        }
        let path = match &self.database {
            Some(state) => {
                let title =
                    silex_desktop::markdown::titled(self.open_icon().as_deref(), &self.title);
                let markdown = silex_desktop::markdown::database_to_markdown(
                    &title,
                    &state.properties,
                    &state.rows,
                    &state.values,
                );
                silex_desktop::export::export_markdown(dir, &self.title, &markdown)?
            }
            None => silex_desktop::export::export_page_with_files(
                &self.db,
                dir,
                &self.title,
                self.open_icon().as_deref(),
                &self.blocks,
            )?,
        };
        self.last_export = Some(path);
        Ok(())
    }

    /// Exports the open database as CSV (H11.5): what its view shows, or
    /// every row and column with `all`.
    pub(super) fn export_database_csv(&mut self, ctx: &egui::Context, all: bool) -> Result<()> {
        let dir = silex_desktop::export::export_root()?;
        self.export_database_csv_to(ctx, &dir, all)
    }

    /// [`Self::export_database_csv`] into `dir`. The text is made here from
    /// the table on screen (names typed and not saved yet included); the
    /// file is written on the `silex-export` thread.
    pub(super) fn export_database_csv_to(
        &mut self,
        ctx: &egui::Context,
        dir: &std::path::Path,
        all: bool,
    ) -> Result<()> {
        use silex_desktop::csv::{CsvScope, database_to_csv};
        if self.csv_export.is_some() {
            self.notice = Some("Ya se está exportando una base a CSV.".into());
            return Ok(());
        }
        // A save error is shown by the status bar; export what the user sees anyway.
        let _ = self.save_now();
        let Some(state) = &self.database else {
            anyhow::bail!("La página abierta no es una base de datos");
        };
        let fallback = silex_desktop::view::ViewConfig::default();
        let scope = if all {
            CsvScope::All
        } else {
            CsvScope::View(state.config().unwrap_or(&fallback))
        };
        let csv = database_to_csv(&state.properties, &state.rows, &state.values, scope);
        let title = display_title(&self.title).to_owned();
        let repaint = ctx.clone();
        let job = silex_desktop::export::CsvExportJob::spawn_csv(
            dir.to_path_buf(),
            title.clone(),
            csv,
            move || repaint.request_repaint(),
        )?;
        self.csv_export = Some((job, title));
        self.last_export = None;
        Ok(())
    }

    /// Collects the CSV export from its worker, when it is done.
    pub(super) fn tick_csv_export(&mut self) {
        let Some(result) = self.csv_export.as_ref().and_then(|(job, _)| job.poll()) else {
            return;
        };
        self.csv_export = None;
        match result {
            Ok(path) => self.last_export = Some(path),
            Err(err) => self.notice = Some(format!("No se pudo exportar a CSV: {err:#}")),
        }
    }

    pub(super) fn describe_last_backup(&self) -> String {
        let Some(path) = self.db.path() else {
            return "Copias no disponibles (base en memoria)".into();
        };
        match backup::newest_stamp(&backup::backups_dir(&path)) {
            Ok(Some(stamp)) => format!("Última copia: {}", backup::describe_stamp(&stamp)),
            Ok(None) => "Aún no hay copias".into(),
            Err(err) => format!("Copias: {err:#}"),
        }
    }

    pub(super) fn start_backup(&mut self, ctx: &egui::Context) {
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
    pub(super) fn tick_backups(&mut self, ctx: &egui::Context) {
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

    /// Starts exporting every page to a new folder under `root`, in the background.
    pub(super) fn start_workspace_export(
        &mut self,
        ctx: &egui::Context,
        root: PathBuf,
    ) -> Result<()> {
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

    /// Collects the page export from its worker, when it is done.
    pub(super) fn tick_page_export(&mut self) {
        let Some(result) = self.page_export.as_ref().and_then(|(job, _)| job.poll()) else {
            return;
        };
        self.page_export = None;
        match result {
            Ok(path) => self.last_export = Some(path),
            Err(err) => self.notice = Some(format!("No se pudo exportar la página: {err:#}")),
        }
    }

    pub(super) fn tick_workspace_export(&mut self) {
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

    /// Files dropped on the window: `.md` files are imported as pages, and
    /// any other file is attached to the open page (ADR 0007). Both in the
    /// background.
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
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
        // Both kinds: two results, shown together.
        self.notice_group = if markdown.is_empty() || other.is_empty() {
            0
        } else {
            2
        };
        if !other.is_empty() {
            self.attach_dropped(ctx, other);
        }
        if markdown.is_empty() {
            return;
        }
        if self.import_job.is_some() {
            self.notice = Some("Ya hay una importación en curso.".into());
            return;
        }
        let repaint = ctx.clone();
        match silex_desktop::import::ImportJob::spawn(markdown, move || repaint.request_repaint()) {
            Ok(job) => self.import_job = Some(job),
            Err(err) => self.notice = Some(format!("Importación: {err:#}")),
        }
    }

    /// Creates the pages read by the import job (one transaction each) and
    /// opens the last one.
    pub(super) fn tick_import(&mut self) {
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
                let id = self
                    .db
                    .create_page_with_blocks(&page.title, None, &page.blocks)?;
                // A `# 🌾 Título` gives the page its icon (H12.8).
                if let Some(icon) = &page.icon {
                    self.db.set_page_icon(&id, Some(icon))?;
                }
                Ok(id)
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
        if !errors.is_empty() {
            message.push_str(&format!(" No se importaron: {}", errors.join("; ")));
        }
        self.post_notice(message);
    }

    /// Shows a notice; the second of a drop's two results is added to the
    /// first (see `notice_group`).
    pub(super) fn post_notice(&mut self, message: String) {
        let second = self.notice_group == 1;
        self.notice_group = self.notice_group.saturating_sub(1);
        self.notice = Some(match (second, self.notice.take()) {
            (true, Some(first)) => format!("{first} {message}"),
            _ => message,
        });
    }

    /// Centered hint while files are dragged over the window.
    pub(super) fn drop_hint(ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        egui::Area::new(egui::Id::unique("silex_drop_hint"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.heading("Suelta los archivos aquí");
                    ui.label(
                        "Los .md se importan como páginas; los demás se adjuntan a esta página (hasta 20 MB cada uno).",
                    );
                });
            });
    }
}
