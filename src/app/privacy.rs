//! Local data kept private (H7.12, ADR 0012), in the window: hardening
//! at start, the warning about a synced folder, and «Privacidad» in
//! «Ajustes».
use std::path::PathBuf;

use eframe::egui;
use silex_desktop::db::{Database, backup};

use super::theme;
use super::*;

/// The database (or its copies) is in a folder a cloud service syncs.
pub(super) struct SyncedWarning {
    pub(super) service: &'static str,
    pub(super) path: PathBuf,
}

impl SilexApp {
    /// At start: only the user can read the database and its copies, and
    /// a warning if a cloud service syncs their folder. Neither stops the
    /// app.
    pub(super) fn protect_local_data(&mut self) {
        let Some(path) = self.db.path() else {
            return;
        };
        let backups = backup::backups_dir(&path);
        let own = Database::is_default_location(&path);
        if let Err(err) = silex_desktop::privacy::harden(&path, &backups, own) {
            self.notice = Some(format!(
                "No se pudieron proteger los permisos de tus datos: {err:#}"
            ));
        }
        let service = silex_desktop::privacy::synced_folder(&path)
            .or_else(|| silex_desktop::privacy::synced_folder(&backups));
        if let Some(service) = service
            && !self.db.synced_warning_dismissed().unwrap_or(false)
        {
            self.synced_warning = Some(SyncedWarning { service, path });
        }
    }

    /// Deletes the copies of opened attachments (when the app closes).
    pub(super) fn clear_opened_copies(&self) {
        if let Some(folders) = &self.files.folders {
            // An app may still have one open (Windows refuses): no matter,
            // the next session empties the folder first.
            let _ = silex_desktop::attach_job::clear_opened(folders);
        }
    }

    /// «Tus datos están en una carpeta de …»: «Entendido» for this
    /// session, «No volver a avisar» for good.
    pub(super) fn synced_warning_dialog(&mut self, ctx: &egui::Context) {
        let Some(warning) = &self.synced_warning else {
            return;
        };
        let service = warning.service;
        let path = warning.path.display().to_string();
        let mut close = false;
        let mut never = false;
        let response = egui::Modal::new(egui::Id::unique("silex_synced_warning")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(format!("Tus datos están en una carpeta de {service}"));
            ui.add_space(theme::space::SM);
            ui.label(format!(
                "Silex guarda tus páginas en {path}. {service} sincroniza esa carpeta \
                 mientras Silex la usa, y eso puede dañar la base o crear copias en conflicto."
            ));
            ui.add_space(theme::space::XS);
            ui.label(
                "Es mejor guardarla en una carpeta que no se sincronice. Para llevar tus \
                 datos a otro equipo, usa una copia de seguridad y restáurala allí.",
            );
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                if ui.button("Entendido").clicked() {
                    close = true;
                }
                if ui.button("No volver a avisar").clicked() {
                    never = true;
                }
            });
        });
        if never {
            let result = self.db.dismiss_synced_warning();
            self.report(result);
        }
        if close || never || response.should_close() {
            self.synced_warning = None;
        }
    }

    /// «Privacidad» in «Ajustes»: where the data is, that nothing leaves
    /// the computer, and what protects a lost one (ADR 0012).
    pub(super) fn privacy_section(&mut self, ui: &mut egui::Ui) {
        ui.add_space(theme::space::SM);
        ui.separator();
        ui.label(egui::RichText::new("Privacidad").small().weak());
        ui.set_max_width(320.0);
        let Some(path) = self.db.path() else {
            ui.label(egui::RichText::new("Base en memoria: no se guarda nada.").small());
            return;
        };
        ui.add(
            egui::Label::new(egui::RichText::new(
                "Tus páginas y archivos se guardan solo en este equipo; Silex no usa la red.",
            ))
            .wrap(),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(path.display().to_string())
                    .small()
                    .weak(),
            )
            .wrap(),
        );
        if let Some(service) = silex_desktop::privacy::synced_folder(&path) {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "Esta carpeta la sincroniza {service}: puede dañar la base."
                    ))
                    .small(),
                )
                .wrap(),
            );
        }
        if ui.button("Mostrar la carpeta").clicked()
            && let Some(folder) = path.parent()
        {
            let result = silex_desktop::export::open_in_file_manager(folder);
            self.report(result);
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(
                    "Si pierdes el equipo, lo que protege tus datos es el cifrado de disco del \
                     sistema: FileVault en macOS, BitLocker en Windows o LUKS en Linux.",
                )
                .small()
                .weak(),
            )
            .wrap(),
        );
    }
}
