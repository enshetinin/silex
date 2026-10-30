//! Page templates in the window (H7.11, ADR 0010): the «Plantillas»
//! section, a template's menu, «Guardar como plantilla» and «Nueva página
//! desde plantilla…» with its picker.
use anyhow::Result;
use eframe::egui;
use silex_desktop::db::Shift;
use silex_desktop::model::{Page, PageKind, TreeRow};

use super::*;
use super::{icons, theme};

/// Templates and their part of the window.
#[derive(Default)]
pub(super) struct Templates {
    /// Live templates, in their order.
    pub(super) list: Vec<Page>,
    /// The «Plantillas» section is unfolded (this session only).
    pub(super) open: bool,
    pub(super) picker: Option<TemplatePicker>,
}

/// «Nueva página desde plantilla…»: what is typed, the template
/// highlighted, and where the new page goes.
#[derive(Debug, Clone, Default)]
pub(super) struct TemplatePicker {
    pub(super) query: String,
    pub(super) selected: usize,
    /// The new page goes inside this page (else at the root).
    pub(super) parent: Option<String>,
    /// Focus its field on the next frame.
    opening: bool,
}

/// The section title, with a caret: a click folds or unfolds it.
fn section_toggle(ui: &mut egui::Ui, title: &str, open: bool) -> egui::Response {
    ui.add_space(theme::space::XS);
    // Drawn like the other section titles (plain labels), so they line
    // up; the whole line answers to a click.
    let row = ui.horizontal(|ui| {
        ui.add_space(theme::space::SM);
        let caret = if open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        };
        ui.add(egui::Label::new(egui::RichText::new(title).small().weak()).selectable(false));
        let weak = ui.visuals().weak_text_color();
        icons::decoration(ui, caret, 12.0, weak);
    });
    let response = ui
        .interact(
            row.response.rect,
            ui.scope_id().with(("silex_section_toggle", title)),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if open { "Plegar" } else { "Desplegar" });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    // Screen readers say whether the section is folded.
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_expanded(open));
    focus_ring(ui, row.response.rect, &response);
    ui.add_space(2.0);
    response
}

