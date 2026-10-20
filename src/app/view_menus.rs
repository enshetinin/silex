//! The view's menus in the table's toolbar (H6.3a, ADR 0006): filters,
//! sort and visible columns; on a board (H6.5), the properties on the
//! cards and the property it groups by. They edit the view's settings in place and
//! say whether they changed; the table applies them at once and saves them
//! (`DatabaseState::view_changed`).
use eframe::egui::{self, TextEdit};
use silex_desktop::model::ViewKind;
use silex_desktop::property::{Property, PropertyKind};
use silex_desktop::view::{Direction, Filter, FilterOp, NAME, Sort, ViewConfig, board_property};

use super::*;
use super::{icons, theme};

/// The filter value field `index` (to know when one is being typed).
pub(super) fn filter_value_id(index: usize) -> egui::Id {
    egui::Id::unique(("silex-filter-value", index))
}

/// «Filtrar», «Ordenar» and «Columnas» (on a board, «Agrupar» first, and
/// «Propiedades»), left to right before «Propiedad». Each shows how many
/// rules it has and is accented then; `compact` shows icons only. Returns
/// whether the view's settings changed.
pub(super) fn view_menus(ui: &mut egui::Ui, state: &mut DatabaseState, compact: bool) -> bool {
    let DatabaseState {
        views,
        view,
        properties,
        typing_filter,
        view_picker,
        ..
    } = state;
    *typing_filter = false;
    let Some((kind, config)) = views.get_mut(*view).map(|v| (v.kind, &mut v.config)) else {
        return false;
    };
    let board = kind == ViewKind::Board;
    let mut changed = false;

    if board {
        let group = board_property(properties, config);
        let label = match group {
            Some(p) => format!("Agrupar: {}", p.name),
            None => "Agrupar".to_owned(),
        };
        let button = toolbar_button(ui, icons::KANBAN, &label, false, compact);
        menu_from(&button, "silex-group-menu", 240.0).show(|ui| {
            changed |= group_menu(ui, config, properties);
        });
    }

    let label = counted("Filtrar", config.filters.len());
    let button = toolbar_button(
        ui,
        icons::FUNNEL,
        &label,
        !config.filters.is_empty(),
        compact,
    );
    if button.clicked() {
        *view_picker = None;
    }
    menu_from(&button, "silex-filter-menu", 340.0).show(|ui| {
        changed |= filter_menu(ui, config, properties, view_picker, typing_filter);
    });

    let label = counted("Ordenar", config.sorts.len());
    let button = toolbar_button(
        ui,
        icons::ARROWS_DOWN_UP,
        &label,
        !config.sorts.is_empty(),
        compact,
    );
    if button.clicked() {
        *view_picker = None;
    }
    menu_from(&button, "silex-sort-menu", 320.0).show(|ui| {
        changed |= sort_menu(ui, config, properties, view_picker);
    });

    let hidden = properties
        .iter()
        .filter(|p| config.is_hidden(&p.id))
        .count();
    let label = match (board, hidden) {
        (false, 0) => "Columnas".to_owned(),
        (false, 1) => "Columnas (1 oculta)".to_owned(),
        (false, n) => format!("Columnas ({n} ocultas)"),
        (true, 0) => "Propiedades".to_owned(),
        (true, 1) => "Propiedades (1 oculta)".to_owned(),
        (true, n) => format!("Propiedades ({n} ocultas)"),
    };
    let button = toolbar_button(ui, icons::EYE, &label, hidden > 0, compact);
    if button.clicked() {
        *view_picker = None;
    }
    menu_from(&button, "silex-columns-menu", 240.0).show(|ui| {
        let caption = if board {
            "Propiedades en las tarjetas"
        } else {
            "Columnas de esta vista"
        };
        changed |= columns_menu(ui, config, properties, caption);
    });

    changed
}

/// The select properties a board can group by, the current one ticked.
fn group_menu(ui: &mut egui::Ui, config: &mut ViewConfig, properties: &[Property]) -> bool {
    menu_spacing(ui);
    menu_caption(ui, "Agrupar por");
    let current = board_property(properties, config).map(|p| p.id.clone());
    let mut changed = false;
    let selects = properties.iter().filter(|p| p.kind == PropertyKind::Select);
    let mut any = false;
    for property in selects {
        any = true;
        let on = current.as_deref() == Some(property.id.as_str());
        if check_row(ui, on, property_icon(property.kind), &property.name).clicked() && !on {
            config.group_by = Some(property.id.clone());
            changed = true;
        }
    }
    if !any {
        menu_caption(
            ui,
            "Un tablero se agrupa por una propiedad de selección, y todavía no hay ninguna.",
        );
    }
    changed
}

