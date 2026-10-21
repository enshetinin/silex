//! Shown instead of the app when it cannot start (e.g. the database cannot
//! be opened). A packaged app has no terminal, so the error must be on
//! screen; nothing is written.
use eframe::egui::{self};

use super::{icons, theme};

pub struct StartupError {
    message: String,
    /// The database file, when known (for support and backups).
    path: Option<String>,
    theme_installed: bool,
}

impl StartupError {
    pub fn new(err: &anyhow::Error, path: Option<&std::path::Path>) -> Self {
        Self {
            message: format!("{err:#}"),
            path: path.map(|p| p.display().to_string()),
            theme_installed: false,
        }
    }

    pub(super) fn update_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !std::mem::replace(&mut self.theme_installed, true) {
            theme::install(&ctx);
            ui.set_style(ctx.global_style());
        }
        let p = theme::current(&ctx);
        ui.painter().rect_filled(ctx.content_rect(), 0.0, p.bg);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::same(theme::space::XXL as i8)),
            )
            .show(ui, |ui| {
                ui.set_max_width(560.0);
                ui.horizontal(|ui| {
                    icons::decoration(ui, icons::WARNING, theme::icon::LG, p.warning);
                    ui.heading("Silex no se ha podido abrir");
                });
                ui.add_space(theme::space::MD);
                ui.label("No ha podido abrir tus datos. Este es el error:");
                ui.add_space(theme::space::SM);
                egui::Frame::new()
                    .fill(p.subtle)
                    .corner_radius(theme::radius::MD)
                    .inner_margin(egui::Margin::same(theme::space::MD as i8))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(&self.message).wrap().selectable(true));
                    });
                if let Some(path) = &self.path {
                    ui.add_space(theme::space::SM);
                    ui.label(egui::RichText::new("Archivo de datos:").small().weak());
                    ui.add(
                        egui::Label::new(egui::RichText::new(path).small().monospace())
                            .wrap()
                            .selectable(true),
                    );
                }
                ui.add_space(theme::space::LG);
                ui.horizontal(|ui| {
                    if ui.button("Copiar el error").clicked() {
                        let mut text = self.message.clone();
                        if let Some(path) = &self.path {
                            text.push_str(&format!("\nArchivo de datos: {path}"));
                        }
                        ctx.copy_text(text);
                    }
                    if ui.button("Cerrar").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}

impl eframe::App for StartupError {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update_ui(ui);
    }
}
