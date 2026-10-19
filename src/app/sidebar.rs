//! Sidebar: page tree, favorites, search button, page menu and rename.
use anyhow::Result;
use eframe::egui::{self, TextEdit};
use silex_desktop::{
    db::Shift,
    model::{TreeRow, descendants},
};

use super::*;
use super::{icons, theme};
use silex_desktop::settings::{ThemeMode, ZOOM_STEPS};

/// Inline rename in the sidebar.
pub(super) struct Rename {
    pub(super) id: String,
    pub(super) text: String,
    /// Focus the text field on the next frame it is drawn.
    pub(super) focus: bool,
}

/// Something the user asked for on a page row (click, menu, keyboard).
pub(super) enum RowAction {
    Open(String),
    Toggle(String),
    StartRename(String),
    CommitRename,
    CancelRename,
    Subpage(String),
    Favorite(String, bool),
    Reorder(String, Shift),
    Move(String, Option<String>),
    Trash(String),
}

/// Height of every sidebar row.
const ROW_HEIGHT: f32 = 28.0;
/// Width of the caret slot before a row's icon.
const CARET_SLOT: f32 = 22.0;
/// Indent per tree level: one caret slot, so a child's caret sits under
/// its parent's icon.
const ROW_INDENT: f32 = CARET_SLOT;

/// Dark sidebar styling for this `Ui` (popups opened from it keep the
/// normal theme, since they live on their own layer).
fn sidebar_style(ui: &mut egui::Ui, p: &theme::Palette) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(p.sidebar_text);
    v.weak_text_color = Some(p.sidebar_muted);
    v.extreme_bg_color = p.sidebar_hover;
    v.text_edit_bg_color = Some(p.sidebar_hover);
    v.selection.bg_fill = p.sidebar_selected;
    v.widgets.noninteractive.fg_stroke.color = p.sidebar_text;
    v.widgets.noninteractive.bg_stroke.color = p.sidebar_hover;
    for (state, fill) in [
        (&mut v.widgets.inactive, p.sidebar_hover),
        (&mut v.widgets.hovered, p.sidebar_selected),
        (&mut v.widgets.active, p.sidebar_selected),
        (&mut v.widgets.open, p.sidebar_selected),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = egui::Stroke::NONE;
        state.fg_stroke.color = p.sidebar_text;
    }
}

/// Small muted section title.
/// Indented like row content, so it lines up with the rows' first icon.
fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(theme::space::XS);
    ui.horizontal(|ui| {
        ui.add_space(theme::space::SM);
        ui.label(egui::RichText::new(title).small().weak());
    });
    ui.add_space(2.0);
}

/// Section title with a "+" at the right; returns the "+" button.
fn section_header_with_add(ui: &mut egui::Ui, title: &str, tooltip: &str) -> egui::Response {
    ui.add_space(theme::space::XS);
    let button = ui
        .horizontal(|ui| {
            ui.add_space(theme::space::SM);
            ui.label(egui::RichText::new(title).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Button::new(icons::icon(icons::PLUS).weak()).frame_when_inactive(false),
                )
                .on_hover_text(tooltip)
                .accessible_name(tooltip)
            })
            .inner
        })
        .inner;
    ui.add_space(2.0);
    button
}

/// What a sidebar row shows.
struct RowParts<'a> {
    depth: usize,
    /// `Some(expanded)` when the row has children to show or hide.
    caret: Option<bool>,
    /// Reserve room for a caret (tree rows) so titles line up.
    caret_slot: bool,
    icon: &'a str,
    label: &'a str,
    /// Muted text at the right (e.g. a count).
    trailing: Option<String>,
    selected: bool,
    /// Show "+" and "…" while hovered.
    hover_actions: bool,
}

impl<'a> RowParts<'a> {
    /// A plain navigation row (footer).
    fn nav(icon: &'a str, label: &'a str, trailing: Option<String>) -> Self {
        Self {
            depth: 0,
            caret: None,
            caret_slot: false,
            icon,
            label,
            trailing,
            selected: false,
            hover_actions: false,
        }
    }
}

/// Responses of the interactive parts of a row.
struct RowOutput {
    row: egui::Response,
    caret: Option<egui::Response>,
    plus: Option<egui::Response>,
    more: Option<egui::Response>,
    /// Where the title is drawn (used by the inline rename field).
    label_rect: egui::Rect,
}

