//! What a database view shows (H6.3, ADR 0006): its sort, its filters,
//! its hidden properties and, on a board (H6.5), the property it groups
//! by, stored as JSON in `db_views.config`; and the rows they leave in
//! sight. Pure: no SQL and no egui.
//!
//! The JSON is read leniently, like the settings: invalid JSON, unknown
//! values or rules of a kind this version does not know are left out, and a
//! damaged view never stops a database from opening. Unknown top-level keys
//! (a newer Silex) are kept and written back.
use std::cmp::Ordering;

use serde_json::{Map, Value, json};

use crate::editor::fold;
use crate::model::Page;
use crate::property::{CellValue, Date, Property, PropertyKind, TableValues, parse_number};

/// The «Nombre» column (the row's title) in sort and filter rules, where
/// properties go by their id (a UUID, so never this).
pub const NAME: &str = "name";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ViewConfig {
    /// Applied in order: the second breaks the ties of the first, and so on.
    /// With none, rows keep their manual order.
    pub sorts: Vec<Sort>,
    /// A row is shown when it matches all of them.
    pub filters: Vec<Filter>,
    /// Ids of the properties not shown as columns (on a board, not shown
    /// on the cards).
    pub hidden: Vec<String>,
    /// The select property a board groups its cards by. `None`, or one
    /// that is no longer a live select: the first select property
    /// ([`board_property`]).
    pub group_by: Option<String>,
    /// Keys this version does not use, kept as they were.
    extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    /// A property id or [`NAME`].
    pub property: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::Ascending => "asc",
            Direction::Descending => "desc",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        [Direction::Ascending, Direction::Descending]
            .into_iter()
            .find(|d| d.as_str() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::Ascending => "Ascendente",
            Direction::Descending => "Descendente",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// A property id or [`NAME`].
    pub property: String,
    pub op: FilterOp,
    /// As typed: text, a number, a date, or an option id. Unused by the
    /// operators that take no value ([`FilterOp::takes_value`]).
    pub value: String,
}

/// How a filter compares. Which ones a property offers depends on its type:
/// see [`FilterOp::for_kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterOp {
    Contains,
    NotContains,
    Is,
    IsNot,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    Before,
    After,
    Checked,
    Unchecked,
    Empty,
    NotEmpty,
}

impl FilterOp {
    const ALL: [FilterOp; 14] = [
        FilterOp::Contains,
        FilterOp::NotContains,
        FilterOp::Is,
        FilterOp::IsNot,
        FilterOp::Greater,
        FilterOp::Less,
        FilterOp::GreaterOrEqual,
        FilterOp::LessOrEqual,
        FilterOp::Before,
        FilterOp::After,
        FilterOp::Checked,
        FilterOp::Unchecked,
        FilterOp::Empty,
        FilterOp::NotEmpty,
    ];

    fn as_str(self) -> &'static str {
        match self {
            FilterOp::Contains => "contains",
            FilterOp::NotContains => "not_contains",
            FilterOp::Is => "is",
            FilterOp::IsNot => "is_not",
            FilterOp::Greater => "greater",
            FilterOp::Less => "less",
            FilterOp::GreaterOrEqual => "greater_or_equal",
            FilterOp::LessOrEqual => "less_or_equal",
            FilterOp::Before => "before",
            FilterOp::After => "after",
            FilterOp::Checked => "checked",
            FilterOp::Unchecked => "unchecked",
            FilterOp::Empty => "empty",
            FilterOp::NotEmpty => "not_empty",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == name)
    }

    /// The operators of a property type, the first one being the default
    /// of a new filter. `None` is the «Nombre» column.
    pub fn for_kind(kind: Option<PropertyKind>) -> &'static [FilterOp] {
        use FilterOp::*;
        match kind {
            None | Some(PropertyKind::Text) => &[Contains, NotContains, Is, IsNot, Empty, NotEmpty],
            Some(PropertyKind::Number) => &[
                Is,
                IsNot,
                Greater,
                Less,
                GreaterOrEqual,
                LessOrEqual,
                Empty,
                NotEmpty,
            ],
            Some(PropertyKind::Checkbox) => &[Checked, Unchecked],
            Some(PropertyKind::Date) => &[Is, Before, After, Empty, NotEmpty],
            Some(PropertyKind::Select) => &[Is, IsNot, Empty, NotEmpty],
            // One of its options (ADR 0018).
            Some(PropertyKind::MultiSelect) => &[Contains, NotContains, Empty, NotEmpty],
        }
    }

    /// Name shown in the filter menu.
    pub fn label(self, kind: Option<PropertyKind>) -> &'static str {
        let number = kind == Some(PropertyKind::Number);
        match self {
            FilterOp::Contains => "contiene",
            FilterOp::NotContains => "no contiene",
            FilterOp::Is if number => "igual a",
            FilterOp::IsNot if number => "distinto de",
            FilterOp::Is => "es",
            FilterOp::IsNot => "no es",
            FilterOp::Greater => "mayor que",
            FilterOp::Less => "menor que",
            FilterOp::GreaterOrEqual => "mayor o igual que",
            FilterOp::LessOrEqual => "menor o igual que",
            FilterOp::Before => "antes de",
            FilterOp::After => "después de",
            FilterOp::Checked => "está marcada",
            FilterOp::Unchecked => "no está marcada",
            FilterOp::Empty => "está vacío",
            FilterOp::NotEmpty => "no está vacío",
        }
    }

    /// Whether the operator compares with a value typed or chosen by the user.
    pub fn takes_value(self) -> bool {
        !matches!(
            self,
            FilterOp::Checked | FilterOp::Unchecked | FilterOp::Empty | FilterOp::NotEmpty
        )
    }
}

