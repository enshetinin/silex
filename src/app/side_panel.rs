//! The page beside (H8.2, ADR 0011): another page, or a database's
//! table, shown read-only on the right while the open page is edited.
//! «Editar aquí» moves it to the active tab.
use anyhow::Result;
use eframe::egui;
use silex_desktop::attach_job::Request;
use silex_desktop::attachment::Attachment;
use silex_desktop::model::{Block, Page, PageKind};
use silex_desktop::property::{Property, TableValues};

use super::*;
use super::{icons, theme};

/// Narrowest the edited page gets with the panel beside it.
pub(super) const MIN_PAGE_WIDTH: f32 = 480.0;
/// Narrowest the panel gets.
pub(super) const MIN_SIDE_WIDTH: f32 = 300.0;
/// Height of a row of a table beside.
const TABLE_ROW: f32 = 28.0;
/// Width of a column of a table beside.
const TABLE_COLUMN: f32 = 150.0;

/// The page beside and what it shows.
pub(super) struct SidePanel {
    pub(super) page: String,
    pub(super) content: SideContent,
}

/// What the panel shows, as read from the database.
pub(super) enum SideContent {
    Page {
        title: String,
        blocks: Vec<Block>,
    },
    Database {
        title: String,
        properties: Vec<Property>,
        rows: Vec<Page>,
        values: TableValues,
    },
    /// In the trash, or gone.
    Gone,
}

/// What a click in the panel asks for.
enum SideAction {
    Close,
    EditHere,
    Open(String),
    OpenFile(String),
}

impl SilexApp {
    /// Shows `id` beside the open page («Abrir al lado», Alt+click). In a
    /// window too narrow for both, says so instead.
    pub(super) fn open_beside(&mut self, id: &str) -> Result<()> {
        if self.main_width < MIN_PAGE_WIDTH + MIN_SIDE_WIDTH {
            self.notice = Some(
                "La ventana es demasiado estrecha para ver dos páginas. Agrándala, o abre la \
                 página en una pestaña nueva (Cmd/Ctrl+clic)."
                    .into(),
            );
            return Ok(());
        }
        let content = self.load_side(id)?;
        self.side = Some(SidePanel {
            page: id.to_owned(),
            content,
        });
        Ok(())
    }

    /// Reads what the panel shows for `id`.
    fn load_side(&self, id: &str) -> Result<SideContent> {
        let Some(page) = self.db.get_page(id)? else {
            return Ok(SideContent::Gone);
        };
        Ok(match page.kind {
            PageKind::Page => SideContent::Page {
                title: page.title,
                blocks: self.db.list_blocks(id)?,
            },
            PageKind::Database => SideContent::Database {
                title: page.title,
                properties: self.db.database_properties(id)?,
                rows: self.db.database_rows(id)?,
                values: self.db.database_values(id)?,
            },
        })
    }

    /// Reads the page beside again (after a save, or another page or tab
    /// shown), so it never shows stale content for long.
    pub(super) fn refresh_side(&mut self) {
        let Some(page) = self.side.as_ref().map(|s| s.page.clone()) else {
            return;
        };
        match self.load_side(&page) {
            Ok(content) => {
                if let Some(side) = &mut self.side {
                    side.content = content;
                }
            }
            Err(err) => self.notice = Some(format!("{err:#}")),
        }
    }

    /// Opens a page as a click in the page asks: Alt beside it,
    /// Cmd/Ctrl in a new tab, else in the active tab.
    pub(super) fn open_clicked(&mut self, id: String) -> Result<()> {
        let modifiers = self.click_modifiers;
        if modifiers.alt {
            self.open_beside(&id)
        } else {
            self.open_page_in(id, modifiers.command)
        }
    }

