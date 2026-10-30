//! The window's top bar (H10): «Silex / path of the open page» at the
//! left, save state and settings at the right, a 1 px line under it.
use eframe::egui;
use silex_desktop::model::ancestors;

use super::{SilexApp, display_title, icons, theme};

/// Height of the top bar.
pub(super) const TOP_BAR: f32 = 48.0;
/// Space from the window edge to the bar's content, the same as from the
/// sidebar's edge to its rows' icons.
const INSET: f32 = 18.0;

impl SilexApp {
    pub(super) fn window_bar(&mut self, ui: &mut egui::Ui) {
        let p = theme::current(ui.ctx());
        let bar = ui.max_rect();
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            egui::Stroke::new(1.0, p.border),
        );
        let inner = bar.shrink2(egui::vec2(INSET, 0.0));
        let mut open = None;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = theme::space::SM;
                ui.label(
                    egui::RichText::new("Silex")
                        .size(15.0)
                        .family(egui::FontFamily::Name(theme::SEMIBOLD.into()))
                        .color(p.text),
                );
                // Path of the open page: ancestors (open them) and its title.
                if let Some(page_id) = self.selected.clone() {
                    let path: Vec<(String, String)> = ancestors(&self.pages, &page_id)
                        .into_iter()
                        .rev()
                        .filter_map(|id| {
                            self.find_page(&id).map(|page| {
                                (id.clone(), self.titled(&id, display_title(&page.title)))
                            })
                        })
                        .collect();
                    for (id, title) in &path {
                        slash(ui, p);
                        let link = ui.add(
                            egui::Label::new(egui::RichText::new(title).color(p.text_muted))
                                .sense(egui::Sense::click())
                                .truncate(),
                        );
                        // A link to the ancestor page for screen readers too.
                        ui.ctx().accesskit_node_builder(link.id, |node| {
                            node.set_role(egui::accesskit::Role::Link);
                            node.set_label(format!("Abrir «{title}»"));
                        });
                        if link.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if link.clicked() {
                            open = Some(id.clone());
                        }
                    }
                    slash(ui, p);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(self.titled(&page_id, display_title(&self.title)))
                                .color(p.text),
                        )
                        .truncate(),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = theme::space::XS;
                    let settings =
                        icons::button(ui, true, icons::SETTINGS, p.text_muted, "Ajustes", None);
                    egui::Popup::menu(&settings)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| self.settings_menu(ui));
                    ui.add_space(theme::space::SM);
                    self.status_bar(ui);
                });
            },
        );
        if let Some(id) = open {
            let result = self.open_page(id);
            self.report(result);
        }
    }
}

/// A faint «/» between parts of the path.
fn slash(ui: &mut egui::Ui, p: &theme::Palette) {
    ui.label(egui::RichText::new("/").color(p.text_faint));
}
