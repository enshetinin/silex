//! Dialogs (trash, restore, close) and the status, notice and export bars.
use std::path::PathBuf;

use anyhow::{Context, Result};
use eframe::egui::{self};
use silex_desktop::{
    db::backup::{self},
    model::TrashItem,
    save_state::SaveStatus,
};

use super::*;
use super::{icons, theme};

pub(super) struct TrashDialog {
    pub(super) items: Vec<TrashItem>,
    pub(super) error: Option<String>,
}

/// Backup picker; restoring needs an explicit second click on a selected copy.
pub(super) struct RestoreDialog {
    pub(super) backups: Vec<PathBuf>,
    pub(super) selected: Option<usize>,
    pub(super) error: Option<String>,
}

pub(super) enum CloseAction {
    Retry,
    Discard,
    Cancel,
}

impl SilexApp {
    pub(super) fn open_trash_dialog(&mut self) {
        match self.db.list_trash() {
            Ok(items) => self.trash_dialog = Some(TrashDialog { items, error: None }),
            Err(err) => self.notice = Some(format!("No se puede abrir la papelera: {err:#}")),
        }
    }

    pub(super) fn trash_dialog(&mut self, ctx: &egui::Context) {
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

    /// Keyboard shortcuts in a window (the sidebar shows them in a menu).
    pub(super) fn shortcuts_dialog(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_dialog {
            return;
        }
        let response =
            egui::Modal::new(egui::Id::unique("silex_shortcuts_dialog")).show(ctx, |ui| {
                ui.heading("Atajos de teclado");
                ui.add_space(theme::space::SM);
                Self::shortcuts_help(ui);
                ui.add_space(theme::space::MD);
                ui.button("Cerrar").clicked()
            });
        if response.inner || response.should_close() {
            self.shortcuts_dialog = false;
        }
    }

    /// "Exportada a …" with a button to open the folder.
    pub(super) fn export_bar(&mut self, ui: &mut egui::Ui) {
        // A page with attachments being exported (H7.9).
        if let Some((_, title)) = &self.page_export {
            let text = format!("Exportando «{title}» y sus adjuntos…");
            Self::banner(ui, icons::EXPORT, &text, |ui| {
                ui.spinner();
            });
            return;
        }
        let Some(path) = self.last_export.clone() else {
            return;
        };
        // A workspace export is a folder: open it; for a page, its folder.
        let folder = if path.is_dir() {
            Some(path.clone())
        } else {
            path.parent().map(|p| p.to_path_buf())
        };
        let (mut open, mut close) = (false, false);
        let text = format!("Exportado a {}", path.display());
        Self::banner(ui, icons::EXPORT, &text, |ui| {
            close = ui
                .add(egui::Button::new(icons::icon(icons::CLOSE)).frame_when_inactive(false))
                .on_hover_text("Cerrar")
                .accessible_name("Cerrar aviso de exportación")
                .clicked();
            open = ui.small_button("Abrir carpeta").clicked();
        });
        if open && let Some(dir) = folder {
            let result = silex_desktop::export::open_in_file_manager(&dir);
            self.report(result);
        }
        if close {
            self.last_export = None;
        }
    }

    pub(super) fn open_restore_dialog(&mut self) {
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
    pub(super) fn restore(&mut self, backup_file: &std::path::Path) -> Result<PathBuf> {
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

    pub(super) fn restore_dialog(&mut self, ctx: &egui::Context) {
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

    /// Always-visible save state; an error offers an immediate retry.
    /// Discreet save state (top bar, laid out right to left).
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::current(ui.ctx());
        match self.save.status() {
            SaveStatus::Saved => {
                ui.label(egui::RichText::new("Guardado").small().color(p.text_muted));
                ui.label(icons::icon(icons::CHECK).size(14.0).color(p.text_muted));
            }
            SaveStatus::Dirty => {
                ui.label(
                    egui::RichText::new("Guardando…")
                        .small()
                        .color(p.text_muted),
                );
            }
            SaveStatus::Failed { message } => {
                let message = message.to_owned();
                if ui.small_button("Reintentar").clicked() {
                    let _ = self.save_now();
                }
                ui.label(
                    egui::RichText::new("Error al guardar")
                        .small()
                        .color(p.danger),
                )
                .on_hover_text(message);
                ui.label(icons::icon(icons::WARNING).color(p.danger));
            }
        }
    }

    /// A quiet banner with an icon, a message and optional buttons.
    fn banner(ui: &mut egui::Ui, icon: &str, text: &str, add_buttons: impl FnOnce(&mut egui::Ui)) {
        let p = theme::current(ui.ctx());
        ui.add_space(theme::space::SM);
        egui::Frame::new()
            .fill(p.subtle)
            .corner_radius(theme::radius::MD)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(icons::icon(icon).color(p.text_muted));
                    ui.add(egui::Label::new(egui::RichText::new(text).color(p.text)).wrap());
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        add_buttons,
                    );
                });
            });
    }

    pub(super) fn notice_bar(&mut self, ui: &mut egui::Ui) {
        let Some(notice) = self.notice.clone() else {
            return;
        };
        let mut close = false;
        Self::banner(ui, icons::INFO, &notice, |ui| {
            close = ui
                .add(egui::Button::new(icons::icon(icons::CLOSE)).frame_when_inactive(false))
                .on_hover_text("Cerrar aviso")
                .accessible_name("Cerrar aviso")
                .clicked();
        });
        if close {
            self.notice = None;
        }
    }

    /// Intercepts window close: save first; if that fails, keep the window
    /// open and let the user retry or explicitly discard.
    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
        if self.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let _ = self.save_now();
        if !self.save.can_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_dialog = true;
        }
    }

    pub(super) fn close_dialog(&mut self, ctx: &egui::Context) {
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