    /// The panel on the right, when there is a page beside and room for
    /// both. Its edge can be dragged.
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui) {
        if self.side.is_none() || self.main_width < MIN_PAGE_WIDTH + MIN_SIDE_WIDTH {
            return;
        }
        let p = theme::current(ui.ctx());
        let max = (self.main_width - MIN_PAGE_WIDTH).max(MIN_SIDE_WIDTH);
        let mut action = None;
        egui::Panel::right("silex_side_panel")
            .resizable(true)
            .default_size(380.0)
            .size_range(MIN_SIDE_WIDTH..=max)
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(16, 14)),
            )
            .show(ui, |ui| action = self.side_contents(ui));
        let result = match action {
            Some(SideAction::Close) => {
                self.side = None;
                Ok(())
            }
            Some(SideAction::EditHere) => match self.side.take() {
                Some(side) => self.open_page(side.page),
                None => Ok(()),
            },
            Some(SideAction::Open(id)) => self.open_beside(&id),
            Some(SideAction::OpenFile(file)) => self.open_side_file(ui.ctx(), &file),
            None => Ok(()),
        };
        self.report(result);
    }

    /// Header (icon, «Solo lectura», «Editar aquí», «×») and the page or
    /// table, scrolled on its own.
    fn side_contents(&self, ui: &mut egui::Ui) -> Option<SideAction> {
        let side = self.side.as_ref()?;
        let p = theme::current(ui.ctx());
        let mut action = None;
        let gone = matches!(side.content, SideContent::Gone);
        ui.horizontal(|ui| {
            let icon = if matches!(side.content, SideContent::Database { .. }) {
                icons::TABLE
            } else {
                icons::PAGE
            };
            icons::decoration(ui, icon, theme::icon::MD, p.text_muted);
            ui.label(
                egui::RichText::new("Solo lectura")
                    .small()
                    .color(p.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(icons::icon(icons::CLOSE)).frame_when_inactive(false))
                    .on_hover_text("Cerrar el panel")
                    .accessible_name("Cerrar el panel lateral")
                    .clicked()
                {
                    action = Some(SideAction::Close);
                }
                if !gone
                    && ui
                        .button("Editar aquí")
                        .on_hover_text("Abrirla en la pestaña activa para editarla")
                        .clicked()
                {
                    action = Some(SideAction::EditHere);
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("silex_side_scroll")
            .auto_shrink(false)
            .show(ui, |ui| match &side.content {
                SideContent::Page { title, blocks } => {
                    let pages: Vec<Page> = self
                        .pages
                        .iter()
                        .chain(&self.templates.list)
                        .cloned()
                        .collect();
                    match read_only_page(ui, title, blocks, &pages, true, "Esta página está vacía.")
                    {
                        Some(ReadOnlyAction::OpenPage(id)) => action = Some(SideAction::Open(id)),
                        Some(ReadOnlyAction::OpenFile(file)) => {
                            action = Some(SideAction::OpenFile(file));
                        }
                        None => {}
                    }
                }
                SideContent::Database {
                    title,
                    properties,
                    rows,
                    values,
                } => {
                    if let Some(row) = side_table(ui, title, properties, rows, values) {
                        action = Some(SideAction::Open(row));
                    }
                }
                SideContent::Gone => {
                    ui.add_space(theme::space::LG);
                    ui.label(
                        egui::RichText::new("Esta página está en la papelera o ya no existe.")
                            .color(p.text_muted),
                    );
                }
            });
        action
    }

    /// Opens an attachment of the page beside, as its card would: a
    /// program or script is only shown in its folder.
    fn open_side_file(&mut self, ctx: &egui::Context, file: &str) -> Result<()> {
        let request = match self.db.attachment(file)? {
            Some(attachment) if Attachment::is_executable(&attachment) => {
                Request::Reveal(file.to_owned())
            }
            Some(_) => Request::Open(file.to_owned()),
            None => anyhow::bail!("El archivo ya no está"),
        };
        self.file_request(ctx, request)
    }
}

/// A database's table, read-only: «Nombre» and its properties, one line
/// per row (only the rows in sight are drawn). A row's name opens it
/// beside. Returns the row clicked.
fn side_table(
    ui: &mut egui::Ui,
    title: &str,
    properties: &[Property],
    rows: &[Page],
    values: &TableValues,
) -> Option<String> {
    let p = theme::current(ui.ctx());
    ui.label(
        egui::RichText::new(display_title(title))
            .text_style(egui::TextStyle::Name(theme::text::TITLE.into()))
            .color(p.text),
    );
    ui.add_space(theme::space::SM);
    if rows.is_empty() {
        ui.label(egui::RichText::new("Esta base de datos no tiene filas.").color(p.text_muted));
        return None;
    }
    let mut opened = None;
    egui::ScrollArea::horizontal()
        .id_salt("silex_side_table")
        .show(ui, |ui| {
            let cell = |ui: &mut egui::Ui, text: &str, strong: bool| {
                let mut rich =
                    egui::RichText::new(text).color(if strong { p.text_muted } else { p.text });
                if strong {
                    rich = rich.small().strong();
                }
                ui.add_sized([TABLE_COLUMN, TABLE_ROW], egui::Label::new(rich).truncate());
            };
            ui.horizontal(|ui| {
                cell(ui, "Nombre", true);
                for property in properties {
                    cell(ui, &property.name, true);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("silex_side_rows")
                .auto_shrink([false, true])
                .show_rows(ui, TABLE_ROW, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        ui.horizontal(|ui| {
                            let name = display_title(&row.title);
                            if ui
                                .add_sized(
                                    [TABLE_COLUMN, TABLE_ROW],
                                    egui::Link::new(egui::RichText::new(name).color(p.text)),
                                )
                                .clicked()
                            {
                                opened = Some(row.id.clone());
                            }
                            for property in properties {
                                let text = values
                                    .get(&row.id)
                                    .and_then(|cells| cells.get(&property.id))
                                    .map(|value| value.display(property))
                                    .unwrap_or_default();
                                cell(ui, &text, false);
                            }
                        });
                    }
                });
        });
    opened
}
