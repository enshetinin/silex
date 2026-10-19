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
            Some(PropertyKind::Select) => properties
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
            Some(PropertyKind::Checkbox) => true,
        }
    }
}

/// Orders two cells of the same column, ascending. Text ignores case and
/// accents; a select goes by the order of its options.
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
    let sorts: Vec<&Sort> = config
        .sorts
        .iter()
        .filter(|s| s.property == NAME || properties.iter().any(|p| p.id == s.property))
        .collect();
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
}