impl ViewConfig {
    /// Reads `db_views.config`. Never fails: what cannot be read is left out.
    pub fn from_json(json: &str) -> Self {
        let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(json) else {
            return Self::default();
        };
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
        let list = |v: Option<Value>| match v {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        let sorts = list(map.remove("sort"))
            .iter()
            .filter_map(|item| {
                Some(Sort {
                    property: text(item, "property")?,
                    direction: Direction::parse(&text(item, "direction")?)?,
                })
            })
            .collect();
        let filters = list(map.remove("filters"))
            .iter()
            .filter_map(|item| {
                Some(Filter {
                    property: text(item, "property")?,
                    op: FilterOp::parse(&text(item, "op")?)?,
                    value: text(item, "value").unwrap_or_default(),
                })
            })
            .collect();
        let hidden = list(map.remove("hidden"))
            .into_iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect();
        let group_by = match map.remove("group_by") {
            Some(Value::String(id)) => Some(id),
            _ => None,
        };
        Self {
            sorts,
            filters,
            hidden,
            group_by,
            extra: map,
        }
    }

    /// What is stored in `db_views.config`.
    pub fn to_json(&self) -> String {
        let mut map = self.extra.clone();
        map.insert(
            "sort".into(),
            self.sorts
                .iter()
                .map(|s| json!({"property": s.property, "direction": s.direction.as_str()}))
                .collect(),
        );
        map.insert(
            "filters".into(),
            self.filters
                .iter()
                .map(|f| json!({"property": f.property, "op": f.op.as_str(), "value": f.value}))
                .collect(),
        );
        map.insert("hidden".into(), json!(self.hidden));
        if let Some(group_by) = &self.group_by {
            map.insert("group_by".into(), json!(group_by));
        }
        Value::Object(map).to_string()
    }

    pub fn is_hidden(&self, property: &str) -> bool {
        self.hidden.iter().any(|id| id == property)
    }

    /// Hides or shows a property's column.
    pub fn set_hidden(&mut self, property: &str, hidden: bool) {
        self.hidden.retain(|id| id != property);
        if hidden {
            self.hidden.push(property.to_owned());
        }
    }

    /// The sorts that apply: those on the name or on a live property.
    fn live_sorts<'a>(&'a self, properties: &[Property]) -> Vec<&'a Sort> {
        self.sorts
            .iter()
            .filter(|s| s.property == NAME || properties.iter().any(|p| p.id == s.property))
            .collect()
    }

    /// Whether the view orders its rows by a sort: then they cannot be
    /// moved by hand (H13.1). A sort on a deleted property does not count.
    pub fn is_sorted(&self, properties: &[Property]) -> bool {
        !self.live_sorts(properties).is_empty()
    }
}

/// One cell as the rules see it: the row's title or a property's value.
#[derive(Clone, Copy)]
enum Subject<'a> {
    Name(&'a str),
    Value(&'a Property, Option<&'a CellValue>),
}

/// Where `rule` (a property id or [`NAME`]) points in `row`; `None` if it
/// is a property that is not live (deleted, or of another database).
fn subject<'a>(
    rule: &str,
    row: &'a Page,
    values: &'a TableValues,
    properties: &'a [Property],
) -> Option<Subject<'a>> {
    if rule == NAME {
        return Some(Subject::Name(&row.title));
    }
    let property = properties.iter().find(|p| p.id == rule)?;
    let value = values.get(&row.id).and_then(|cells| cells.get(rule));
    Some(Subject::Value(property, value))
}

impl Subject<'_> {
    fn kind(&self) -> Option<PropertyKind> {
        match self {
            Subject::Name(_) => None,
            Subject::Value(property, _) => Some(property.kind),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Subject::Name(title) => title.trim().is_empty(),
            Subject::Value(_, value) => value.is_none(),
        }
    }

    /// The text of a name or text value, for comparing.
    fn text(&self) -> Option<&str> {
        match self {
            Subject::Name(title) => Some(title),
            Subject::Value(_, Some(CellValue::Text(text))) => Some(text),
            _ => None,
        }
    }
}

impl Filter {
    /// Whether this filter can decide anything: its value fits its type
    /// (a number, a date, an option of the property) and its operator is
    /// one of the type's. A filter that is not complete shows every row.
    pub fn is_complete(&self, properties: &[Property]) -> bool {
        let kind = if self.property == NAME {
            None
        } else {
            match properties.iter().find(|p| p.id == self.property) {
                Some(property) => Some(property.kind),
                None => return false,
            }
        };
        if !FilterOp::for_kind(kind).contains(&self.op) {
            return false;
        }
        if !self.op.takes_value() {
            return true;
        }
        match kind {
            None | Some(PropertyKind::Text) => !self.value.trim().is_empty(),
            Some(PropertyKind::Number) => parse_number(&self.value).is_some(),
            Some(PropertyKind::Date) => Date::parse(&self.value).is_some(),
            Some(PropertyKind::Select | PropertyKind::MultiSelect) => properties
                .iter()
                .any(|p| p.id == self.property && p.option(&self.value).is_some()),
            Some(PropertyKind::Checkbox) => true,
        }
    }

