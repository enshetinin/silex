//! Background work: backups, Markdown import and exports.
use std::path::PathBuf;

use anyhow::{Context, Result};
use eframe::egui::{self};
use silex_desktop::db::backup::{self, BackupJob};

use super::*;

impl SilexApp {
    /// Exports the open page as it is on screen (saved first when possible).
    pub(super) fn export_current_page(&mut self) -> Result<()> {
        let dir = silex_desktop::export::export_root()?;
        self.export_current_page_to(&dir)
    }

    pub(super) fn export_current_page_to(&mut self, dir: &std::path::Path) -> Result<()> {
        // A save error is shown by the status bar; export what the user sees anyway.
        let _ = self.save_now();
        let path = match &self.database {
            Some(state) => {
                let markdown = silex_desktop::markdown::database_to_markdown(
                    &self.title,
                    &state.properties,
                    &state.rows,
                    &state.values,
                );
                silex_desktop::export::export_markdown(dir, &self.title, &markdown)?
            }
            None => silex_desktop::export::export_page(dir, &self.title, &self.blocks)?,
        };
        self.last_export = Some(path);
        Ok(())
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

    /// Files dropped on the window: `.md` files are imported in the background.
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
    pub(super) fn drop_hint(ctx: &egui::Context) {
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
}