/// One sidebar row: full-width clickable area with hover and selected
/// backgrounds, laid out by hand so every part stays aligned.
fn sidebar_row(ui: &mut egui::Ui, parts: &RowParts<'_>) -> RowOutput {
    let p = theme::current(ui.ctx());
    let interact_height = ui.spacing().interact_size.y;
    let (rect, row) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    row.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, parts.selected, parts.label)
    });
    let hovered = ui.rect_contains_pointer(rect);
    // The "…" menu is open: keep the row's actions (the pointer leaves the
    // row to reach the menu, and the menu only lives while its button is
    // drawn) and its highlight. Same id `Popup::menu` derives from the button.
    let menu_open = egui::Popup::is_id_open(ui.ctx(), row.id.with(icons::DOTS_THREE).with("popup"));
    let fill = if parts.selected {
        p.sidebar_selected
    } else if hovered || row.has_focus() || menu_open {
        p.sidebar_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(theme::radius::MD), fill);
    if parts.selected {
        // Selected page: an accent bar along the left edge, following the
        // row's rounded corners.
        let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
        let r = theme::radius::MD;
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            p.sidebar_selected_text,
        );
    }
    if row.has_focus() {
        // Keyboard focus: an outline, distinct from hover and selection.
        ui.painter().rect_stroke(
            rect.shrink(0.5),
            egui::CornerRadius::same(theme::radius::MD),
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }

    let cy = rect.center().y;
    let mut x = rect.left() + theme::space::SM + parts.depth as f32 * ROW_INDENT;
    // Icon buttons must fit inside the row: the theme's minimum interact
    // height (28 px) would make rows with buttons taller than the others.
    ui.spacing_mut().interact_size.y = 24.0;
    // Interact + paint instead of `put`: `put` reserves the button's real
    // size (wider than 24 px), which made the panel grow a little every frame.
    let row_id = row.id;
    let icon_button = |ui: &mut egui::Ui, center: egui::Pos2, icon: &str, name: &str| {
        let rect = egui::Rect::from_center_size(center, egui::vec2(24.0, 24.0));
        let response = ui.interact(rect, row_id.with(icon), egui::Sense::click());
        // Painted by hand, so name it for screen readers and keyboard users.
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let active = response.hovered() || response.has_focus();
        if active {
            ui.painter()
                .rect_filled(rect, theme::radius::SM, p.sidebar_selected);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            icons::font(theme::icon::MD),
            if active {
                p.sidebar_text
            } else {
                p.sidebar_muted
            },
        );
        response.on_hover_text(name)
    };
    let mut caret = None;
    if parts.caret_slot {
        if let Some(expanded) = parts.caret {
            let icon = if expanded {
                icons::CARET_DOWN
            } else {
                icons::CARET_RIGHT
            };
            let name = if expanded {
                "Ocultar subpáginas"
            } else {
                "Mostrar subpáginas"
            };
            let response = icon_button(ui, egui::pos2(x + 10.0, cy), icon, name);
            caret = Some(response);
        }
        x += CARET_SLOT;
    }
    ui.painter().text(
        egui::pos2(x, cy),
        egui::Align2::LEFT_CENTER,
        parts.icon,
        icons::font(theme::icon::MD),
        if parts.selected {
            p.sidebar_selected_text
        } else {
            p.sidebar_muted
        },
    );
    x += 26.0;

    let mut right = rect.right() - theme::space::XS;
    let (mut plus, mut more) = (None, None);
    // Also while the row has keyboard focus, so Tab reaches its actions.
    let focus_inside = row.has_focus()
        || [icons::DOTS_THREE, icons::PLUS]
            .iter()
            .any(|icon| ui.memory(|m| m.has_focus(row_id.with(*icon))));
    if parts.hover_actions && (hovered || focus_inside || menu_open) {
        more = Some(icon_button(
            ui,
            egui::pos2(right - 12.0, cy),
            icons::DOTS_THREE,
            "Más acciones",
        ));
        right -= 26.0;
        plus = Some(icon_button(
            ui,
            egui::pos2(right - 12.0, cy),
            icons::PLUS,
            "Nueva subpágina",
        ));
        right -= 26.0;
    }
    if let Some(trailing) = &parts.trailing {
        let galley = ui.painter().layout_no_wrap(
            trailing.clone(),
            egui::FontId::proportional(12.0),
            p.sidebar_muted,
        );
        right -= theme::space::XS;
        ui.painter().galley(
            egui::pos2(right - galley.size().x, cy - galley.size().y / 2.0),
            galley.clone(),
            p.sidebar_muted,
        );
        right -= galley.size().x + theme::space::SM;
    }
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(x, rect.top() + 2.0),
        egui::pos2(right.max(x), rect.bottom() - 2.0),
    );
    // Left-aligned (`put` would center the label in the rect).
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(parts.label).color(if parts.selected {
                    p.sidebar_selected_text
                } else {
                    p.sidebar_text
                }))
                .truncate()
                .selectable(false),
            );
        },
    );
    ui.spacing_mut().interact_size.y = interact_height;
    RowOutput {
        row,
        caret,
        plus,
        more,
        label_rect,
    }
}

