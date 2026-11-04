//! «Cambiar tipo» of a database property (H13.4b, ADR 0019): the dialog
//! that picks the new type and shows what the change does before doing it,
//! and the change itself, one step of the table's undo.
use anyhow::{Result, anyhow};
use eframe::egui;
use silex_desktop::db::KindChange;
use silex_desktop::property::PropertyKind;

use super::*;
use super::{icons, theme};

/// More new options than this asks a second time (ADR 0019).
const MANY_OPTIONS: usize = 100;
/// New options named in the preview; the rest are counted.
const NAMED_OPTIONS: usize = 5;

/// The open «Cambiar tipo» dialog.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct KindDialog {
    pub(super) property: String,
    pub(super) name: String,
    pub(super) from: PropertyKind,
    /// The type chosen, with what changing to it does.
    pub(super) to: Option<(PropertyKind, KindChange)>,
    /// Waiting for the second «sí» because many options will be created.
    pub(super) many: bool,
    pub(super) error: Option<String>,
    /// Give the keyboard focus to the first type on the next frame.
    focus: bool,
}

/// What the dialog says the change does, a sentence per line. Pure, so
/// the wording is tested without drawing.
pub(super) fn describe(change: &KindChange, from: PropertyKind, to: PropertyKind) -> Vec<String> {
    let mut lines = Vec::new();
    let (from_name, to_name) = (from.label(), to.label());
    if to.is_computed() {
        let date = if to == PropertyKind::Created {
            "cuándo se creó"
        } else {
            "cuándo se editó por última vez"
        };
        lines.push(format!("La columna mostrará {date} cada fila."));
        match change.emptied {
            0 => {}
            1 => lines.push(format!(
                "El valor guardado deja de verse. No se pierde: vuelve si cambias otra vez a «{from_name}»."
            )),
            n => lines.push(format!(
                "Los {n} valores guardados dejan de verse. No se pierden: vuelven si cambias otra vez a «{from_name}»."
            )),
        }
    } else {
        if change.converted + change.restored + change.emptied == 0 {
            lines.push("La propiedad no tiene valores todavía.".to_owned());
        }
        match change.converted {
            0 => {}
            1 => lines.push(format!("1 valor se convierte a «{to_name}».")),
            n => lines.push(format!("{n} valores se convierten a «{to_name}».")),
        }
        match change.restored {
            0 => {}
            1 => lines.push(format!(
                "Vuelve 1 valor que la propiedad tenía antes como «{to_name}»."
            )),
            n => lines.push(format!(
                "Vuelven {n} valores que la propiedad tenía antes como «{to_name}»."
            )),
        }
        match change.emptied {
            0 => {}
            1 => lines.push(format!(
                "1 valor no encaja y queda vacío. No se pierde: vuelve si cambias otra vez a «{from_name}»."
            )),
            n => lines.push(format!(
                "{n} valores no encajan y quedan vacíos. No se pierden: vuelven si cambias otra vez a «{from_name}»."
            )),
        }
    }
    let options = &change.new_options;
    if !options.is_empty() {
        let mut names: Vec<String> = options
            .iter()
            .take(NAMED_OPTIONS)
            .map(|n| format!("«{}»", n.trim()))
            .collect();
        let rest = options.len() - names.len();
        let list = if rest > 0 {
            format!("{} y {rest} más", names.join(", "))
        } else {
            let last = names.pop().unwrap_or_default();
            if names.is_empty() {
                last
            } else {
                format!("{} y {last}", names.join(", "))
            }
        };
        lines.push(match options.len() {
            1 => format!("Se creará 1 opción: {list}."),
            n => format!("Se crearán {n} opciones: {list}."),
        });
    }
    match change.filters {
        0 => {}
        1 => lines.push("Se quitará 1 filtro de las vistas.".to_owned()),
        n => lines.push(format!("Se quitarán {n} filtros de las vistas.")),
    }
    lines
}

