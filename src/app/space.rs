//! «Espacio» in the «Copias y exportación» menu (H7.4, ADR 0007): what the
//! database, its backups and its unused attachments take, and «Liberar
//! espacio», which asks first and then deletes the unused attachments and
//! compacts the database in the background.
use std::collections::HashSet;

use anyhow::Result;
use eframe::egui;
use silex_desktop::attachment::format_size;
use silex_desktop::db::{SpaceUsage, backup};
use silex_desktop::space::{FreeSpaceJob, Freed, free_space};

use super::theme;
use super::*;

/// Free space inside the file worth compacting for, with no unused
/// attachments (smaller amounts are not worth rewriting the file).
const WORTH_COMPACTING: u64 = 1024 * 1024;

/// What the menu shows, read when it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SpaceInfo {
    pub(super) usage: SpaceUsage,
    /// Backups (count and bytes); `None` for an in-memory database.
    pub(super) backups: Option<(usize, u64)>,
}

impl SpaceInfo {
    fn can_free(&self) -> bool {
        self.usage.unused_attachments > 0 || self.usage.free >= WORTH_COMPACTING
    }
}

/// «Liberar espacio»: asked, running, and what it did.
#[derive(Default)]
pub(super) struct Space {
    pub(super) info: Option<SpaceInfo>,
    /// The confirmation is open.
    pub(super) asking: bool,
    job: Option<FreeSpaceJob>,
}

impl SilexApp {
    /// Reads the sizes again (cheap: no attachment data is read).
    pub(super) fn refresh_space(&mut self) {
        let backups = self
            .db
            .path()
            .and_then(|path| backup::backups_size(&backup::backups_dir(&path)).ok());
        match self.db.space_usage() {
            Ok(usage) => self.space.info = Some(SpaceInfo { usage, backups }),
            Err(err) => {
                self.space.info = None;
                self.notice = Some(format!("No se pudo medir el espacio: {err:#}"));
            }
        }
    }

    /// The «Espacio» part of the backups menu.
    pub(super) fn space_section(&mut self, ui: &mut egui::Ui) {
        let caption = |ui: &mut egui::Ui, text: String| {
            ui.label(egui::RichText::new(text).small().weak());
        };
        caption(ui, "Espacio".into());
        let Some(info) = self.space.info else {
            return;
        };
        let usage = info.usage;
        let mut database = format!("Base de datos: {}", format_size(usage.database));
        if usage.used_bytes > 0 {
            database.push_str(&format!(" (adjuntos: {})", format_size(usage.used_bytes)));
        }
        caption(ui, database);
        if let Some((count, bytes)) = info.backups {
            let copies = match count {
                1 => "1 copia".to_owned(),
                n => format!("{n} copias"),
            };
            caption(
                ui,
                format!("Copias de seguridad: {copies}, {}", format_size(bytes)),
            );
        }
        caption(
            ui,
            match usage.unused_attachments {
                0 => "Ningún adjunto sin usar".to_owned(),
                1 => format!("1 adjunto sin usar, {}", format_size(usage.unused_bytes)),
                n => format!("{n} adjuntos sin usar, {}", format_size(usage.unused_bytes)),
            },
        );
        let running = self.space.job.is_some();
        let label = if running {
            "Liberando espacio…"
        } else {
            "Liberar espacio…"
        };
        let button = ui
            .add_enabled(!running && info.can_free(), egui::Button::new(label))
            .on_disabled_hover_text("No hay nada que liberar");
        if button.clicked() {
            self.space.asking = true;
            ui.close();
        }
    }

    /// The confirmation: what goes, and that older backups keep it.
    pub(super) fn free_space_dialog(&mut self, ctx: &egui::Context) {
        if !self.space.asking {
            return;
        }
        let Some(info) = self.space.info else {
            self.space.asking = false;
            return;
        };
        let p = theme::current(ctx);
        let usage = info.usage;
        let mut confirm = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("silex_free_space")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading("Liberar espacio");
            ui.add_space(theme::space::SM);
            let what = match usage.unused_attachments {
                0 => "Se compactará la base de datos para devolver el espacio libre al disco."
                    .to_owned(),
                1 => format!(
                    "Se borrará 1 adjunto que ya no usa ninguna página ({}) y se compactará la base de datos.",
                    format_size(usage.unused_bytes)
                ),
                n => format!(
                    "Se borrarán {n} adjuntos que ya no usa ninguna página ({}) y se compactará la base de datos.",
                    format_size(usage.unused_bytes)
                ),
            };
            ui.label(what);
            ui.add_space(theme::space::XS);
            ui.label(
                egui::RichText::new(
                    "No se puede deshacer, pero las copias de seguridad anteriores los conservan. \
                     Los archivos que aún puedes recuperar con Cmd+Z en esta página no se borran.",
                )
                .small()
                .color(p.text_muted),
            );
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new("Liberar espacio").color(p.danger))
                    .clicked()
                {
                    confirm = true;
                }
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
            });
        });
        if confirm {
            self.space.asking = false;
            let result = self.start_free_space(ctx);
            self.report(result);
        } else if cancel || response.should_close() {
            self.space.asking = false;
        }
    }

    /// Saves everything, then frees space keeping what undo can bring back.
    pub(super) fn start_free_space(&mut self, ctx: &egui::Context) -> Result<()> {
        self.save_now()?;
        let mut keep: HashSet<String> = self.history.attachments();
        keep.extend(self.blocks.iter().filter_map(|b| b.attachment.clone()));
        let Some(path) = self.db.path() else {
            let result = free_space(&mut self.db, &keep);
            self.space_done(result);
            return Ok(());
        };
        let repaint = ctx.clone();
        self.space.job = Some(FreeSpaceJob::spawn(path, keep, move || {
            repaint.request_repaint();
        })?);
        Ok(())
    }

    pub(super) fn tick_free_space(&mut self) {
        let Some(result) = self.space.job.as_ref().and_then(FreeSpaceJob::poll) else {
            return;
        };
        self.space.job = None;
        self.space_done(result);
    }

    fn space_done(&mut self, result: Result<Freed>) {
        match result {
            Ok(freed) => {
                let files = match freed.attachments {
                    0 => String::new(),
                    1 => " (1 adjunto sin usar)".to_owned(),
                    n => format!(" ({n} adjuntos sin usar)"),
                };
                self.notice = Some(format!("Se liberaron {}{files}.", format_size(freed.bytes)));
            }
            Err(err) => self.notice = Some(format!("No se pudo liberar espacio: {err:#}")),
        }
        self.refresh_space();
    }
}
