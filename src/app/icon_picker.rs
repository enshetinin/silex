//! The page icon picker (H12.8): a window with the emoji offered, by
//! group, a search in Spanish, room for any emoji pasted, and «Quitar
//! icono». Choosing is stored at once (like a favorite: not undone with
//! Cmd+Z).
use anyhow::Result;
use eframe::egui;
use silex_desktop::emoji;

use super::blocks::take_exact_key;
use super::theme;
use super::*;

/// Size of an emoji in the grid, and of its button.
const EMOJI_SIZE: f32 = 22.0;
const CELL: f32 = 36.0;

/// The picker: for which page, and what is typed.
#[derive(Debug, Clone, Default)]
pub(super) struct IconPicker {
    pub(super) page: String,
    pub(super) query: String,
    /// Focus the search field on the next frame.
    opening: bool,
}

impl SilexApp {
    /// Opens the icon picker for page `id`.
    pub(super) fn open_icon_picker(&mut self, id: &str) {
        self.icon_picker = Some(IconPicker {
            page: id.to_owned(),
            opening: true,
            ..IconPicker::default()
        });
    }

    /// Gives page `id` its icon, or takes it away (`None`).
    pub(super) fn set_page_icon(&mut self, id: &str, icon: Option<&str>) -> Result<()> {
        self.db.set_page_icon(id, icon)?;
        match icon {
            Some(icon) => self.page_icons.insert(id.to_owned(), icon.to_owned()),
            None => self.page_icons.remove(id),
        };
        self.refresh_row_times(&[id])
    }

    /// The picker, a window over the page. Intro takes the first emoji
    /// found, Esc closes.
    pub(super) fn icon_picker(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.icon_picker.take() else {
            return;
        };
        let p = theme::current(ctx);
        let found = emoji::search(&picker.query);
        // Something pasted or typed that is an emoji goes first.
        let typed = picker.query.trim();
        let pasted = (emoji::is_icon(typed) && !found.contains(&typed)).then(|| typed.to_owned());
        use egui::{Key, Modifiers as M};
        let mut chosen = take_exact_key(ctx, Key::Enter, M::NONE)
            .then(|| {
                pasted
                    .clone()
                    .or_else(|| found.first().map(|e| (*e).to_owned()))
            })
            .flatten();
        let mut close = take_exact_key(ctx, Key::Escape, M::NONE);
        let mut remove = false;
        let has_icon = self.page_icons.contains_key(&picker.page);
        let response = egui::Modal::new(egui::Id::unique("silex_icon_picker")).show(ctx, |ui| {
            ui.set_width(CELL * 10.0 + 20.0);
            ui.horizontal(|ui| {
                ui.heading("Icono de la página");
                if has_icon {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        remove = ui.button("Quitar icono").clicked();
                    });
                }
            });
            ui.add_space(theme::space::SM);
            let field = framed(
                ui,
                egui::TextEdit::singleline(&mut picker.query)
                    .hint_text(
                        egui::RichText::new("Busca (pan, viaje, idea…) o pega un emoji")
                            .color(p.text_faint),
                    )
                    .desired_width(f32::INFINITY),
            )
            .accessible_name("Buscar un icono");
            if std::mem::take(&mut picker.opening) {
                field.request_focus();
            }
            ui.add_space(theme::space::SM);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    if let Some(emoji) = &pasted
                        && emoji_button(ui, emoji, "el emoji escrito").clicked()
                    {
                        chosen = Some(emoji.clone());
                    }
                    if picker.query.trim().is_empty() {
                        for (group, list) in emoji::GROUPS {
                            ui.label(egui::RichText::new(*group).small().color(p.text_muted));
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                for (emoji, keys) in *list {
                                    let name = keys.split(' ').next().unwrap_or("emoji");
                                    if emoji_button(ui, emoji, name).clicked() {
                                        chosen = Some((*emoji).to_owned());
                                    }
                                }
                            });
                            ui.add_space(theme::space::XS);
                        }
                    } else if found.is_empty() && pasted.is_none() {
                        ui.label(
                            egui::RichText::new(
                                "Ningún icono tiene esa palabra. Puedes pegar cualquier emoji.",
                            )
                            .color(p.text_muted),
                        );
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            for emoji in &found {
                                let name = emoji::name(emoji).unwrap_or("emoji");
                                if emoji_button(ui, emoji, name).clicked() {
                                    chosen = Some((*emoji).to_owned());
                                }
                            }
                        });
                    }
                });
            ui.add_space(theme::space::SM);
            ui.label(
                egui::RichText::new("Intro elige el primero · Esc para cerrar")
                    .small()
                    .color(p.text_muted),
            );
        });
        close |= response.should_close();
        let page = picker.page.clone();
        let result = if let Some(icon) = chosen {
            self.set_page_icon(&page, Some(&icon))
        } else if remove {
            self.set_page_icon(&page, None)
        } else {
            if !close {
                self.icon_picker = Some(picker);
            }
            return;
        };
        self.report(result);
    }
}

/// One emoji of the picker, named for screen readers.
fn emoji_button(ui: &mut egui::Ui, emoji: &str, name: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(emoji).size(EMOJI_SIZE))
            .frame_when_inactive(false)
            .min_size(egui::vec2(CELL, CELL)),
    )
    .accessible_name(format!("Icono: {name}"))
}
