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
    /// What waits for «Borrar para siempre» to be confirmed (H11.3).
    pub(super) confirm: Option<Purge>,
}

/// Deleting for good from the trash (ADR 0013): one entry, or all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Purge {
    /// The entry's page id, its title and how many pages went with it.
    Item(String, String, usize),
    /// Every entry: how many pages in all.
    All(usize),
}

impl Purge {
    /// What the confirmation says goes.
    fn what(&self) -> String {
        match self {
            Purge::Item(_, title, 0) => format!("«{title}» se borrará para siempre."),
            Purge::Item(_, title, 1) => {
                format!("«{title}» y 1 subpágina o fila se borrarán para siempre.")
            }
            Purge::Item(_, title, n) => {
                format!("«{title}» y {n} subpáginas o filas se borrarán para siempre.")
            }
            Purge::All(1) => "Se borrará para siempre 1 página.".to_owned(),
            Purge::All(n) => {
                format!("Se borrarán para siempre {n} páginas (con sus subpáginas y filas).")
            }
        }
    }
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
            Ok(items) => {
                self.trash_dialog = Some(TrashDialog {
                    items,
                    error: None,
                    confirm: None,
                });
            }
            Err(err) => self.notice = Some(format!("No se puede abrir la papelera: {err:#}")),
        }
    }

    pub(super) fn trash_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.trash_dialog.as_ref() else {
            return;
        };
        let p = theme::current(ctx);
        let mut restore = None;
        let mut close = false;
        let mut ask = None;
        let mut confirmed = false;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("silex_trash")).show(ctx, |ui| {
            // Deleting for good asks first, in the same window (ADR 0013).
            if let Some(purge) = &dialog.confirm {
                ui.heading("¿Borrar para siempre?");
                ui.add_space(theme::space::SM);
                ui.label(purge.what());
                ui.label(
                    "No se puede deshacer. Las copias de seguridad anteriores aún lo conservan.",
                );
                ui.weak(
                    "Los archivos adjuntos que queden sin uso se borran con «Liberar espacio».",
                );
                ui.add_space(theme::space::MD);
                ui.horizontal(|ui| {
                    confirmed = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Borrar para siempre").color(p.on_accent),
                            )
                            .fill(p.danger),
                        )
                        .accessible_name("Confirmar: borrar para siempre")
                        .clicked();
                    cancel = ui.button("Cancelar").clicked();
                });
                return;
            }
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
                            let title = display_title(&item.page.title);
                            ui.label(title);
                            if item.subpages > 0 {
                                ui.weak(format!("+{} subpágina(s)", item.subpages));
                            }
                            ui.weak(describe_timestamp(&item.deleted_at));
                            if ui.button("Restaurar").clicked() {
                                restore = Some(item.page.id.clone());
                            }
                            if ui
                                .button(egui::RichText::new("Borrar para siempre").color(p.danger))
                                .accessible_name(format!("Borrar «{title}» para siempre"))
                                .clicked()
                            {
                                ask = Some(Purge::Item(
                                    item.page.id.clone(),
                                    title.to_owned(),
                                    item.subpages,
                                ));
                            }
                        });
                    }
                });
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                close = ui.button("Cerrar").clicked();
                let pages = dialog.items.iter().map(|i| i.subpages + 1).sum();
                if ui
                    .add_enabled(
                        !dialog.items.is_empty(),
                        egui::Button::new(
                            egui::RichText::new("Vaciar la papelera").color(p.danger),
                        ),
                    )
                    .clicked()
                {
                    ask = Some(Purge::All(pages));
                }
            });
        });
        let confirming = dialog.confirm.clone();
        // Escape or a click outside: out of the confirmation, else closes.
        if confirming.is_some() && (cancel || response.should_close()) {
            if let Some(dialog) = self.trash_dialog.as_mut() {
                dialog.confirm = None;
            }
            return;
        }
        if let Some(purge) = ask {
            if let Some(dialog) = self.trash_dialog.as_mut() {
                dialog.confirm = Some(purge);
                dialog.error = None;
            }
            return;
        }
        if confirmed && let Some(purge) = confirming {
            match self.purge_from_trash(&purge) {
                Ok(()) => self.open_trash_dialog(),
                Err(err) => {
                    if let Some(dialog) = self.trash_dialog.as_mut() {
                        dialog.confirm = None;
                        dialog.error = Some(format!("{err:#}"));
                    }
                }
            }
            return;
        }
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

    /// Deletes for good what the trash confirmed (ADR 0013). The open page
    /// is saved first; links to what went now point nowhere.
    fn purge_from_trash(&mut self, purge: &Purge) -> Result<()> {
        self.save_now()?;
        let deleted = match purge {
            Purge::Item(id, _, _) => self.db.purge_trash_item(id)?,
            Purge::All(_) => self.db.empty_trash()?,
        };
        // «Deshacer» could not bring back a row that is gone.
        self.trash_notice = None;
        self.reload_pages()?;
        self.load_links()?;
        self.notice = Some(match deleted {
            1 => "Se borró 1 página para siempre.".to_owned(),
            n => format!("Se borraron {n} páginas para siempre."),
        });
        Ok(())
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
        if let Some((_, title)) = &self.csv_export {
            let text = format!("Exportando «{title}» a CSV…");
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
                icons::decoration(ui, icons::CHECK, 14.0, p.text_muted);
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
                icons::decoration(ui, icons::WARNING, theme::icon::MD, p.danger);
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
                    icons::decoration(ui, icon, theme::icon::MD, p.text_muted);
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

    /// A row just sent to the trash from its table or board, with
    /// «Deshacer» (the same step as Cmd+Z) and «Cerrar aviso» (H11.1).
    pub(super) fn trash_notice_bar(&mut self, ui: &mut egui::Ui) {
        let Some(notice) = self.current_trash_notice() else {
            return;
        };
        let (mut undo, mut close) = (false, false);
        Self::banner(ui, icons::TRASH, &notice.text, |ui| {
            // Left to right, so Tab visits them in the order they are seen.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                right_aligned(ui, egui::Id::unique("silex-trash-notice"), |ui| {
                    let shortcut = ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
                        egui::Modifiers::COMMAND,
                        egui::Key::Z,
                    ));
                    undo = ui
                        .button("Deshacer")
                        .on_hover_text(format!("Devolver a su sitio   {shortcut}"))
                        .accessible_name("Deshacer el envío a la papelera")
                        .clicked();
                    close = ui
                        .add(
                            egui::Button::new(icons::icon(icons::CLOSE)).frame_when_inactive(false),
                        )
                        .on_hover_text("Cerrar aviso")
                        .accessible_name("Cerrar aviso")
                        .clicked();
                });
            });
        });
        if undo {
            self.undo_table();
        } else if close {
            self.trash_notice = None;
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
