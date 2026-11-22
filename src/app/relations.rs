use super::*;

impl SilexApp {
    pub(super) fn relation_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some((source, mut name)) = self.relation_dialog.clone() else {
            return;
        };
        let mut target = None;
        let mut cancel = false;
        let response = egui::Modal::new(egui::Id::unique("relation-target")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Relacionar con una base de datos");
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("Nombre de la relación"));
            ui.label("Cada celda podrá enlazar una o varias entradas de la base elegida.");
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for page in self.pages.iter().filter(|p| p.kind == PageKind::Database) {
                        if ui.button(display_title(&page.title)).clicked() {
                            target = Some(page.id.clone());
                        }
                    }
                });
            cancel = ui.button("Cancelar").clicked();
        });
        if let Some(target) = target {
            let result = self
                .db
                .add_relation(&source, &name, &target)
                .and_then(|id| {
                    if self.database.as_ref().is_some_and(|d| d.id == source) {
                        self.record_table(TableOp::AddProperty(id));
                        self.reload_properties()?;
                    }
                    self.refresh_inline_tables()
                });
            self.report(result);
            self.relation_dialog = None;
        } else if cancel || response.should_close() {
            self.relation_dialog = None;
        } else {
            self.relation_dialog = Some((source, name));
        }
    }
}