/// «Ordenar» or «Ordenar (2)».
fn counted(label: &str, count: usize) -> String {
    match count {
        0 => label.to_owned(),
        n => format!("{label} ({n})"),
    }
}

/// A small toolbar button, accented while its rules are in use. Compact
/// (a narrow page): the icon alone, with the label as its tooltip.
fn toolbar_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    active: bool,
    compact: bool,
) -> egui::Response {
    let p = theme::current(ui.ctx());
    let color = if active {
        p.accent_strong
    } else {
        p.text_muted
    };
    if compact {
        return icons::button(ui, true, icon, color, label, None);
    }
    ui.add(
        egui::Button::new((
            icons::icon(icon).color(color),
            egui::RichText::new(format!(" {label}"))
                .small()
                .color(color),
        ))
        .frame_when_inactive(false),
    )
    .accessible_name(label)
}

/// The popup menu of a toolbar button. Clicks inside keep it open.
fn menu_from<'a>(button: &egui::Response, id: &str, width: f32) -> egui::Popup<'a> {
    egui::Popup::menu(button)
        .id(egui::Id::unique(id))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(width)
}

/// What a rule can be about: «Nombre» and every live property.
fn choices(properties: &[Property]) -> Vec<Column> {
    std::iter::once((NAME.to_owned(), "Nombre".to_owned(), None))
        .chain(
            properties
                .iter()
                .map(|p| (p.id.clone(), p.name.clone(), Some(p.kind))),
        )
        .collect()
}

/// The name and type of what a rule is about; `None` for a property that
/// is no longer live (its rule does nothing until it is restored).
fn column(rule: &str, properties: &[Property]) -> Option<(String, Option<PropertyKind>)> {
    if rule == NAME {
        return Some(("Nombre".to_owned(), None));
    }
    let property = properties.iter().find(|p| p.id == rule)?;
    Some((property.name.clone(), Some(property.kind)))
}

fn kind_icon(kind: Option<PropertyKind>) -> &'static str {
    kind.map_or(icons::TEXT_AA, property_icon)
}

/// Which inline list of a view menu is open, if any: `"filter:0:op"`,
/// `"sort:add"`… Choices open in place, inside the same menu, instead of
/// in submenus: egui closes a whole menu when one of its submenus closes.
type Picker = Option<String>;

/// A choice of an inline list: its value, label and icon.
type Item<T> = (T, String, Option<&'static str>);

/// A column a rule can be about: its id, name and type (`None` for
/// «Nombre»).
type Column = (String, String, Option<PropertyKind>);

/// A button showing `label` and a caret that opens or closes the inline
/// list `key`. Returns whether that list is open.
fn picker(ui: &mut egui::Ui, open: &mut Picker, key: &str, label: &str) -> bool {
    let is_open = open.as_deref() == Some(key);
    let caret = if is_open {
        icons::CARET_UP
    } else {
        icons::CARET_DOWN
    };
    let button = ui
        .add(
            egui::Button::new((egui::RichText::new(label), icons::icon(caret).size(12.0)))
                .selected(is_open),
        )
        .accessible_name(label);
    if button.clicked() {
        *open = (!is_open).then(|| key.to_owned());
    }
    open.as_deref() == Some(key)
}

/// The choices of an open inline list, indented under its button. A click
/// closes the list and returns the value chosen.
fn pick<T: Clone>(ui: &mut egui::Ui, open: &mut Picker, items: &[Item<T>]) -> Option<T> {
    let mut chosen = None;
    ui.indent("silex-pick", |ui| {
        for (value, label, icon) in items {
            let clicked = match icon {
                Some(icon) => ui.button((icons::icon(icon), format!(" {label}"))),
                None => ui.button(label.as_str()),
            }
            .clicked();
            if clicked {
                chosen = Some(value.clone());
            }
        }
    });
    if chosen.is_some() {
        *open = None;
    }
    chosen
}

/// The columns a rule can be about, for [`pick`].
fn column_items(options: &[Column]) -> Vec<Item<(String, Option<PropertyKind>)>> {
    options
        .iter()
        .map(|(id, name, kind)| ((id.clone(), *kind), name.clone(), Some(kind_icon(*kind))))
        .collect()
}

/// The line of a rule: its choices from the left and a «×» that removes
/// it, at the right end when there is room (so it does not move with the
/// width of the choices), else right after them (never on top of them).
fn rule_line(
    ui: &mut egui::Ui,
    remove_name: String,
    remove: &mut Option<usize>,
    index: usize,
    choices: impl FnOnce(&mut egui::Ui),
) {
    let p = theme::current(ui.ctx());
    ui.horizontal(|ui| {
        choices(ui);
        let button = egui::Button::new(icons::icon(icons::CLOSE).color(p.text_muted))
            .frame_when_inactive(false);
        let room = theme::icon::MD + 2.0 * ui.spacing().button_padding.x;
        let response = if ui.available_width() > room {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(button)
            })
            .inner
        } else {
            ui.add(button)
        };
        if response
            .on_hover_text("Quitar")
            .accessible_name(remove_name)
            .clicked()
        {
            *remove = Some(index);
        }
    });
}