impl SilexApp {
    /// Favorites section; hidden when there are none.
    pub(super) fn favorite_rows(&mut self, ui: &mut egui::Ui) {
        if self.favorites.is_empty() {
            return;
        }
        section_header(ui, "Favoritos");
        let rows: Vec<TreeRow> = self
            .favorites
            .iter()
            .map(|page| TreeRow {
                page: page.clone(),
                depth: 0,
                has_children: false,
            })
            .collect();
        self.page_rows(ui, &rows, false, "fav", icons::STAR);
        ui.add_space(theme::space::MD);
    }

    /// Page rows (tree, favorites or search): indentation, optional expand
    /// arrow, icon, title (or the inline rename field), and on hover "+"
    /// (new subpage) and "…" (page menu). `section` keeps widget ids
    /// distinct when a page appears twice.
    pub(super) fn page_rows(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[TreeRow],
        arrows: bool,
        section: &str,
        icon: &str,
    ) {
        let mut actions = Vec::new();
        for row in rows {
            ui.push_id((section, &row.page.id), |ui| {
                actions.extend(self.page_row(ui, row, arrows, section, icon));
            });
        }
        for action in actions {
            let result = self.apply(action);
            self.report(result);
        }
    }

    fn page_row(
        &mut self,
        ui: &mut egui::Ui,
        row: &TreeRow,
        arrows: bool,
        section: &str,
        icon: &str,
    ) -> Vec<RowAction> {
        let id = row.page.id.clone();
        let mut actions = Vec::new();
        let renaming = self.rename.as_ref().is_some_and(|r| r.id == id);
        let is_database = row.page.kind == PageKind::Database;
        let icon = if is_database && icon == icons::PAGE {
            icons::TABLE
        } else {
            icon
        };
        let parts = RowParts {
            depth: row.depth,
            caret: (arrows && row.has_children).then(|| self.expanded.contains(&id)),
            caret_slot: arrows,
            icon,
            label: display_title(&row.page.title),
            trailing: None,
            selected: self.selected.as_deref() == Some(id.as_str()),
            hover_actions: !renaming,
        };
        let mut out = sidebar_row(ui, &parts);
        if let Some(caret) = &out.caret
            && caret.clicked()
        {
            actions.push(RowAction::Toggle(id.clone()));
        }
        let plus_tip = if is_database {
            "Nueva fila"
        } else {
            "Nueva subpágina"
        };
        if let Some(plus) = out.plus.take()
            && plus.on_hover_text(plus_tip).clicked()
        {
            actions.push(RowAction::Subpage(id.clone()));
        }
        if let Some(more) = &out.more {
            egui::Popup::menu(more).show(|ui| {
                if let Some(chosen) = self.page_menu(ui, &id) {
                    actions.push(chosen);
                    ui.close();
                }
            });
        }
        if renaming {
            if let Some(action) = self.rename_field(ui, out.label_rect, section, &id) {
                actions.push(action);
            }
            return actions;
        }
        if out.row.double_clicked() {
            actions.push(RowAction::StartRename(id.clone()));
        } else if out.row.clicked() {
            actions.push(RowAction::Open(id.clone()));
        }
        out.row.context_menu(|ui| {
            if let Some(chosen) = self.page_menu(ui, &id) {
                actions.push(chosen);
                ui.close();
            }
        });
        actions
    }

