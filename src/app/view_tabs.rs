//! A database's views as tabs (H6.3b, ADR 0006): a click switches view
//! (each keeps its own sort, filters and columns), «+» adds a table or a
//! board (H6.5), and the
//! open tab (or a right click on any) has a menu to rename, duplicate,
//! move or delete it. Deleting a view loses its settings, never rows.
use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::db::Shift;
use silex_desktop::model::ViewKind;
use silex_desktop::view::{ViewConfig, board_property};

use super::*;
use super::{icons, theme};

fn view_menu_id(view: &str) -> egui::Id {
    egui::Id::unique(("silex-view-menu", view))
}

/// The name of a new view: «Tabla», «Tabla 2»… (or «Tablero»…) the
/// first one not taken.
pub(super) fn new_view_name(kind: ViewKind, taken: &[String]) -> String {
    let base = kind_label(kind);
    (1..)
        .map(|n| match n {
            1 => base.to_owned(),
            n => format!("{base} {n}"),
        })
        .find(|name| !taken.iter().any(|t| t == name))
        .unwrap_or_else(|| base.to_owned())
}

fn kind_label(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Table => "Tabla",
        ViewKind::Board => "Tablero",
    }
}

pub(super) fn view_icon(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::Table => icons::TABLE,
        ViewKind::Board => icons::KANBAN,
    }
}