    fn matches(&self, subject: Subject<'_>) -> bool {
        use FilterOp::*;
        match self.op {
            Empty => return subject.is_empty(),
            NotEmpty => return !subject.is_empty(),
            Checked => {
                return matches!(subject, Subject::Value(_, Some(CellValue::Checkbox(true))));
            }
            Unchecked => {
                return !matches!(subject, Subject::Value(_, Some(CellValue::Checkbox(true))));
            }
            _ => {}
        }
        match subject.kind() {
            None | Some(PropertyKind::Text) => {
                let text = fold(subject.text().unwrap_or_default());
                let wanted = fold(self.value.trim());
                match self.op {
                    Contains => text.contains(&wanted),
                    NotContains => !text.contains(&wanted),
                    Is => text.trim() == wanted,
                    IsNot => text.trim() != wanted,
                    _ => true,
                }
            }
            Some(PropertyKind::Number) => {
                let Some(wanted) = parse_number(&self.value) else {
                    return true;
                };
                let Subject::Value(_, Some(CellValue::Number(n))) = subject else {
                    // An empty number is only "distinto de" anything.
                    return self.op == IsNot;
                };
                match self.op {
                    Is => *n == wanted,
                    IsNot => *n != wanted,
                    Greater => *n > wanted,
                    Less => *n < wanted,
                    GreaterOrEqual => *n >= wanted,
                    LessOrEqual => *n <= wanted,
                    _ => true,
                }
            }
            Some(PropertyKind::Date) => {
                let Some(wanted) = Date::parse(&self.value) else {
                    return true;
                };
                let Subject::Value(_, Some(CellValue::Date(date))) = subject else {
                    return false;
                };
                match self.op {
                    Is => *date == wanted,
                    Before => *date < wanted,
                    After => *date > wanted,
                    _ => true,
                }
            }
            Some(PropertyKind::Select) => {
                let current = match subject {
                    Subject::Value(_, Some(CellValue::Select(id))) => Some(id.as_str()),
                    _ => None,
                };
                match self.op {
                    Is => current == Some(self.value.as_str()),
                    IsNot => current != Some(self.value.as_str()),
                    _ => true,
                }
            }
            Some(PropertyKind::MultiSelect) => {
                let has = matches!(
                    subject,
                    Subject::Value(_, Some(CellValue::MultiSelect(ids))) if ids.contains(&self.value)
                );
                match self.op {
                    Contains => has,
                    NotContains => !has,
                    _ => true,
                }
            }
            Some(PropertyKind::Checkbox) => true,
        }
    }
}

/// Orders two cells of the same column, ascending. Text ignores case and
/// accents; a select goes by the order of its options, and a multi-select
/// by its first option.
fn compare(a: Subject<'_>, b: Subject<'_>) -> Ordering {
    if let (Some(x), Some(y)) = (a.text(), b.text()) {
        return fold(x.trim()).cmp(&fold(y.trim())).then_with(|| x.cmp(y));
    }
    let (Subject::Value(property, x), Subject::Value(_, y)) = (a, b) else {
        return Ordering::Equal;
    };
    match (x, y) {
        (Some(CellValue::Number(x)), Some(CellValue::Number(y))) => x.total_cmp(y),
        (Some(CellValue::Checkbox(x)), Some(CellValue::Checkbox(y))) => x.cmp(y),
        (Some(CellValue::Date(x)), Some(CellValue::Date(y))) => x.cmp(y),
        (Some(CellValue::Select(x)), Some(CellValue::Select(y))) => {
            let position = |id: &str| property.options.iter().position(|o| o.id == id);
            position(x).cmp(&position(y))
        }
        // By its first option, then the next ones (ADR 0018).
        (Some(CellValue::MultiSelect(x)), Some(CellValue::MultiSelect(y))) => {
            let positions = |ids: &[String]| -> Vec<Option<usize>> {
                ids.iter()
                    .map(|id| property.options.iter().position(|o| o.id == *id))
                    .collect()
            };
            positions(x).cmp(&positions(y))
        }
        _ => Ordering::Equal,
    }
}

/// The rows a view shows, as indices into `rows` (which come in their
/// manual order): those that match every complete filter, ordered by the
/// sorts. Empty cells go last in either direction; ties keep the manual
/// order. Rules about properties that are not live are left out.
pub fn visible_rows(
    rows: &[Page],
    values: &TableValues,
    properties: &[Property],
    config: &ViewConfig,
) -> Vec<usize> {
    let filters: Vec<&Filter> = config
        .filters
        .iter()
        .filter(|f| f.is_complete(properties))
        .collect();
    let mut shown: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            filters.iter().all(|f| {
                subject(&f.property, &rows[i], values, properties).is_none_or(|s| f.matches(s))
            })
        })
        .collect();
    let sorts = config.live_sorts(properties);
    if sorts.is_empty() {
        return shown;
    }
    shown.sort_by(|&a, &b| {
        for sort in &sorts {
            let (Some(x), Some(y)) = (
                subject(&sort.property, &rows[a], values, properties),
                subject(&sort.property, &rows[b], values, properties),
            ) else {
                continue;
            };
            // Checkboxes have no "empty": unticked sorts first.
            let empty = |s: &Subject<'_>| s.is_empty() && s.kind() != Some(PropertyKind::Checkbox);
            let order = match (empty(&x), empty(&y)) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    let by_value = compare(unticked(x), unticked(y));
                    match sort.direction {
                        Direction::Ascending => by_value,
                        Direction::Descending => by_value.reverse(),
                    }
                }
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        Ordering::Equal
    });
    shown
}

/// The property a board groups by: the view's choice if it is still a live
/// select property, else the first select property. `None` if the
/// database has no select property (the board says how to add one).
pub fn board_property<'a>(properties: &'a [Property], config: &ViewConfig) -> Option<&'a Property> {
    let selects = || properties.iter().filter(|p| p.kind == PropertyKind::Select);
    config
        .group_by
        .as_deref()
        .and_then(|id| selects().find(|p| p.id == id))
        .or_else(|| selects().next())
}

/// A column of a board: the cards whose value is `option`, or that have
/// none (`None`, the first column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardGroup {
    pub option: Option<String>,
    /// Indices into the rows, in the view's order.
    pub rows: Vec<usize>,
}