impl SilexApp {
    /// The «Plantillas» section, folded at first. Unfolding it the first
    /// time creates the example templates (ADR 0010).
    pub(super) fn template_rows(&mut self, ui: &mut egui::Ui) {
        if section_toggle(ui, "Plantillas", self.templates.open).clicked() {
            self.templates.open = !self.templates.open;
            if self.templates.open {
                let result = self.ensure_example_templates();
                self.report(result);
            }
        }
        if self.templates.open {
            if self.templates.list.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(theme::space::SM);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Guarda una página como plantilla desde su menú «…».",
                            )
                            .small()
                            .weak(),
                        )
                        .wrap(),
                    );
                });
            }
            let rows: Vec<TreeRow> = self
                .templates
                .list
                .iter()
                .map(|page| TreeRow {
                    page: page.clone(),
                    depth: 0,
                    has_children: false,
                })
                .collect();
            self.page_rows(ui, &rows, false, "tpl", icons::COPY);
        }
        ui.add_space(theme::space::MD);
    }

    /// Creates the example templates the first time (ADR 0010).
    fn ensure_example_templates(&mut self) -> Result<()> {
        if self.db.create_example_templates()? {
            self.templates.list = self.db.list_templates()?;
        }
        Ok(())
    }

    /// The menu of a template (its row, or the «…» of the open one).
    pub(super) fn template_menu(&self, ui: &mut egui::Ui, id: &str) -> Option<RowAction> {
        let index = self.templates.list.iter().position(|t| t.id == id)?;
        let owned = || id.to_owned();
        let mut action = None;
        if ui.button("Abrir").clicked() {
            action = Some(RowAction::Open(owned()));
        }
        if ui.button("Abrir en una pestaña nueva").clicked() {
            action = Some(RowAction::OpenInNewTab(owned()));
        }
        if ui.button("Abrir al lado").clicked() {
            action = Some(RowAction::OpenBeside(owned()));
        }
        if ui.button("Usar esta plantilla").clicked() {
            action = Some(RowAction::UseTemplate(owned(), None));
        }
        if ui.button("Renombrar").clicked() {
            action = Some(RowAction::StartRename(owned()));
        }
        ui.separator();
        if ui
            .add_enabled(index > 0, egui::Button::new("Subir"))
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Up));
        }
        if ui
            .add_enabled(
                index + 1 < self.templates.list.len(),
                egui::Button::new("Bajar"),
            )
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Down));
        }
        ui.separator();
        if ui.button("Enviar a la papelera").clicked() {
            action = Some(RowAction::Trash(owned()));
        }
        action
    }

    /// Whether `id` can be saved as a template: a page, not a database, a
    /// row of one, or a template (ADR 0010).
    pub(super) fn can_save_as_template(&self, id: &str) -> bool {
        let Some(page) = self.pages.iter().find(|p| p.id == id) else {
            return false;
        };
        page.kind == PageKind::Page
            && !page
                .parent_id
                .as_deref()
                .is_some_and(|parent| self.is_database(parent))
    }

    /// «Guardar como plantilla»: a copy of the page in «Plantillas»; the
    /// page stays open as it was.
    pub(super) fn save_as_template(&mut self, id: &str) -> Result<()> {
        self.save_now()?;
        self.db.save_as_template(id)?;
        self.reload_pages()?;
        self.templates.open = true;
        let title = self
            .find_page(id)
            .map(|p| display_title(&p.title).to_owned())
            .unwrap_or_default();
        self.notice = Some(format!(
            "«{title}» se guardó como plantilla, en «Plantillas»."
        ));
        Ok(())
    }

    /// A new page from a template, inside `parent` (or at the root),
    /// opened with its title ready to edit.
    pub(super) fn use_template(&mut self, template: &str, parent: Option<&str>) -> Result<()> {
        self.save_now()?;
        let today = self.db.local_today()?;
        let id = self.db.create_from_template(template, parent, &today)?;
        if let Some(parent) = parent {
            self.expanded.insert(parent.to_owned());
        }
        self.reload_pages()?;
        self.open_page(id)?;
        self.focus_title = true;
        Ok(())
    }

    /// Opens «Nueva página desde plantilla…»; the page goes inside
    /// `parent`, or at the root.
    pub(super) fn open_template_picker(&mut self, parent: Option<String>) {
        let result = self.ensure_example_templates();
        self.report(result);
        self.slash = None;
        self.templates.picker = Some(TemplatePicker {
            parent,
            opening: true,
            ..TemplatePicker::default()
        });
    }

    /// The template picker, a modal window: typing filters by title, ↑ ↓
    /// choose, Intro creates the page, Esc closes.
    pub(super) fn template_picker(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.templates.picker.take() else {
            return;
        };
        let p = theme::current(ctx);
        let matches: Vec<Page> = picker_matches(&self.templates.list, &picker.query, None)
            .into_iter()
            .cloned()
            .collect();
        use egui::{Key, Modifiers as M};
        let last = matches.len().saturating_sub(1);
        if take_exact_key(ctx, Key::ArrowDown, M::NONE) {
            picker.selected = (picker.selected + 1).min(last);
        }
        if take_exact_key(ctx, Key::ArrowUp, M::NONE) {
            picker.selected = picker.selected.saturating_sub(1);
        }
        picker.selected = picker.selected.min(last);
        let mut chosen = take_exact_key(ctx, Key::Enter, M::NONE)
            .then(|| matches.get(picker.selected).cloned())
            .flatten();
        let mut close = take_exact_key(ctx, Key::Escape, M::NONE);
        let where_to = picker
            .parent
            .as_deref()
            .and_then(|id| self.find_page(id))
            .map(|parent| format!("Se creará dentro de «{}».", short_title(&parent.title)));
        let response =
            egui::Modal::new(egui::Id::unique("silex_template_picker")).show(ctx, |ui| {
                ui.set_width(460.0);
                ui.heading("Nueva página desde plantilla");
                if let Some(where_to) = &where_to {
                    ui.label(egui::RichText::new(where_to).color(p.text_muted));
                }
                ui.add_space(theme::space::SM);
                let field = framed(
                    ui,
                    egui::TextEdit::singleline(&mut picker.query)
                        .hint_text(
                            egui::RichText::new("Busca una plantilla por su título")
                                .color(p.text_faint),
                        )
                        .desired_width(f32::INFINITY),
                )
                .accessible_name("Plantilla");
                if std::mem::take(&mut picker.opening) {
                    field.request_focus();
                }
                if field.changed() {
                    picker.selected = 0;
                }
                ui.add_space(theme::space::SM);
                if matches.is_empty() {
                    let text = if self.templates.list.is_empty() {
                        "No hay plantillas. Guarda una página como plantilla desde su menú «…»."
                    } else {
                        "Ninguna plantilla tiene ese título."
                    };
                    ui.label(egui::RichText::new(text).color(p.text_muted));
                }
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (i, template) in matches.iter().enumerate() {
                            let row = template_row(ui, template, i == picker.selected);
                            if i == picker.selected {
                                row.scroll_to_me(None);
                            }
                            if row.clicked() {
                                chosen = Some(template.clone());
                            }
                        }
                    });
                ui.add_space(theme::space::SM);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        close = true;
                    }
                    ui.label(
                        egui::RichText::new("↑ ↓ para elegir · Intro para crear · Esc para cerrar")
                            .small()
                            .color(p.text_muted),
                    );
                });
            });
        if let Some(template) = chosen {
            let result = self.use_template(&template.id, picker.parent.as_deref());
            self.report(result);
            return;
        }
        if !close && !response.should_close() {
            self.templates.picker = Some(picker);
        }
    }
}

/// One template in the picker: its icon and title, the whole width.
fn template_row(ui: &mut egui::Ui, template: &Page, selected: bool) -> egui::Response {
    let p = theme::current(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    let title = display_title(&template.title);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, title));
    let radius = egui::CornerRadius::same(theme::radius::SM);
    if selected {
        ui.painter().rect_filled(rect, radius, p.accent_soft);
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, radius, p.subtle);
    }
    focus_ring(ui, rect, &response);
    let at = ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icons::COPY,
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        p.text,
    );
    let left = at.right() + theme::space::SM;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left - 8.0).max(0.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    response
}