/// The tabs and «+», at the start of the toolbar. Returns the open tab's
/// rect (its underline goes on the table's top line).
pub(super) fn view_tabs(
    ui: &mut egui::Ui,
    state: &mut DatabaseState,
    actions: &mut Vec<TableAction>,
) -> Option<egui::Rect> {
    let p = theme::current(ui.ctx());
    let mut selected_tab = None;
    let count = state.views.len();
    let DatabaseState {
        views,
        view: current,
        view_rename,
        view_delete,
        ..
    } = state;
    for (index, view) in views.iter().enumerate() {
        let selected = index == *current;
        let color = if selected { p.text } else { p.text_muted };
        let tab = ui
            .add(
                egui::Button::new((
                    icons::icon(view_icon(view.kind)).color(color),
                    egui::RichText::new(&view.name).color(color),
                ))
                .frame_when_inactive(false)
                .selected(selected),
            )
            .on_hover_text(if selected {
                "Opciones de la vista"
            } else {
                "Cambiar a esta vista"
            })
            .accessible_name(format!("Vista «{}»", view.name));
        if selected {
            selected_tab = Some(tab.rect);
        }
        if tab.clicked() && !selected {
            actions.push(TableAction::SwitchView(index));
        }
        let menu = view_menu_id(&view.id);
        let open = ((tab.clicked() && selected) || tab.secondary_clicked())
            && !egui::Popup::is_id_open(ui.ctx(), menu);
        if open {
            *view_rename = Some((view.id.clone(), view.name.clone()));
            *view_delete = None;
        }
        egui::Popup::menu(&tab)
            .id(menu)
            .open_memory(open.then_some(egui::SetOpenCommand::Bool(true)))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(240.0)
            .show(|ui| {
                view_menu(
                    ui,
                    view,
                    (index, count),
                    open,
                    view_rename,
                    view_delete,
                    actions,
                );
            });
    }
    // A new name is saved when its menu closes; Escape drops it.
    if let Some((id, _)) = view_rename
        && !egui::Popup::is_id_open(ui.ctx(), view_menu_id(id))
        && let Some((id, name)) = view_rename.take()
        && !ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        actions.push(TableAction::RenameView(id, name));
    }
    let add = ui
        .add(
            egui::Button::new(icons::icon(icons::PLUS).color(p.text_muted))
                .frame_when_inactive(false),
        )
        .on_hover_text("Nueva vista")
        .accessible_name("Nueva vista");
    egui::Popup::menu(&add)
        .id(egui::Id::unique("silex-add-view-menu"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(200.0)
        .show(|ui| {
            menu_spacing(ui);
            menu_caption(ui, "Nueva vista");
            for kind in [ViewKind::Table, ViewKind::Board] {
                if ui
                    .button((
                        icons::icon(view_icon(kind)),
                        format!(" {}", kind_label(kind)),
                    ))
                    .clicked()
                {
                    actions.push(TableAction::AddView(kind));
                    ui.close();
                }
            }
        });
    selected_tab
}

/// A view's menu: its name, duplicating, moving and deleting (asked first,
/// and never the last view).
fn view_menu(
    ui: &mut egui::Ui,
    view: &silex_desktop::model::DatabaseView,
    (index, count): (usize, usize),
    opening: bool,
    rename: &mut Option<(String, String)>,
    delete: &mut Option<String>,
    actions: &mut Vec<TableAction>,
) {
    let p = theme::current(ui.ctx());
    menu_spacing(ui);
    if let Some((_, name)) = rename.as_mut().filter(|(id, _)| *id == view.id) {
        let field = menu_field(ui, name, "Nombre de la vista", "Nombre de la vista");
        if opening {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.close();
        }
    }
    ui.add_space(theme::space::XS);
    menu_caption(
        ui,
        match view.kind {
            ViewKind::Table => "Vista de tabla",
            ViewKind::Board => "Vista de tablero",
        },
    );
    ui.separator();
    if ui
        .button((icons::icon(icons::COPY), " Duplicar la vista"))
        .clicked()
    {
        actions.push(TableAction::DuplicateView(index));
        ui.close();
    }
    let moves = [
        (
            Shift::Up,
            icons::ARROW_LEFT,
            " Mover a la izquierda",
            index > 0,
        ),
        (
            Shift::Down,
            icons::ARROW_RIGHT,
            " Mover a la derecha",
            index + 1 < count,
        ),
    ];
    for (shift, icon, label, enabled) in moves {
        if ui
            .add_enabled(enabled, egui::Button::new((icons::icon(icon), label)))
            .clicked()
        {
            actions.push(TableAction::MoveView(view.id.clone(), shift));
            ui.close();
        }
    }
    ui.separator();
    if delete.as_deref() == Some(view.id.as_str()) {
        menu_caption(
            ui,
            &format!(
                "¿Borrar la vista «{}»? Se pierden su orden, sus filtros y sus columnas; las filas no.",
                view.name
            ),
        );
        ui.horizontal(|ui| {
            if ui
                .button(egui::RichText::new("Borrar").color(p.danger))
                .clicked()
            {
                actions.push(TableAction::DeleteView(view.id.clone()));
                *delete = None;
                ui.close();
            }
            if ui.button("Cancelar").clicked() {
                *delete = None;
            }
        });
        return;
    }
    let last = count <= 1;
    let button = ui
        .add_enabled(
            !last,
            egui::Button::new((
                icons::icon(icons::TRASH).color(p.danger),
                egui::RichText::new(" Borrar la vista").color(p.danger),
            )),
        )
        .on_disabled_hover_text("Una base de datos tiene siempre al menos una vista");
    if button.clicked() {
        *delete = Some(view.id.clone());
    }
}

impl SilexApp {
    /// Reads the views again (after adding, moving or deleting one) and
    /// shows `select` (a view id), or the one that was open, or the first.
    /// The open view's pending settings are saved first.
    fn reload_views(&mut self, select: Option<&str>) -> Result<()> {
        self.save_view()?;
        let Some(state) = self.database.as_mut() else {
            return Ok(());
        };
        let open = state.views.get(state.view).map(|v| v.id.clone());
        let was = state.view;
        state.views = self.db.database_views(&state.id)?;
        let wanted = select.map(str::to_owned).or(open);
        // A deleted view gives way to the one now in its place.
        state.view = wanted
            .and_then(|id| state.views.iter().position(|v| v.id == id))
            .unwrap_or(was)
            .min(state.views.len().saturating_sub(1));
        state.pinned.clear();
        state.apply_view();
        self.remember_view();
        Ok(())
    }

    /// Notes the open view of the open database, to show it again when the
    /// database is opened later in this session.
    fn remember_view(&mut self) {
        if let Some(state) = &self.database
            && let Some(view) = state.views.get(state.view)
        {
            self.last_views.insert(state.id.clone(), view.id.clone());
        }
    }

    pub(super) fn switch_view(&mut self, index: usize) -> Result<()> {
        // The settings of the view being left are saved first.
        self.save_view()?;
        if let Some(state) = self.database.as_mut()
            && index < state.views.len()
        {
            state.view = index;
            state.pinned.clear();
            state.editing = None;
            state.apply_view();
        }
        self.remember_view();
        Ok(())
    }

    /// A new, empty view at the end, shown at once. A board groups by the
    /// first select property, if there is one.
    pub(super) fn add_view(&mut self, kind: ViewKind) -> Result<()> {
        let Some(state) = self.database.as_ref() else {
            return Ok(());
        };
        let taken: Vec<String> = state.views.iter().map(|v| v.name.clone()).collect();
        let database = state.id.clone();
        let mut config = ViewConfig::default();
        if kind == ViewKind::Board {
            config.group_by = board_property(&state.properties, &config).map(|p| p.id.clone());
        }
        let id = self
            .db
            .add_view(&database, &new_view_name(kind, &taken), kind, &config)?;
        self.reload_views(Some(&id))
    }

    /// A copy of a view, with its sort, filters and columns, shown at once.
    pub(super) fn duplicate_view(&mut self, index: usize) -> Result<()> {
        let Some((database, view)) = self
            .database
            .as_ref()
            .and_then(|d| Some((d.id.clone(), d.views.get(index)?.clone())))
        else {
            return Ok(());
        };
        let name = format!("{} (copia)", view.name);
        let id = self
            .db
            .add_view(&database, &name, view.kind, &view.config)?;
        self.reload_views(Some(&id))
    }

    pub(super) fn rename_view(&mut self, id: &str, name: &str) -> Result<()> {
        let unchanged = self
            .database
            .as_ref()
            .and_then(|d| d.views.iter().find(|v| v.id == id))
            .is_none_or(|v| v.name == name.trim());
        // An empty name keeps the old one.
        if unchanged || name.trim().is_empty() {
            return Ok(());
        }
        self.db.rename_view(id, name.trim())?;
        self.reload_views(None)
    }

    pub(super) fn move_view(&mut self, id: &str, shift: Shift) -> Result<()> {
        self.db.move_view(id, shift)?;
        self.reload_views(None)
    }

    pub(super) fn delete_view(&mut self, id: &str) -> Result<()> {
        let name = self
            .database
            .as_ref()
            .and_then(|d| d.views.iter().find(|v| v.id == id))
            .map(|v| v.name.clone())
            .unwrap_or_default();
        // Its pending settings would be saved into a view about to go.
        if let Some(state) = self.database.as_mut()
            && state.views.get(state.view).is_some_and(|v| v.id == id)
        {
            state.view_dirty = false;
        }
        self.db.delete_view(id)?;
        self.reload_views(None)?;
        self.notice = Some(format!("Se borró la vista «{name}»."));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::new_view_name;
    use silex_desktop::model::ViewKind::{Board, Table};

    #[test]
    fn new_views_take_the_first_free_name() {
        let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(new_view_name(Table, &names(&[])), "Tabla");
        assert_eq!(new_view_name(Table, &names(&["Tabla"])), "Tabla 2");
        assert_eq!(
            new_view_name(Table, &names(&["Tabla", "Tabla 2", "Tabla 4"])),
            "Tabla 3"
        );
        assert_eq!(new_view_name(Table, &names(&["Pendientes"])), "Tabla");
        assert_eq!(new_view_name(Board, &names(&["Tabla"])), "Tablero");
        assert_eq!(new_view_name(Board, &names(&["Tablero"])), "Tablero 2");
    }
}