/// One check box per property: ticked, it is a column of this view (on a
/// board, it is shown on the cards).
fn columns_menu(
    ui: &mut egui::Ui,
    config: &mut ViewConfig,
    properties: &[Property],
    caption: &str,
) -> bool {
    menu_spacing(ui);
    let mut changed = false;
    menu_caption(ui, caption);
    if properties.is_empty() {
        menu_caption(ui, "Todavía no hay propiedades.");
    }
    for property in properties {
        let shown = !config.is_hidden(&property.id);
        let row = check_row(ui, shown, property_icon(property.kind), &property.name);
        if row.clicked() {
            config.set_hidden(&property.id, shown);
            changed = true;
        }
    }
    if properties.iter().any(|p| config.is_hidden(&p.id)) {
        ui.separator();
        if ui
            .button((icons::icon(icons::EYE), " Mostrar todas"))
            .clicked()
        {
            config.hidden.clear();
            changed = true;
        }
    }
    changed
}

/// The sort rules, each a column and a direction; the first decides, the
/// next ones break its ties.
fn sort_menu(
    ui: &mut egui::Ui,
    config: &mut ViewConfig,
    properties: &[Property],
    open: &mut Picker,
) -> bool {
    menu_spacing(ui);
    let mut changed = false;
    let options = choices(properties);
    if config.sorts.is_empty() {
        menu_caption(
            ui,
            "Sin orden: las filas van en el orden en que se añadieron.",
        );
    }
    let mut remove = None;
    for (index, sort) in config.sorts.iter_mut().enumerate() {
        let name = column(&sort.property, properties)
            .map_or("(propiedad borrada)".to_owned(), |(name, _)| name);
        let (column_key, direction_key) =
            (format!("sort:{index}:column"), format!("sort:{index}:dir"));
        rule_line(
            ui,
            format!("Quitar el orden por {name}"),
            &mut remove,
            index,
            |ui| {
                picker(ui, open, &column_key, &name);
                picker(ui, open, &direction_key, sort.direction.label());
            },
        );
        if open.as_deref() == Some(column_key.as_str())
            && let Some((id, _)) = pick(ui, open, &column_items(&options))
        {
            sort.property = id;
            changed = true;
        }
        if open.as_deref() == Some(direction_key.as_str()) {
            let items = [Direction::Ascending, Direction::Descending]
                .map(|d| (d, d.label().to_owned(), None));
            if let Some(direction) = pick(ui, open, &items) {
                sort.direction = direction;
                changed = true;
            }
        }
    }
    if let Some(index) = remove {
        config.sorts.remove(index);
        *open = None;
        changed = true;
    }
    let unsorted: Vec<_> = options
        .iter()
        .filter(|(id, _, _)| !config.sorts.iter().any(|s| s.property == *id))
        .cloned()
        .collect();
    if !unsorted.is_empty() {
        ui.separator();
        if picker(ui, open, "sort:add", "+ Añadir un orden")
            && let Some((property, _)) = pick(ui, open, &column_items(&unsorted))
        {
            config.sorts.push(Sort {
                property,
                direction: Direction::Ascending,
            });
            changed = true;
        }
    }
    changed
}