/// Splits the rows in sight (`order`, indices into `rows`, already
/// filtered and sorted) into the columns of a board grouped by `property`:
/// first the rows without a value, then one column per option in the
/// option's order, empty ones included. A value that is not one of its
/// options goes with the rows without one.
pub fn board_groups(
    order: &[usize],
    rows: &[Page],
    values: &TableValues,
    property: &Property,
) -> Vec<BoardGroup> {
    let mut groups: Vec<BoardGroup> = std::iter::once(None)
        .chain(property.options.iter().map(|o| Some(o.id.clone())))
        .map(|option| BoardGroup {
            option,
            rows: Vec::new(),
        })
        .collect();
    for &index in order {
        let Some(row) = rows.get(index) else {
            continue;
        };
        let column = match values
            .get(&row.id)
            .and_then(|cells| cells.get(&property.id))
        {
            Some(CellValue::Select(id)) => property
                .options
                .iter()
                .position(|o| o.id == *id)
                .map_or(0, |i| i + 1),
            _ => 0,
        };
        groups[column].rows.push(index);
    }
    groups
}

/// A checkbox without a value is an unticked box, for sorting.
fn unticked(subject: Subject<'_>) -> Subject<'_> {
    static NO: CellValue = CellValue::Checkbox(false);
    match subject {
        Subject::Value(p, None) if p.kind == PropertyKind::Checkbox => Subject::Value(p, Some(&NO)),
        other => other,
    }
}

/// What the card of a database shows in a page (H12.11): the columns and
/// first rows of one of its views, and how many rows that view has.
#[derive(Debug, Clone, PartialEq)]
pub struct TablePreview {
    /// The view's name («Tabla»).
    pub view: String,
    /// The properties the view shows, in order (after «Nombre»).
    pub columns: Vec<Property>,
    /// The first rows the view shows, with their values.
    pub rows: Vec<(Page, std::collections::HashMap<String, CellValue>)>,
    /// How many rows the view shows in all.
    pub total: usize,
}

/// The first `max` rows of a view (filtered and sorted as it is) and its
/// columns that are not hidden. `rows` come in their manual order.
pub fn table_preview(
    view: &str,
    rows: &[Page],
    values: &TableValues,
    properties: &[Property],
    config: &ViewConfig,
    max: usize,
) -> TablePreview {
    let shown = visible_rows(rows, values, properties, config);
    TablePreview {
        view: view.to_owned(),
        columns: properties
            .iter()
            .filter(|p| !config.is_hidden(&p.id))
            .cloned()
            .collect(),
        rows: shown
            .iter()
            .take(max)
            .map(|&i| {
                let row = &rows[i];
                (
                    row.clone(),
                    values.get(&row.id).cloned().unwrap_or_default(),
                )
            })
            .collect(),
        total: shown.len(),
    }
}

/// The manual order of a database's rows after moving `moving` to the
/// gap `gap` of the list on show (`shown`, ids in the view's order,
/// filtered: a table, or a board's column), H13.1. `all` is the manual
/// order of every live row. The moved rows keep their order among
/// themselves and go before the first row on show after the gap that is
/// not moved, or else after the last one before it, so rows the view
/// hides keep their place. `None` if nothing would change, or if nothing
/// on show is left to place them by.
pub fn drop_rows(
    all: &[String],
    shown: &[String],
    moving: &[String],
    gap: usize,
) -> Option<Vec<String>> {
    let moved = |id: &String| moving.contains(id);
    let gap = gap.min(shown.len());
    let before = shown[gap..].iter().find(|id| !moved(id));
    let after = shown[..gap].iter().rev().find(|id| !moved(id));
    // The moved rows, in their manual order.
    let rows: Vec<String> = all.iter().filter(|id| moved(id)).cloned().collect();
    if rows.is_empty() {
        return None;
    }
    let mut order: Vec<String> = all.iter().filter(|id| !moved(id)).cloned().collect();
    let at = match (before, after) {
        (Some(next), _) => order.iter().position(|id| id == next)?,
        (None, Some(previous)) => order.iter().position(|id| id == previous)? + 1,
        (None, None) => return None,
    };
    order.splice(at..at, rows);
    (order != all).then_some(order)
}

/// The part of an order that changed between `before` and `after` (the
/// same rows): what it was and what it is, without the rows that kept
/// their place at either end. Undoing a move only needs this (H13.1).
pub fn changed_span(before: &[String], after: &[String]) -> (Vec<String>, Vec<String>) {
    let start = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let end = before[start..]
        .iter()
        .rev()
        .zip(after[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (
        before[start..before.len() - end].to_vec(),
        after[start..after.len() - end].to_vec(),
    )
}

/// `all` with the rows of `wanted` put in that order, in the places those
/// rows take in `all`; the other rows stay where they are. Rows of
/// `wanted` not in `all` (trashed meanwhile) are left out. Applies a
/// [`changed_span`] again, also after other rows came or went.
pub fn arrange(all: &[String], wanted: &[String]) -> Vec<String> {
    let index: std::collections::HashMap<&str, usize> = all
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let wanted: Vec<&String> = wanted
        .iter()
        .filter(|id| index.contains_key(id.as_str()) && seen.insert(id.as_str()))
        .collect();
    let mut places: Vec<usize> = wanted.iter().map(|id| index[id.as_str()]).collect();
    places.sort_unstable();
    let mut order = all.to_vec();
    for (place, id) in places.into_iter().zip(wanted) {
        order[place] = id.clone();
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PageKind;
    use crate::property::{OptionColor, SelectOption};

    fn page(id: &str, title: &str) -> Page {
        Page {
            id: id.into(),
            parent_id: Some("db".into()),
            title: title.into(),
            kind: PageKind::Page,
        }
    }

    fn property(id: &str, kind: PropertyKind) -> Property {
        let options = if kind == PropertyKind::Select {
            ["pendiente", "leyendo", "leido"]
                .into_iter()
                .map(|o| SelectOption {
                    id: o.into(),
                    name: o.into(),
                    color: OptionColor::Gray,
                })
                .collect()
        } else {
            Vec::new()
        };
        Property {
            id: id.into(),
            name: id.into(),
            kind,
            options,
        }
    }

    /// Five books: title, pages, read, finished, state.
    fn library() -> (Vec<Page>, TableValues, Vec<Property>) {
        let rows = vec![
            page("1", "Rayuela"),
            page("2", "Pedro Páramo"),
            page("3", "Ficciones"),
            page("4", "el Aleph"),
            page("5", ""),
        ];
        let properties = vec![
            property("paginas", PropertyKind::Number),
            property("leido", PropertyKind::Checkbox),
            property("fin", PropertyKind::Date),
            property("estado", PropertyKind::Select),
            property("autor", PropertyKind::Text),
        ];
        let date = |d: &str| CellValue::Date(Date::parse(d).expect("date"));
        let mut values = TableValues::new();
        let mut set = |row: &str, property: &str, value: CellValue| {
            values
                .entry(row.into())
                .or_default()
                .insert(property.into(), value);
        };
        set("1", "paginas", CellValue::Number(736.0));
        set("2", "paginas", CellValue::Number(124.0));
        set("3", "paginas", CellValue::Number(174.0));
        set("1", "leido", CellValue::Checkbox(true));
        set("3", "leido", CellValue::Checkbox(true));
        set("1", "fin", date("2026-09-14"));
        set("3", "fin", date("2025-01-02"));
        set("1", "estado", CellValue::Select("leido".into()));
        set("2", "estado", CellValue::Select("pendiente".into()));
        set("4", "estado", CellValue::Select("leyendo".into()));
        set("1", "autor", CellValue::Text("Julio Cortázar".into()));
        set("2", "autor", CellValue::Text("Juan Rulfo".into()));
        set("3", "autor", CellValue::Text("Jorge Luis Borges".into()));
        set("4", "autor", CellValue::Text("Jorge Luis Borges".into()));
        (rows, values, properties)
    }

    fn ids(rows: &[Page], shown: &[usize]) -> Vec<String> {
        shown.iter().map(|&i| rows[i].id.clone()).collect()
    }

    fn sorted(by: &[(&str, Direction)]) -> ViewConfig {
        ViewConfig {
            sorts: by
                .iter()
                .map(|(p, d)| Sort {
                    property: (*p).into(),
                    direction: *d,
                })
                .collect(),
            ..ViewConfig::default()
        }
    }

    fn filtered(rules: &[(&str, FilterOp, &str)]) -> ViewConfig {
        ViewConfig {
            filters: rules
                .iter()
                .map(|(p, op, v)| Filter {
                    property: (*p).into(),
                    op: *op,
                    value: (*v).into(),
                })
                .collect(),
            ..ViewConfig::default()
        }
    }

    #[test]
    fn without_rules_rows_keep_their_manual_order() {
        let (rows, values, properties) = library();
        let shown = visible_rows(&rows, &values, &properties, &ViewConfig::default());
        assert_eq!(ids(&rows, &shown), ["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn sorts_put_empty_cells_last_either_way() {
        use Direction::*;
        let (rows, values, properties) = library();
        let by = |rules: &[(&str, Direction)]| {
            ids(
                &rows,
                &visible_rows(&rows, &values, &properties, &sorted(rules)),
            )
        };
        assert_eq!(by(&[("paginas", Ascending)]), ["2", "3", "1", "4", "5"]);
        assert_eq!(by(&[("paginas", Descending)]), ["1", "3", "2", "4", "5"]);
        // Name: without case or accents; the untitled row last.
        assert_eq!(by(&[(NAME, Ascending)]), ["4", "3", "2", "1", "5"]);
        assert_eq!(by(&[("fin", Descending)]), ["1", "3", "2", "4", "5"]);
        // Select: in the order of its options.
        assert_eq!(by(&[("estado", Ascending)]), ["2", "4", "1", "3", "5"]);
        // Checkbox: unticked first, and nothing is "empty".
        assert_eq!(by(&[("leido", Ascending)]), ["2", "4", "5", "1", "3"]);
        // A second sort breaks the ties of the first.
        assert_eq!(
            by(&[("autor", Ascending), (NAME, Descending)]),
            ["3", "4", "2", "1", "5"]
        );
        // A rule about a deleted property is left out.
        assert_eq!(by(&[("borrada", Ascending)]), ["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn filters_match_by_type_and_all_must_hold() {
        use FilterOp::*;
        let (rows, values, properties) = library();
        let with = |rules: &[(&str, FilterOp, &str)]| {
            ids(
                &rows,
                &visible_rows(&rows, &values, &properties, &filtered(rules)),
            )
        };
        assert_eq!(with(&[(NAME, Contains, "PARAMO")]), ["2"]);
        assert_eq!(with(&[(NAME, NotContains, "a")]), ["3", "5"]);
        assert_eq!(with(&[(NAME, Empty, "")]), ["5"]);
        assert_eq!(with(&[("autor", Is, "jorge luis borges")]), ["3", "4"]);
        assert_eq!(
            with(&[("autor", IsNot, "Juan Rulfo")]),
            ["1", "3", "4", "5"]
        );
        assert_eq!(with(&[("paginas", Greater, "150")]), ["1", "3"]);
        assert_eq!(with(&[("paginas", LessOrEqual, "174,0")]), ["2", "3"]);
        assert_eq!(with(&[("paginas", IsNot, "124")]), ["1", "3", "4", "5"]);
        assert_eq!(with(&[("paginas", Empty, "")]), ["4", "5"]);
        assert_eq!(with(&[("leido", Checked, "")]), ["1", "3"]);
        assert_eq!(with(&[("leido", Unchecked, "")]), ["2", "4", "5"]);
        assert_eq!(with(&[("fin", Before, "1/6/2026")]), ["3"]);
        assert_eq!(with(&[("fin", After, "2025-01-02")]), ["1"]);
        assert_eq!(with(&[("estado", Is, "leyendo")]), ["4"]);
        assert_eq!(with(&[("estado", IsNot, "leyendo")]), ["1", "2", "3", "5"]);
        assert_eq!(with(&[("estado", NotEmpty, "")]), ["1", "2", "4"]);
        // All must hold.
        assert_eq!(
            with(&[("autor", Contains, "borges"), ("leido", Checked, "")]),
            ["3"]
        );
    }

    #[test]
    fn incomplete_filters_show_every_row() {
        use FilterOp::*;
        let (rows, values, properties) = library();
        let all = ["1", "2", "3", "4", "5"];
        for rules in [
            [(NAME, Contains, "  ")],
            [("paginas", Greater, "muchas")],
            [("fin", Before, "2026-02-30")],
            [("estado", Is, "no-existe")],
            [("borrada", Is, "x")],
            // An operator of another type.
            [("leido", Contains, "x")],
        ] {
            let config = filtered(&rules);
            assert!(!config.filters[0].is_complete(&properties), "{rules:?}");
            assert_eq!(
                ids(&rows, &visible_rows(&rows, &values, &properties, &config)),
                all,
                "{rules:?}"
            );
        }
    }

    #[test]
    fn operators_belong_to_their_types() {
        for kind in [
            None,
            Some(PropertyKind::Text),
            Some(PropertyKind::Number),
            Some(PropertyKind::Checkbox),
            Some(PropertyKind::Date),
            Some(PropertyKind::Select),
        ] {
            let ops = FilterOp::for_kind(kind);
            assert!(!ops.is_empty());
            for op in ops {
                assert!(!op.label(kind).is_empty());
                assert_eq!(FilterOp::parse(op.as_str()), Some(*op));
            }
        }
        assert_eq!(FilterOp::Is.label(Some(PropertyKind::Number)), "igual a");
        assert_eq!(FilterOp::Is.label(Some(PropertyKind::Select)), "es");
        assert!(!FilterOp::Checked.takes_value());
    }

    #[test]
    fn the_json_round_trips_and_keeps_unknown_keys() {
        let mut config = filtered(&[("paginas", FilterOp::Greater, "100")]);
        config.sorts = sorted(&[(NAME, Direction::Descending)]).sorts;
        config.set_hidden("autor", true);
        assert_eq!(ViewConfig::from_json(&config.to_json()), config);

        let newer = r#"{"sort":[],"widths":{"autor":220},"group_by":"estado"}"#;
        let read = ViewConfig::from_json(newer);
        assert_eq!(read.group_by.as_deref(), Some("estado"));
        let written: Value = serde_json::from_str(&read.to_json()).expect("json");
        assert_eq!(written["widths"]["autor"], 220);
        assert_eq!(written["group_by"], "estado");
    }

    #[test]
    fn a_damaged_config_reads_as_the_default_or_loses_only_its_bad_parts() {
        for json in ["", "nada", "[]", "{", "null", r#"{"sort": 3}"#] {
            assert_eq!(ViewConfig::from_json(json), ViewConfig::default(), "{json}");
        }
        let read = ViewConfig::from_json(
            r#"{"sort":[{"property":"a","direction":"sideways"},{"property":"b","direction":"asc"}],
                "filters":[{"property":"a","op":"resembles","value":"x"},{"op":"empty"},{"property":"c","op":"empty"}],
                "hidden":["d", 4, null]}"#,
        );
        assert_eq!(read.sorts.len(), 1);
        assert_eq!(read.sorts[0].property, "b");
        assert_eq!(read.filters.len(), 1);
        assert_eq!(read.filters[0].property, "c");
        assert_eq!(read.hidden, ["d"]);
        assert_eq!(ViewConfig::from_json(r#"{"group_by": 7}"#).group_by, None);
    }

    #[test]
    fn a_board_groups_by_its_select_or_the_first_one() {
        let (_, _, mut properties) = library();
        let mut config = ViewConfig::default();
        assert_eq!(
            board_property(&properties, &config).map(|p| p.id.as_str()),
            Some("estado")
        );
        let mut otra = property("otra", PropertyKind::Select);
        otra.options.truncate(1);
        properties.push(otra);
        config.group_by = Some("otra".into());
        assert_eq!(
            board_property(&properties, &config).map(|p| p.id.as_str()),
            Some("otra")
        );
        // Not a select, or gone: back to the first select.
        for gone in ["paginas", "borrada"] {
            config.group_by = Some(gone.into());
            assert_eq!(
                board_property(&properties, &config).map(|p| p.id.as_str()),
                Some("estado"),
                "{gone}"
            );
        }
        properties.retain(|p| p.kind != PropertyKind::Select);
        assert!(board_property(&properties, &config).is_none());
    }

    #[test]
    fn board_columns_follow_the_options_and_keep_the_view_order() {
        let (rows, mut values, properties) = library();
        let estado = properties
            .iter()
            .find(|p| p.id == "estado")
            .expect("estado");
        // An option id that is not one of its options: no value.
        values
            .entry("5".into())
            .or_default()
            .insert("estado".into(), CellValue::Select("perdida".into()));
        let by_pages = sorted(&[("paginas", Direction::Descending)]);
        let order = visible_rows(&rows, &values, &properties, &by_pages);
        let groups = board_groups(&order, &rows, &values, estado);
        let shown: Vec<(Option<&str>, Vec<String>)> = groups
            .iter()
            .map(|g| (g.option.as_deref(), ids(&rows, &g.rows)))
            .collect();
        assert_eq!(
            shown,
            [
                (None, vec!["3".to_owned(), "5".to_owned()]),
                (Some("pendiente"), vec!["2".to_owned()]),
                (Some("leyendo"), vec!["4".to_owned()]),
                (Some("leido"), vec!["1".to_owned()]),
            ]
        );
        // Filtered out rows are in no column; empty columns stay.
        let only = filtered(&[(NAME, FilterOp::Contains, "rayuela")]);
        let order = visible_rows(&rows, &values, &properties, &only);
        let groups = board_groups(&order, &rows, &values, estado);
        assert_eq!(groups.len(), 4);
        assert_eq!(
            groups.iter().map(|g| g.rows.len()).collect::<Vec<_>>(),
            [0, 0, 0, 1]
        );
    }

    #[test]
    fn hiding_a_column_twice_keeps_one_entry() {
        let mut config = ViewConfig::default();
        config.set_hidden("autor", true);
        config.set_hidden("autor", true);
        assert_eq!(config.hidden, ["autor"]);
        assert!(config.is_hidden("autor"));
        config.set_hidden("autor", false);
        assert!(!config.is_hidden("autor"));
    }

    // ---- H12.11: the card of a database in a page ----

    #[test]
    fn a_preview_is_the_first_rows_of_the_view_and_its_visible_columns() {
        let (rows, values, properties) = library();
        let mut config = filtered(&[("leido", FilterOp::Unchecked, "")]);
        config.sorts = sorted(&[(NAME, Direction::Ascending)]).sorts;
        config.set_hidden("fin", true);
        let preview = table_preview("Pendientes", &rows, &values, &properties, &config, 2);
        assert_eq!(preview.view, "Pendientes");
        assert_eq!(preview.total, 3, "Rayuela and Ficciones are read");
        let titles: Vec<&str> = preview.rows.iter().map(|(p, _)| p.title.as_str()).collect();
        assert_eq!(titles, ["el Aleph", "Pedro Páramo"]);
        assert_eq!(
            preview.rows[1].1.get("autor"),
            Some(&CellValue::Text("Juan Rulfo".into()))
        );
        let columns: Vec<&str> = preview.columns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(columns, ["paginas", "leido", "estado", "autor"]);
        // An empty database: no rows, the columns all the same.
        let empty = table_preview(
            "Tabla",
            &[],
            &values,
            &properties,
            &ViewConfig::default(),
            5,
        );
        assert_eq!(
            (empty.total, empty.rows.len(), empty.columns.len()),
            (0, 0, 5)
        );
    }

    fn order(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_row_dropped_in_a_gap_goes_there() {
        let all = order("a b c d");
        // Down: between c and d.
        assert_eq!(
            drop_rows(&all, &all, &order("a"), 3),
            Some(order("b c a d"))
        );
        // Up, to the top, and to the end.
        assert_eq!(
            drop_rows(&all, &all, &order("d"), 0),
            Some(order("d a b c"))
        );
        assert_eq!(
            drop_rows(&all, &all, &order("a"), 4),
            Some(order("b c d a"))
        );
        // Its own place, on either side: nothing changes.
        assert_eq!(drop_rows(&all, &all, &order("b"), 1), None);
        assert_eq!(drop_rows(&all, &all, &order("b"), 2), None);
        // A gap past the end is the end.
        assert_eq!(
            drop_rows(&all, &all, &order("a"), 99),
            Some(order("b c d a"))
        );
    }

    #[test]
    fn rows_hidden_by_the_view_keep_their_place() {
        // The view hides x and y.
        let all = order("a x b y c");
        let shown = order("a b c");
        // Between a and b: before b, so after x.
        assert_eq!(
            drop_rows(&all, &shown, &order("c"), 1),
            Some(order("a x c b y"))
        );
        // At the end of what is shown: after c, the last one shown.
        assert_eq!(
            drop_rows(&all, &shown, &order("a"), 3),
            Some(order("x b y c a"))
        );
        // A board's column shows its rows in their manual order too.
        assert_eq!(
            drop_rows(&all, &order("x y"), &order("y"), 0),
            Some(order("a y x b c"))
        );
    }

    #[test]
    fn several_rows_move_together_in_their_order() {
        let all = order("a b c d e");
        // a and c (chosen in that order or not) to the end.
        assert_eq!(
            drop_rows(&all, &all, &order("c a"), 5),
            Some(order("b d e a c"))
        );
        // Dropped in a gap next to one of them: placed by the first row
        // that is not moved.
        assert_eq!(drop_rows(&all, &all, &order("b c"), 2), None,);
        assert_eq!(
            drop_rows(&all, &all, &order("a c"), 2),
            Some(order("b a c d e"))
        );
        // Every row on show moved: nothing to place them by.
        assert_eq!(drop_rows(&all, &order("b c"), &order("b c"), 0), None);
        assert_eq!(drop_rows(&all, &all, &order("zz"), 0), None);
    }

    #[test]
    fn the_changed_span_undoes_and_redoes_a_move() {
        let before = order("a b c d e f");
        let after = order("a d b c e f");
        let (was, is) = changed_span(&before, &after);
        assert_eq!((was.clone(), is.clone()), (order("b c d"), order("d b c")));
        assert_eq!(arrange(&after, &was), before);
        assert_eq!(arrange(&before, &is), after);
        // Also after a row came and another went meanwhile.
        let now = order("a d b z c f");
        assert_eq!(arrange(&now, &was), order("a b c z d f"));
        // Nothing changed: nothing to keep.
        assert_eq!(changed_span(&before, &before), (vec![], vec![]));
        // Repeated or unknown ids are left out.
        assert_eq!(arrange(&order("a b c"), &order("c c q a")), order("c b a"));
    }

    /// Small deterministic generator (no new crates).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    #[test]
    fn random_drops_keep_every_row_and_undo_exactly() {
        let mut rng = Rng(0x5eed_1301);
        for _ in 0..3000 {
            let len = 1 + rng.next(12);
            let all: Vec<String> = (0..len).map(|i| format!("r{i}")).collect();
            let shown: Vec<String> = all.iter().filter(|_| rng.next(4) > 0).cloned().collect();
            if shown.is_empty() {
                continue;
            }
            let moving: Vec<String> = shown.iter().filter(|_| rng.next(3) == 0).cloned().collect();
            let gap = rng.next(shown.len() + 1);
            let Some(after) = drop_rows(&all, &shown, &moving, gap) else {
                continue;
            };
            // The same rows, the moved ones together and in their order,
            // the others in theirs.
            let mut sorted = after.clone();
            sorted.sort();
            let mut expected = all.clone();
            expected.sort();
            assert_eq!(sorted, expected);
            let first = after.iter().position(|id| moving.contains(id)).unwrap();
            let together: Vec<String> = after[first..first + moving.len()].to_vec();
            let in_order: Vec<String> = all
                .iter()
                .filter(|id| moving.contains(id))
                .cloned()
                .collect();
            assert_eq!(together, in_order);
            let rest = |order: &[String]| -> Vec<String> {
                order
                    .iter()
                    .filter(|id| !moving.contains(id))
                    .cloned()
                    .collect()
            };
            assert_eq!(rest(&after), rest(&all));
            // The span is enough to undo and redo it.
            let (was, is) = changed_span(&all, &after);
            assert_eq!(arrange(&after, &was), all);
            assert_eq!(arrange(&all, &is), after);
        }
    }

    #[test]
    fn a_view_is_sorted_only_by_rules_that_apply() {
        let properties = vec![property("autor", PropertyKind::Text)];
        let sort = |property: &str| Sort {
            property: property.into(),
            direction: Direction::Ascending,
        };
        let mut config = ViewConfig::default();
        assert!(!config.is_sorted(&properties));
        config.sorts = vec![sort("borrada")];
        assert!(!config.is_sorted(&properties));
        config.sorts.push(sort("autor"));
        assert!(config.is_sorted(&properties));
        config.sorts = vec![sort(NAME)];
        assert!(config.is_sorted(&properties));
    }

    #[test]
    fn a_multi_select_filters_by_one_option_and_sorts_by_its_first() {
        use FilterOp::*;
        let mut generos = property("generos", PropertyKind::Select);
        generos.kind = PropertyKind::MultiSelect;
        let properties = vec![generos];
        let rows = vec![
            page("1", "Rayuela"),
            page("2", "Ficciones"),
            page("3", "Aleph"),
            page("4", "Sin géneros"),
        ];
        let multi =
            |ids: &[&str]| CellValue::MultiSelect(ids.iter().map(|i| (*i).into()).collect());
        let mut values = TableValues::new();
        for (row, ids) in [
            ("1", &["leyendo", "leido"][..]),
            ("2", &["leido"][..]),
            ("3", &["pendiente", "leido"][..]),
        ] {
            values
                .entry(row.into())
                .or_default()
                .insert("generos".into(), multi(ids));
        }
        let with = |rules: &[(&str, FilterOp, &str)]| {
            let config = ViewConfig {
                filters: rules
                    .iter()
                    .map(|(property, op, value)| Filter {
                        property: (*property).into(),
                        op: *op,
                        value: (*value).into(),
                    })
                    .collect(),
                ..ViewConfig::default()
            };
            ids(&rows, &visible_rows(&rows, &values, &properties, &config))
        };
        assert_eq!(
            FilterOp::for_kind(Some(PropertyKind::MultiSelect))[0],
            Contains
        );
        assert_eq!(with(&[("generos", Contains, "leido")]), ["1", "2", "3"]);
        assert_eq!(with(&[("generos", Contains, "leyendo")]), ["1"]);
        // A row without options contains none of them.
        assert_eq!(
            with(&[("generos", NotContains, "leyendo")]),
            ["2", "3", "4"]
        );
        assert_eq!(with(&[("generos", Empty, "")]), ["4"]);
        assert_eq!(with(&[("generos", NotEmpty, "")]), ["1", "2", "3"]);
        // An option of no property, or `Is`, does not apply.
        assert_eq!(with(&[("generos", Contains, "otra")]), ["1", "2", "3", "4"]);
        assert_eq!(with(&[("generos", Is, "leido")]), ["1", "2", "3", "4"]);
        // Sorted by the first option (pendiente, leyendo, leido), empty last
        // either way.
        let sorted = |direction| {
            let config = ViewConfig {
                sorts: vec![Sort {
                    property: "generos".into(),
                    direction,
                }],
                ..ViewConfig::default()
            };
            ids(&rows, &visible_rows(&rows, &values, &properties, &config))
        };
        assert_eq!(sorted(Direction::Ascending), ["3", "1", "2", "4"]);
        assert_eq!(sorted(Direction::Descending), ["2", "1", "3", "4"]);
    }
}
