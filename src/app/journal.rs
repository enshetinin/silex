//! Daily notes (H12.9): «Hoy» in the sidebar, Cmd+Mayús+D and the palette
//! open today's note in «Diario» › month › day, made if missing from the
//! template chosen in Ajustes.
use anyhow::Result;
use eframe::egui;

use super::*;

impl SilexApp {
    /// Opens today's note (local date), making it (and «Diario», and the
    /// month) if missing.
    pub(super) fn open_today(&mut self) -> Result<()> {
        self.save_now()?;
        let date = self.db.local_today()?;
        let (root, template) = self.db.journal_settings()?;
        let day = self
            .db
            .journal_day(root.as_deref(), &date, template.as_deref())?;
        if root.as_deref() != Some(day.root.as_str()) {
            self.db.set_journal_page(&day.root)?;
        }
        self.reload_pages()?;
        // Where it is, in sight in the sidebar.
        self.expanded.insert(day.root.clone());
        if let Some(month) = self
            .pages
            .iter()
            .find(|p| p.id == day.page)
            .and_then(|p| p.parent_id.clone())
        {
            self.expanded.insert(month);
        }
        self.rebuild_tree();
        self.open_page(day.page)
    }

    /// «Plantilla del diario» in Ajustes: none, or one of the templates.
    pub(super) fn journal_template_menu(&mut self, ui: &mut egui::Ui) {
        let chosen = self.db.journal_settings().ok().and_then(|(_, t)| t);
        let name = chosen
            .as_ref()
            .and_then(|id| self.templates.list.iter().find(|t| &t.id == id))
            .map_or("Ninguna".to_owned(), |t| short_title(&t.title));
        let mut pick: Option<Option<String>> = None;
        ui.menu_button(format!("Plantilla del diario: {name}"), |ui| {
            if ui
                .selectable_label(chosen.is_none(), "Ninguna (página vacía)")
                .clicked()
            {
                pick = Some(None);
                ui.close();
            }
            for template in &self.templates.list {
                let selected = chosen.as_deref() == Some(template.id.as_str());
                if ui
                    .selectable_label(selected, short_title(&template.title))
                    .clicked()
                {
                    pick = Some(Some(template.id.clone()));
                    ui.close();
                }
            }
            if self.templates.list.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Guarda una página como plantilla («…» → «Guardar como plantilla») \
                         para usarla aquí.",
                    )
                    .small()
                    .weak(),
                );
            }
        });
        if let Some(template) = pick {
            let result = self.db.set_journal_template(template.as_deref());
            self.report(result);
        }
    }
}
