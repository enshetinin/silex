//! A row's page (H6.2b, ADR 0006): the properties of its database under
//! the title, each value edited like its cell in the table.
use std::collections::HashMap;

use anyhow::Result;
use eframe::egui;
use silex_desktop::model::{Page, PageKind};
use silex_desktop::property::{CellValue, Property};

use super::*;
use super::{icons, theme};

/// The open page's properties, when it is a row of a database.
pub(super) struct RowPage {
    pub(super) row: String,
    pub(super) database: String,
    pub(super) properties: Vec<Property>,
    /// Property id → value; empty cells are absent.
    pub(super) values: HashMap<String, CellValue>,
    pub(super) editing: Option<CellEdit>,
    pub(super) option_filter: String,
}

/// Width of the property names on the left.
const LABEL_WIDTH: f32 = 168.0;
/// Height of one property line.
const LINE_HEIGHT: f32 = 34.0;

impl SilexApp {
    /// Reads the properties of `page` if it is a row (a page inside a
    /// database); otherwise there are none to show.
    pub(super) fn load_row_page(&mut self, page: &Page) -> Result<()> {
        self.row_page = None;
        let Some(database) = page
            .parent_id
            .as_deref()
            .filter(|parent| self.is_database(parent))
        else {
            return Ok(());
        };
        self.row_page = Some(RowPage {
            row: page.id.clone(),
            database: database.to_owned(),
            properties: self.db.database_properties(database)?,
            values: self.db.row_values(&page.id)?,
            editing: None,
            option_filter: String::new(),
        });
        Ok(())
    }

    /// Reads the row's properties and values again (a new option).
    pub(super) fn reload_row_page(&mut self) -> Result<()> {
        if let Some(page) = self.row_page.as_mut() {
            page.properties = self.db.database_properties(&page.database)?;
            page.values = self.db.row_values(&page.row)?;
        }
        Ok(())
    }

    /// The row's properties under its title: the name on the left and the
    /// value on the right, edited like the table's cells.
    pub(super) fn row_properties(&mut self, ui: &mut egui::Ui) {
        let Some(mut page) = self.row_page.take() else {
            return;
        };
        if page.properties.is_empty() || self.selected.as_deref() != Some(page.row.as_str()) {
            self.row_page = Some(page);
            return;
        }
        let p = theme::current(ui.ctx());
        let row = Page {
            id: page.row.clone(),
            parent_id: Some(page.database.clone()),
            title: self.title.clone(),
            kind: PageKind::Page,
        };
        let mut actions = Vec::new();
        let RowPage {
            properties,
            values,
            editing,
            option_filter,
            ..
        } = &mut page;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for property in properties.iter() {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), LINE_HEIGHT),
                    egui::Sense::hover(),
                );
                let label =
                    egui::Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, rect.height()));
                let value =
                    egui::Rect::from_min_max(egui::pos2(label.right(), rect.top()), rect.max);
                property_label(ui, label, property);
                if ui.rect_contains_pointer(value) {
                    ui.painter().rect_filled(
                        value,
                        egui::CornerRadius::same(theme::radius::SM),
                        p.subtle,
                    );
                }
                let cell = Cell {
                    rect: value,
                    row: &row,
                    property,
                    value: values.get(&property.id),
                    wanted: false,
                    place: CellPlace::Page,
                };
                property_cell(ui, &cell, editing, option_filter, &mut actions);
            }
        });
        // A line between the properties and the blocks.
        ui.add_space(theme::space::SM);
        let (line, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter()
            .hline(line.x_range(), line.center().y, (1.0, p.border));
        self.row_page = Some(page);
        self.apply_cell_actions(actions);
    }
}

/// A property's icon and name, muted, where its line starts.
fn property_label(ui: &egui::Ui, rect: egui::Rect, property: &Property) {
    let p = theme::current(ui.ctx());
    let painter = ui.painter();
    let icon = painter.text(
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        property_icon(property.kind),
        icons::font(theme::icon::MD),
        p.text_muted,
    );
    let text = egui::Rect::from_min_max(
        egui::pos2(icon.right() + theme::space::SM, rect.top()),
        egui::pos2(rect.right() - theme::space::SM, rect.bottom()),
    );
    let galley = egui::WidgetText::from(property.name.as_str()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        text.width().max(0.0),
        egui::TextStyle::Body,
    );
    let at = egui::Align2::LEFT_CENTER
        .align_size_within_rect(galley.size(), text)
        .min;
    painter.galley(at, galley, p.text_muted);
}