/// The filters, each a column, an operator of its type and, for most, a
/// value. A row is shown when it matches them all; a filter whose value
/// does not fit (yet) shows every row and says so.
fn filter_menu(
    ui: &mut egui::Ui,
    config: &mut ViewConfig,
    properties: &[Property],
    open: &mut Picker,
    typing: &mut bool,
) -> bool {
    menu_spacing(ui);
    let mut changed = false;
    let options = choices(properties);
    if config.filters.is_empty() {
        menu_caption(ui, "Sin filtros: se ven todas las filas.");
    }
    let mut remove = None;
    for (index, filter) in config.filters.iter_mut().enumerate() {
        if index > 0 {
            ui.add_space(theme::space::XS);
        }
        let (name, kind) = column(&filter.property, properties)
            .unwrap_or(("(propiedad borrada)".to_owned(), None));
        let (column_key, op_key) = (
            format!("filter:{index}:column"),
            format!("filter:{index}:op"),
        );
        rule_line(
            ui,
            format!("Quitar el filtro {}", index + 1),
            &mut remove,
            index,
            |ui| {
                picker(ui, open, &column_key, &name);
                picker(ui, open, &op_key, filter.op.label(kind));
            },
        );
        if open.as_deref() == Some(column_key.as_str())
            && let Some((id, kind)) = pick(ui, open, &column_items(&options))
        {
            filter.property = id;
            filter.op = FilterOp::for_kind(kind)[0];
            filter.value.clear();
            changed = true;
        }
        if open.as_deref() == Some(op_key.as_str()) {
            let items: Vec<_> = FilterOp::for_kind(kind)
                .iter()
                .map(|op| (*op, op.label(kind).to_owned(), None))
                .collect();
            if let Some(op) = pick(ui, open, &items) {
                filter.op = op;
                changed = true;
            }
        }
        if filter.op.takes_value() {
            changed |= filter_value(ui, filter, index, kind, properties, open, typing);
        }
    }
    if let Some(index) = remove {
        config.filters.remove(index);
        *open = None;
        changed = true;
    }
    ui.separator();
    if picker(ui, open, "filter:add", "+ Añadir un filtro")
        && let Some((property, kind)) = pick(ui, open, &column_items(&options))
    {
        config.filters.push(Filter {
            property,
            op: FilterOp::for_kind(kind)[0],
            value: String::new(),
        });
        changed = true;
    }
    changed
}

/// The value of a filter: an option of a select, or typed text.
fn filter_value(
    ui: &mut egui::Ui,
    filter: &mut Filter,
    index: usize,
    kind: Option<PropertyKind>,
    properties: &[Property],
    open: &mut Picker,
    typing: &mut bool,
) -> bool {
    let p = theme::current(ui.ctx());
    let mut changed = false;
    if kind == Some(PropertyKind::Select) {
        let options = properties
            .iter()
            .find(|p| p.id == filter.property)
            .map(|p| p.options.clone())
            .unwrap_or_default();
        let current = options
            .iter()
            .find(|o| o.id == filter.value)
            .map_or("Elige una opción".to_owned(), |o| o.name.clone());
        let key = format!("filter:{index}:value");
        if picker(ui, open, &key, &current) {
            if options.is_empty() {
                menu_caption(ui, "Esta propiedad aún no tiene opciones.");
            }
            let items: Vec<_> = options
                .iter()
                .map(|o| (o.id.clone(), o.name.clone(), None))
                .collect();
            if let Some(option) = pick(ui, open, &items) {
                filter.value = option;
                changed = true;
            }
        }
    } else {
        let hint = match kind {
            Some(PropertyKind::Number) => "Número",
            Some(PropertyKind::Date) => "AAAA-MM-DD",
            _ => "Texto",
        };
        let field = framed(
            ui,
            TextEdit::singleline(&mut filter.value)
                .id(filter_value_id(index))
                .margin(egui::Margin::symmetric(MENU_PAD as i8, 4))
                .hint_text(egui::RichText::new(hint).color(p.text_faint))
                .desired_width(f32::INFINITY),
        )
        .accessible_name(format!("Valor del filtro {}", index + 1));
        changed |= field.changed();
        *typing |= field.has_focus();
    }
    if !filter.value.trim().is_empty() && !filter.is_complete(properties) {
        menu_caption(ui, "Valor no válido: este filtro no se aplica.");
    }
    changed
}