impl SilexApp {
    /// Opens «Cambiar tipo» for the property `id` of the open database.
    pub(super) fn open_kind_dialog(&mut self, id: &str) -> Result<()> {
        let state = self
            .database
            .as_mut()
            .ok_or_else(|| anyhow!("No hay ninguna base de datos abierta"))?;
        let property = state
            .properties
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow!("La propiedad ya no existe"))?;
        state.kind_change = Some(KindDialog {
            property: property.id.clone(),
            name: property.name.clone(),
            from: property.kind,
            to: None,
            many: false,
            error: None,
            focus: true,
        });
        Ok(())
    }

    /// The «Cambiar tipo» dialog, if open: the types to pick from, what the
    /// chosen one does, and the buttons.
    pub(super) fn kind_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.database.as_mut().and_then(|d| d.kind_change.take()) else {
            return;
        };
        let p = theme::current(ctx);
        let mut picked = None;
        let mut confirmed = false;
        let mut cancel = false;
        let width = (ctx.content_rect().width() - 64.0).clamp(260.0, 460.0);
        let response = egui::Modal::new(egui::Id::unique("silex_kind_change")).show(ctx, |ui| {
            ui.set_width(width);
            let new_options = dialog.to.as_ref().map_or(0, |(_, c)| c.new_options.len());
            if dialog.many {
                ui.heading(format!("¿Crear {new_options} opciones?"));
                ui.add_space(theme::space::SM);
                ui.label(format!(
                    "Cada valor distinto de «{}» será una opción. Son muchas para una selección: si no es lo que esperabas, vuelve y elige otro tipo.",
                    dialog.name
                ));
                ui.add_space(theme::space::MD);
                ui.horizontal(|ui| {
                    confirmed = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "Crear {new_options} opciones y cambiar"
                                ))
                                .color(p.on_accent),
                            )
                            .fill(p.accent),
                        )
                        .clicked();
                    cancel = ui.button("Volver").clicked();
                });
                return;
            }
            ui.heading(format!("Cambiar el tipo de «{}»", dialog.name));
            ui.weak(format!("Ahora es «{}».", dialog.from.label()));
            ui.add_space(theme::space::SM);
            menu_caption(ui, "Tipo nuevo");
            let chosen = dialog.to.as_ref().map(|(k, _)| *k);
            let mut first = true;
            for kind in PropertyKind::ALL {
                if kind == dialog.from {
                    continue;
                }
                let option = ui.add(egui::Button::selectable(
                    chosen == Some(kind),
                    (
                        icons::icon(property_icon(kind)),
                        format!("  {}", kind.label()),
                    ),
                ));
                if first && dialog.focus {
                    option.request_focus();
                    dialog.focus = false;
                }
                first = false;
                if option.clicked() {
                    picked = Some(kind);
                }
            }
            ui.add_space(theme::space::SM);
            ui.separator();
            match &dialog.to {
                Some((to, change)) => {
                    for line in describe(change, dialog.from, *to) {
                        ui.label(line);
                    }
                    ui.weak("Cmd+Z lo deshace.");
                }
                None => {
                    ui.weak("Elige el tipo nuevo para ver qué pasa con los valores.");
                }
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(theme::space::MD);
            ui.horizontal(|ui| {
                confirmed = ui
                    .add_enabled(
                        dialog.to.is_some(),
                        egui::Button::new(
                            egui::RichText::new("Cambiar tipo").color(p.on_accent),
                        )
                        .fill(p.accent),
                    )
                    .clicked();
                cancel = ui.button("Cancelar").clicked();
            });
        });
        // Escape or a click outside: out of the second question, else closes.
        if cancel || response.should_close() {
            if dialog.many {
                dialog.many = false;
                self.put_kind_dialog(dialog);
            }
            return;
        }
        if let Some(kind) = picked {
            dialog.error = None;
            match self.db.preview_kind_change(&dialog.property, kind) {
                Ok(change) => dialog.to = Some((kind, change)),
                Err(err) => {
                    dialog.to = None;
                    dialog.error = Some(format!("{err:#}"));
                }
            }
            self.put_kind_dialog(dialog);
            return;
        }
        let Some((to, change)) = dialog.to.clone().filter(|_| confirmed) else {
            self.put_kind_dialog(dialog);
            return;
        };
        if change.new_options.len() > MANY_OPTIONS && !dialog.many {
            dialog.many = true;
            self.put_kind_dialog(dialog);
            return;
        }
        if let Err(err) = self.change_kind(&dialog.property, to) {
            dialog.many = false;
            dialog.error = Some(format!("No se pudo cambiar el tipo: {err:#}"));
            self.put_kind_dialog(dialog);
        }
    }

    fn put_kind_dialog(&mut self, dialog: KindDialog) {
        if let Some(state) = self.database.as_mut() {
            state.kind_change = Some(dialog);
        }
    }

    /// Changes the type of the property `id` to `to` (ADR 0019): what is
    /// being typed in the table is saved first, and the change is one step
    /// of the table's undo.
    pub(super) fn change_kind(&mut self, id: &str, to: PropertyKind) -> Result<()> {
        self.save_table_edits()?;
        let name = self
            .database
            .as_ref()
            .and_then(|d| d.properties.iter().find(|p| p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let snapshot = self.db.change_property_kind(id, to)?;
        self.after_kind_change(id)?;
        self.record_table(TableOp::Kind { snapshot, to });
        self.notice = Some(format!(
            "«{name}» ahora es de tipo «{}». Cmd+Z lo deshace.",
            to.label()
        ));
        Ok(())
    }

    /// After a property changed type (or went back): its cells, options and
    /// the views' filters are read again, and nothing being edited keeps
    /// the old type.
    pub(super) fn after_kind_change(&mut self, id: &str) -> Result<()> {
        if let Some(state) = self.database.as_mut() {
            if state.editing.as_ref().is_some_and(|e| e.property == id) {
                state.editing = None;
            }
            if state.bulk.property.as_deref() == Some(id) {
                state.bulk = BulkValue::default();
            }
        }
        self.reload_properties()?;
        self.reload_views(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(converted: usize, restored: usize, emptied: usize) -> KindChange {
        KindChange {
            converted,
            restored,
            emptied,
            ..KindChange::default()
        }
    }

    #[test]
    fn the_preview_counts_each_kind_of_cell_in_words() {
        use PropertyKind::{Number, Text};
        assert_eq!(
            describe(&change(0, 0, 0), Text, Number),
            ["La propiedad no tiene valores todavía."]
        );
        assert_eq!(
            describe(&change(1, 1, 1), Text, Number),
            [
                "1 valor se convierte a «Número».",
                "Vuelve 1 valor que la propiedad tenía antes como «Número».",
                "1 valor no encaja y queda vacío. No se pierde: vuelve si cambias otra vez a «Texto».",
            ]
        );
        assert_eq!(
            describe(&change(12, 0, 3), Text, Number),
            [
                "12 valores se convierten a «Número».",
                "3 valores no encajan y quedan vacíos. No se pierden: vuelven si cambias otra vez a «Texto».",
            ]
        );
    }

    #[test]
    fn to_created_or_edited_says_what_the_column_shows() {
        use PropertyKind::{Created, Edited, Text};
        assert_eq!(
            describe(&change(0, 0, 2), Text, Created),
            [
                "La columna mostrará cuándo se creó cada fila.",
                "Los 2 valores guardados dejan de verse. No se pierden: vuelven si cambias otra vez a «Texto».",
            ]
        );
        assert_eq!(
            describe(&change(0, 0, 0), Text, Edited),
            ["La columna mostrará cuándo se editó por última vez cada fila."]
        );
    }

    #[test]
    fn new_options_are_named_up_to_five_and_filters_counted() {
        use PropertyKind::{Select, Text};
        let with = |names: &[&str], filters| KindChange {
            converted: names.len(),
            new_options: names.iter().map(|n| (*n).to_owned()).collect(),
            filters,
            ..KindChange::default()
        };
        let lines = describe(&with(&["Novela"], 1), Text, Select);
        assert_eq!(lines[1], "Se creará 1 opción: «Novela».");
        assert_eq!(lines[2], "Se quitará 1 filtro de las vistas.");
        let lines = describe(&with(&["Novela", "Cuento"], 2), Text, Select);
        assert_eq!(lines[1], "Se crearán 2 opciones: «Novela» y «Cuento».");
        assert_eq!(lines[2], "Se quitarán 2 filtros de las vistas.");
        let lines = describe(&with(&["A", "B", "C", "D", "E", "F", "G"], 0), Text, Select);
        assert_eq!(
            lines[1],
            "Se crearán 7 opciones: «A», «B», «C», «D», «E» y 2 más."
        );
        assert_eq!(lines.len(), 2, "no filters, no line");
    }
}