    /// Inline rename field drawn over the row's title.
    fn rename_field(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        section: &str,
        id: &str,
    ) -> Option<RowAction> {
        let rename = self.rename.as_mut()?;
        let response = ui.put(
            rect,
            TextEdit::singleline(&mut rename.text)
                .id(egui::Id::unique(("silex-rename", section, id)))
                .desired_width(rect.width()),
        );
        if rename.focus {
            response.request_focus();
            rename.focus = false;
        }
        if response.lost_focus() {
            return Some(if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                RowAction::CancelRename
            } else {
                RowAction::CommitRename
            });
        }
        None
    }

    /// Context menu for a page.
    pub(super) fn page_menu(&self, ui: &mut egui::Ui, id: &str) -> Option<RowAction> {
        let page = self.pages.iter().find(|p| p.id == id)?;
        let siblings: Vec<&str> = self
            .pages
            .iter()
            .filter(|p| p.parent_id == page.parent_id)
            .map(|p| p.id.as_str())
            .collect();
        let index = siblings.iter().position(|s| *s == id);
        let is_favorite = self.favorites.iter().any(|p| p.id == id);
        let owned = || id.to_owned();
        let mut action = None;
        if ui.button("Abrir").clicked() {
            action = Some(RowAction::Open(owned()));
        }
        if ui.button("Renombrar").clicked() {
            action = Some(RowAction::StartRename(owned()));
        }
        let child = if page.kind == PageKind::Database {
            "Nueva fila"
        } else {
            "Nueva subpágina"
        };
        if ui.button(child).clicked() {
            action = Some(RowAction::Subpage(owned()));
        }
        let favorite_label = if is_favorite {
            "Quitar de favoritos"
        } else {
            "Añadir a favoritos"
        };
        if ui.button(favorite_label).clicked() {
            action = Some(RowAction::Favorite(owned(), !is_favorite));
        }
        ui.separator();
        if ui
            .add_enabled(index.is_some_and(|i| i > 0), egui::Button::new("Subir"))
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Up));
        }
        if ui
            .add_enabled(
                index.is_some_and(|i| i + 1 < siblings.len()),
                egui::Button::new("Bajar"),
            )
            .clicked()
        {
            action = Some(RowAction::Reorder(owned(), Shift::Down));
        }
        ui.menu_button("Mover a…", |ui| {
            let blocked = descendants(&self.pages, id);
            egui::ScrollArea::vertical()
                .id_salt("silex_menu_move")
                .max_height(240.0)
                .show(ui, |ui| {
                    if page.parent_id.is_some() && ui.button("Raíz").clicked() {
                        action = Some(RowAction::Move(owned(), None));
                    }
                    for target in &self.pages {
                        // A database is no destination (ADR 0006).
                        let valid = target.id != id
                            && target.kind != PageKind::Database
                            && !blocked.contains(&target.id)
                            && page.parent_id.as_deref() != Some(target.id.as_str());
                        if valid && ui.button(display_title(&target.title)).clicked() {
                            action = Some(RowAction::Move(owned(), Some(target.id.clone())));
                        }
                    }
                });
        });
        ui.separator();
        if ui.button("Enviar a la papelera").clicked() {
            action = Some(RowAction::Trash(owned()));
        }
        action
    }

    pub(super) fn apply(&mut self, action: RowAction) -> Result<()> {
        match action {
            RowAction::Open(id) => self.open_page(id),
            RowAction::Toggle(id) => {
                self.toggle_expanded(&id);
                Ok(())
            }
            RowAction::StartRename(id) => {
                self.start_rename(&id);
                Ok(())
            }
            RowAction::CommitRename => self.commit_rename(),
            RowAction::CancelRename => {
                self.rename = None;
                Ok(())
            }
            RowAction::Subpage(id) => self.create_subpage(&id),
            RowAction::Favorite(id, favorite) => self.set_favorite(&id, favorite),
            RowAction::Reorder(id, shift) => self.reorder_page(&id, shift),
            RowAction::Move(id, parent) => self.move_page(&id, parent.as_deref()),
            RowAction::Trash(id) => self.trash_page(&id),
        }
    }

    pub(super) fn start_rename(&mut self, id: &str) {
        let current = if self.selected.as_deref() == Some(id) {
            // The open page may have an unsaved title in the editor.
            Some(self.title.clone())
        } else {
            self.pages
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.title.clone())
        };
        if let Some(text) = current {
            self.rename = Some(Rename {
                id: id.to_owned(),
                text,
                focus: true,
            });
        }
    }

    pub(super) fn commit_rename(&mut self) -> Result<()> {
        let Some(rename) = self.rename.take() else {
            return Ok(());
        };
        let title = match rename.text.trim() {
            "" => "Sin título".to_owned(),
            t => t.to_owned(),
        };
        if self.selected.as_deref() == Some(rename.id.as_str()) {
            // Keep the editor and the database in agreement: save through the document.
            if title != self.title {
                self.title = title;
                self.mark_dirty();
                self.save_now()?;
            }
            Ok(())
        } else {
            self.db.rename_page(&rename.id, &title)?;
            self.reload_pages()
        }
    }

    /// Opens the page above or below the open one in the visible tree.
    pub(super) fn open_adjacent(&mut self, step: isize) -> Result<()> {
        let ids: Vec<&str> = self.tree.iter().map(|r| r.page.id.as_str()).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|sel| ids.iter().position(|id| id == sel));
        let next = match current {
            Some(i) => i.saturating_add_signed(step).min(ids.len() - 1),
            None => 0,
        };
        if Some(next) == current {
            return Ok(());
        }
        let id = ids[next].to_owned();
        self.open_page(id)
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = theme::current(ui.ctx());
        sidebar_style(ui, p);

        ui.label(
            egui::RichText::new("Silex")
                .text_style(egui::TextStyle::Name(theme::text::H3.into()))
                .color(p.sidebar_text),
        );
        ui.add_space(theme::space::MD);
        let new_page = ui.add_sized(
            [ui.available_width(), 32.0],
            egui::Button::new((
                icons::icon(icons::PLUS).color(p.on_accent),
                egui::RichText::new("Nueva página").color(p.on_accent),
            ))
            .fill(p.accent_strong)
            .corner_radius(theme::radius::MD),
        );
        if new_page.clicked() {
            let result = self.create_root_page("Sin título");
            self.report(result);
        }
        ui.add_space(theme::space::SM);
        self.search_button(ui, p);
        ui.add_space(theme::space::MD);

        egui::Panel::bottom("silex_sidebar_footer")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6)))
            .show(ui, |ui| self.sidebar_footer(ui, p));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("silex_sidebar_tree")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        self.favorite_rows(ui);
                        let add = section_header_with_add(ui, "Páginas", "Añadir");
                        egui::Popup::menu(&add).show(|ui| {
                            let page = ui.button((icons::icon(icons::PAGE), " Nueva página"));
                            let base =
                                ui.button((icons::icon(icons::TABLE), " Nueva base de datos"));
                            let result = if page.clicked() {
                                ui.close();
                                self.create_root_page("Sin título")
                            } else if base.clicked() {
                                ui.close();
                                self.create_database(None)
                            } else {
                                Ok(())
                            };
                            self.report(result);
                        });
                        let rows = self.tree.clone();
                        self.page_rows(ui, &rows, true, "tree", icons::PAGE);
                    });
            });
    }

    /// Looks like a search box; opens the search window (also Cmd+K).
    fn search_button(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        let hint = format!("{}+K", cmd_label(ui.ctx()));
        let button = ui.add(
            egui::Button::new((
                icons::icon(icons::SEARCH).color(p.sidebar_muted),
                egui::RichText::new("Buscar").color(p.sidebar_muted),
            ))
            .shortcut_text(egui::RichText::new(hint).small().color(p.sidebar_muted))
            .fill(p.sidebar_hover)
            .corner_radius(theme::radius::MD)
            .min_size(egui::vec2(ui.available_width(), 32.0)),
        );
        if button.clicked() {
            self.open_search(ui.ctx());
        }
    }

    /// Trash, backups/export and shortcuts, pinned to the bottom.
    fn sidebar_footer(&mut self, ui: &mut egui::Ui, p: &theme::Palette) {
        ui.spacing_mut().item_spacing.y = 1.0;
        let trash = sidebar_row(
            ui,
            &RowParts::nav(icons::TRASH, "Papelera", Some(self.trash_count.to_string())),
        );
        if trash.row.clicked() {
            self.open_trash_dialog();
        }

        let backups = sidebar_row(
            ui,
            &RowParts::nav(icons::BACKUP, "Copias y exportación", None),
        );
        egui::Popup::menu(&backups.row).show(|ui| {
            ui.label(egui::RichText::new(&self.backup_label).small().weak());
            ui.separator();
            let can_backup = self.backup_job.is_none() && self.db.path().is_some();
            if ui
                .add_enabled(can_backup, egui::Button::new("Crear copia ahora"))
                .clicked()
            {
                self.start_backup(ui.ctx());
                ui.close();
            }
            if ui
                .add_enabled(can_backup, egui::Button::new("Restaurar una copia…"))
                .clicked()
            {
                self.open_restore_dialog();
                ui.close();
            }
            ui.separator();
            let can_export = self.export_job.is_none() && self.db.path().is_some();
            let label = if self.export_job.is_some() {
                "Exportando…"
            } else {
                "Exportar todo a Markdown"
            };
            if ui
                .add_enabled(can_export, egui::Button::new(label))
                .on_hover_text("Todas las páginas (sin la papelera) en una carpeta nueva")
                .clicked()
            {
                let result = silex_desktop::export::export_root()
                    .and_then(|root| self.start_workspace_export(ui.ctx(), root));
                self.report(result);
                ui.close();
            }
        });

        let shortcuts = sidebar_row(
            ui,
            &RowParts::nav(icons::KEYBOARD, "Atajos de teclado", None),
        );
        egui::Popup::menu(&shortcuts.row).show(Self::shortcuts_help);

        let settings = sidebar_row(ui, &RowParts::nav(icons::SETTINGS, "Ajustes", None));
        egui::Popup::menu(&settings.row)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.settings_menu(ui));
        let _ = p;
    }

    /// Theme and interface size.
    pub(super) fn settings_menu(&mut self, ui: &mut egui::Ui) {
        let mut settings = self.settings;
        ui.label(egui::RichText::new("Tema").small().weak());
        ui.horizontal(|ui| {
            for mode in ThemeMode::ALL {
                ui.selectable_value(&mut settings.theme, mode, mode.label());
            }
        });
        ui.add_space(theme::space::SM);
        ui.label(egui::RichText::new("Tamaño de la interfaz").small().weak());
        ui.horizontal(|ui| {
            for zoom in ZOOM_STEPS {
                ui.selectable_value(&mut settings.zoom_percent, zoom, format!("{zoom} %"));
            }
        });
        let cmd = cmd_label(ui.ctx());
        ui.label(
            egui::RichText::new(format!("También con {cmd}+ / {cmd}− / {cmd}+0"))
                .small()
                .weak(),
        );
        let ctx = ui.ctx().clone();
        self.set_settings(&ctx, settings);
    }

    pub(super) fn shortcuts_help(ui: &mut egui::Ui) {
        let cmd = cmd_label(ui.ctx());
        egui::Grid::new("silex_shortcuts")
            .num_columns(2)
            .spacing([theme::space::LG, theme::space::XS])
            .show(ui, |ui| {
                for (keys, what) in [
                    (format!("{cmd}+N"), "Nueva página"),
                    (format!("{cmd}+Mayús+N"), "Nueva subpágina"),
                    (format!("{cmd}+K"), "Buscar páginas y acciones"),
                    ("Alt+↑ / Alt+↓".to_owned(), "Página anterior / siguiente"),
                    ("F2".to_owned(), "Renombrar la página abierta"),
                    (format!("{cmd}+S"), "Guardar ahora"),
                    (format!("{cmd}+Z / {cmd}+Mayús+Z"), "Deshacer / rehacer"),
                    ("/".to_owned(), "Cambiar el tipo de bloque"),
                    ("Clic derecho".to_owned(), "Acciones de una página"),
                    (
                        format!("{cmd}+ / {cmd}− / {cmd}+0"),
                        "Tamaño de la interfaz",
                    ),
                    ("Tab / Mayús+Tab".to_owned(), "Moverse entre controles"),
                    ("Intro / Espacio".to_owned(), "Activar el control enfocado"),
                ] {
                    ui.monospace(keys);
                    ui.label(what);
                    ui.end_row();
                }
            });
    }
}
