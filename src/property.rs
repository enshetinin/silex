//! Typed database properties (H6.2, ADR 0006): their kinds, the colors of
//! select options, cell values and how text typed in a cell becomes a value.
//! Pure: no SQL and no egui.
use std::{collections::HashMap, fmt};

use crate::editor::fold;

/// The type of a property. Stored as text; see [`PropertyKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    Text,
    Number,
    Checkbox,
    Date,
    Select,
}

impl PropertyKind {
    /// Menu order.
    pub const ALL: [PropertyKind; 5] = [
        PropertyKind::Text,
        PropertyKind::Number,
        PropertyKind::Checkbox,
        PropertyKind::Date,
        PropertyKind::Select,
    ];

    /// Name stored in SQLite (must match the CHECK in migration 0005).
    pub fn as_str(self) -> &'static str {
        match self {
            PropertyKind::Text => "text",
            PropertyKind::Number => "number",
            PropertyKind::Checkbox => "checkbox",
            PropertyKind::Date => "date",
            PropertyKind::Select => "select",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Name shown in the UI (also the name of a new property of this kind).
    pub fn label(self) -> &'static str {
        match self {
            PropertyKind::Text => "Texto",
            PropertyKind::Number => "Número",
            PropertyKind::Checkbox => "Casilla",
            PropertyKind::Date => "Fecha",
            PropertyKind::Select => "Selección",
        }
    }
}

/// Color of a select option. Stored as text; see [`OptionColor::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionColor {
    Gray,
    Blue,
    Green,
    Yellow,
    Orange,
    Red,
    Purple,
    Pink,
}

impl OptionColor {
    pub const ALL: [OptionColor; 8] = [
        OptionColor::Gray,
        OptionColor::Blue,
        OptionColor::Green,
        OptionColor::Yellow,
        OptionColor::Orange,
        OptionColor::Red,
        OptionColor::Purple,
        OptionColor::Pink,
    ];

    /// Name stored in SQLite (must match the CHECK in migration 0005).
    pub fn as_str(self) -> &'static str {
        match self {
            OptionColor::Gray => "gray",
            OptionColor::Blue => "blue",
            OptionColor::Green => "green",
            OptionColor::Yellow => "yellow",
            OptionColor::Orange => "orange",
            OptionColor::Red => "red",
            OptionColor::Purple => "purple",
            OptionColor::Pink => "pink",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }

    /// Color for a property's new option when it already has `existing`
    /// options: the colors in turn, gray last, so neighbours differ.
    pub fn for_new_option(existing: usize) -> Self {
        const TURN: [OptionColor; 8] = [
            OptionColor::Blue,
            OptionColor::Green,
            OptionColor::Yellow,
            OptionColor::Orange,
            OptionColor::Red,
            OptionColor::Purple,
            OptionColor::Pink,
            OptionColor::Gray,
        ];
        TURN[existing % TURN.len()]
    }
}

/// One choice of a select property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub id: String,
    pub name: String,
    pub color: OptionColor,
}

/// A typed column of a database. `options` is empty unless it is a select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub id: String,
    pub name: String,
    pub kind: PropertyKind,
    pub options: Vec<SelectOption>,
}

impl Property {
    pub fn option(&self, id: &str) -> Option<&SelectOption> {
        self.options.iter().find(|o| o.id == id)
    }

    /// Options whose name contains `query`, ignoring case and accents (all
    /// of them for an empty query), and the one called exactly `query`, if
    /// any (creating it again would make a duplicate).
    pub fn matching_options(&self, query: &str) -> (Vec<&SelectOption>, Option<&SelectOption>) {
        let query = fold(query.trim());
        let matches = self
            .options
            .iter()
            .filter(|o| fold(&o.name).contains(&query))
            .collect();
        let exact = self.options.iter().find(|o| fold(o.name.trim()) == query);
        (matches, exact)
    }
}

/// A calendar date without time (`AAAA-MM-DD`). Only valid dates exist:
/// build one with [`Date::new`] or [`Date::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// A date between 0001-01-01 and 9999-12-31 that exists in the calendar.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let valid = (1..=9999).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        valid.then_some(Self { year, month, day })
    }

    /// Reads `AAAA-MM-DD` (as stored and shown) or `D/M/AAAA` (the usual
    /// order in Spanish). Months and days may have one digit.
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        let number = |part: &str, max_len: usize| -> Option<u16> {
            let ok = !part.is_empty()
                && part.len() <= max_len
                && part.bytes().all(|b| b.is_ascii_digit());
            ok.then(|| part.parse().ok()).flatten()
        };
        let iso = input.contains('-');
        if iso && input.contains('/') {
            return None;
        }
        let parts: Vec<&str> = input.split(['-', '/']).collect();
        let (year, month, day) = match (iso, parts.as_slice()) {
            (true, [y, m, d]) | (false, [d, m, y]) if y.len() == 4 => (*y, *m, *d),
            _ => return None,
        };
        Self::new(
            number(year, 4)?,
            u8::try_from(number(month, 2)?).ok()?,
            u8::try_from(number(day, 2)?).ok()?,
        )
    }
}

/// `AAAA-MM-DD`, the format SQLite stores and checks.
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Reads a number typed in a cell: digits with an optional sign and one
/// decimal separator, comma or point (`-3,5`, `1.25`). No thousands
/// separators, exponents, infinities or NaN.
pub fn parse_number(input: &str) -> Option<f64> {
    let input = input.trim();
    let digits = input.strip_prefix(['-', '+']).unwrap_or(input);
    let separators = digits.chars().filter(|c| matches!(c, ',' | '.')).count();
    let valid = digits.chars().any(|c| c.is_ascii_digit())
        && digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.'))
        && separators <= 1;
    if !valid {
        return None;
    }
    let value: f64 = input.replace(',', ".").parse().ok()?;
    // A long enough string of digits overflows to infinity.
    value.is_finite().then_some(value + 0.0) // -0 becomes 0
}

/// A number as shown in a cell: the shortest form that reads back as the
/// same value, with a decimal comma (`3`, `-0,5`, `1234,25`).
pub fn format_number(value: f64) -> String {
    format!("{}", value + 0.0).replace('.', ",")
}

/// The value of one cell. An empty cell has no value (`None` wherever an
/// `Option<CellValue>` is used); see [`CellValue::is_empty`].
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Text(String),
    /// Always finite.
    Number(f64),
    Checkbox(bool),
    Date(Date),
    /// The id of one of the property's options.
    Select(String),
}

impl CellValue {
    pub fn kind(&self) -> PropertyKind {
        match self {
            CellValue::Text(_) => PropertyKind::Text,
            CellValue::Number(_) => PropertyKind::Number,
            CellValue::Checkbox(_) => PropertyKind::Checkbox,
            CellValue::Date(_) => PropertyKind::Date,
            CellValue::Select(_) => PropertyKind::Select,
        }
    }

    /// Values stored as "no value": empty text and an unticked box.
    pub fn is_empty(&self) -> bool {
        match self {
            CellValue::Text(text) => text.is_empty(),
            CellValue::Checkbox(on) => !on,
            _ => false,
        }
    }

    /// The value read from the `db_values` columns for a property of
    /// `kind`. Only the column of that kind counts: a value in another
    /// column (stored by a newer Silex or damaged) reads as empty, never as
    /// an error. `option` is the option id only if it belongs to the property.
    pub fn from_columns(
        kind: PropertyKind,
        text: Option<String>,
        number: Option<f64>,
        flag: Option<i64>,
        date: Option<String>,
        option: Option<String>,
    ) -> Option<Self> {
        let value = match kind {
            PropertyKind::Text => CellValue::Text(text?),
            PropertyKind::Number => CellValue::Number(number.filter(|n| n.is_finite())?),
            PropertyKind::Checkbox => CellValue::Checkbox(flag? == 1),
            PropertyKind::Date => CellValue::Date(Date::parse(&date?)?),
            PropertyKind::Select => CellValue::Select(option?),
        };
        (!value.is_empty()).then_some(value)
    }

    /// Text for the cell and for screen readers. A select shows its
    /// option's name (empty if the option is gone).
    pub fn display(&self, property: &Property) -> String {
        match self {
            CellValue::Text(text) => text.clone(),
            CellValue::Number(n) => format_number(*n),
            CellValue::Checkbox(true) => "Sí".to_owned(),
            CellValue::Checkbox(false) => "No".to_owned(),
            CellValue::Date(date) => date.to_string(),
            CellValue::Select(id) => property
                .option(id)
                .map(|o| o.name.clone())
                .unwrap_or_default(),
        }
    }
}

/// The values of a database's rows: row id → property id → value. Empty
/// cells are absent.
pub type TableValues = HashMap<String, HashMap<String, CellValue>>;

/// Text typed in a cell of a text, number or date property, turned into
/// its value. `Ok(None)` empties the cell; `Err` explains what is expected.
pub fn parse_input(kind: PropertyKind, input: &str) -> Result<Option<CellValue>, String> {
    if input.trim().is_empty() {
        return Ok(None);
    }
    let shown: String = input.trim().chars().take(40).collect();
    match kind {
        PropertyKind::Text => Ok(Some(CellValue::Text(input.to_owned()))),
        PropertyKind::Number => parse_number(input)
            .map(|n| Some(CellValue::Number(n)))
            .ok_or_else(|| format!("«{shown}» no es un número (por ejemplo, 12 o -3,5).")),
        PropertyKind::Date => Date::parse(input)
            .map(|d| Some(CellValue::Date(d)))
            .ok_or_else(|| {
                format!("«{shown}» no es una fecha válida (AAAA-MM-DD, por ejemplo 2026-10-19).")
            }),
        PropertyKind::Checkbox | PropertyKind::Select => Err(format!(
            "Una {} no se escribe.",
            kind.label().to_lowercase()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_colors_round_trip_their_stored_names() {
        for kind in PropertyKind::ALL {
            assert_eq!(PropertyKind::parse(kind.as_str()), Some(kind));
        }
        for color in OptionColor::ALL {
            assert_eq!(OptionColor::parse(color.as_str()), Some(color));
        }
        assert_eq!(PropertyKind::parse("formula"), None);
        assert_eq!(OptionColor::parse("brown"), None);
    }

    #[test]
    fn new_options_take_every_color_in_turn() {
        let colors: Vec<_> = (0..8).map(OptionColor::for_new_option).collect();
        for color in OptionColor::ALL {
            assert!(colors.contains(&color), "{color:?}");
        }
        assert_eq!(OptionColor::for_new_option(0), OptionColor::Blue);
        assert_eq!(OptionColor::for_new_option(8), OptionColor::Blue);
    }

    #[test]
    fn numbers_take_a_comma_or_a_point() {
        let ok = [
            ("12", 12.0),
            ("-3,5", -3.5),
            ("1.25", 1.25),
            ("+4", 4.0),
            (" 7 ", 7.0),
            (",5", 0.5),
            ("5.", 5.0),
            ("-0", 0.0),
        ];
        for (input, value) in ok {
            assert_eq!(parse_number(input), Some(value), "{input:?}");
        }
        let bad = [
            "", "-", ",", "abc", "1,2,3", "1.2.3", "1,000.5", "1e3", "inf", "NaN", "1 000", "--1",
            "0x10", "١٢",
        ];
        for input in bad {
            assert_eq!(parse_number(input), None, "{input:?}");
        }
        assert_eq!(
            parse_number(&"9".repeat(400)),
            None,
            "overflows to infinity"
        );
        assert!(parse_number("-0").is_some_and(|n| n.is_sign_positive()));
    }

    #[test]
    fn numbers_show_with_a_decimal_comma_and_read_back() {
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(-0.5), "-0,5");
        assert_eq!(format_number(1234.25), "1234,25");
        assert_eq!(format_number(-0.0), "0");
        for n in [0.1 + 0.2, 1e15, -42.125, 1.0 / 3.0] {
            assert_eq!(parse_number(&format_number(n)), Some(n), "{n}");
        }
    }

    #[test]
    fn dates_exist_in_the_calendar() {
        let ok = [
            ("2026-10-19", "2026-10-19"),
            ("2024-02-29", "2024-02-29"),
            ("2000-02-29", "2000-02-29"),
            ("2026-1-5", "2026-01-05"),
            ("19/10/2026", "2026-10-19"),
            ("5/1/2026", "2026-01-05"),
            (" 0001-01-01 ", "0001-01-01"),
            ("9999-12-31", "9999-12-31"),
        ];
        for (input, shown) in ok {
            assert_eq!(
                Date::parse(input).map(|d| d.to_string()).as_deref(),
                Some(shown),
                "{input:?}"
            );
        }
        let bad = [
            "2026-02-30",
            "2025-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "0000-01-01",
            "26-10-19",
            "19/10/26",
            "2026/10/19",
            "2026-10-19T10:00",
            "2026-010-19",
            "+2026-10-19",
            "",
            "hoy",
        ];
        for input in bad {
            assert_eq!(Date::parse(input), None, "{input:?}");
        }
    }

    #[test]
    fn stored_columns_count_only_for_their_kind() {
        use PropertyKind as K;
        let text = || Some("hola".to_owned());
        assert_eq!(
            CellValue::from_columns(K::Text, text(), None, None, None, None),
            Some(CellValue::Text("hola".into()))
        );
        // A value in another column reads as empty.
        assert_eq!(
            CellValue::from_columns(K::Number, text(), None, None, None, None),
            None
        );
        assert_eq!(
            CellValue::from_columns(K::Number, None, Some(f64::NAN), None, None, None),
            None
        );
        assert_eq!(
            CellValue::from_columns(K::Checkbox, None, None, Some(1), None, None),
            Some(CellValue::Checkbox(true))
        );
        // Stored but empty: an unticked box, empty text.
        assert_eq!(
            CellValue::from_columns(K::Checkbox, None, None, Some(0), None, None),
            None
        );
        assert_eq!(
            CellValue::from_columns(K::Text, Some(String::new()), None, None, None, None),
            None
        );
        assert_eq!(
            CellValue::from_columns(K::Date, None, None, None, Some("2026-02-30".into()), None),
            None
        );
        assert_eq!(
            CellValue::from_columns(K::Date, None, None, None, Some("2026-02-28".into()), None)
                .map(|d| matches!(d, CellValue::Date(_))),
            Some(true)
        );
        assert_eq!(
            CellValue::from_columns(K::Select, None, None, None, None, Some("o1".into())),
            Some(CellValue::Select("o1".into()))
        );
    }

    #[test]
    fn typed_input_becomes_a_value_or_explains_why_not() {
        use PropertyKind as K;
        assert_eq!(parse_input(K::Number, "  "), Ok(None));
        assert_eq!(
            parse_input(K::Number, "2,5"),
            Ok(Some(CellValue::Number(2.5)))
        );
        assert_eq!(
            parse_input(K::Text, " con espacios "),
            Ok(Some(CellValue::Text(" con espacios ".into())))
        );
        let err = parse_input(K::Number, "doce").expect_err("not a number");
        assert!(err.contains("«doce»"), "{err}");
        let err = parse_input(K::Date, "30/02/2026").expect_err("no such day");
        assert!(err.contains("AAAA-MM-DD"), "{err}");
        assert!(parse_input(K::Checkbox, "sí").is_err());
    }

    #[test]
    fn options_match_without_case_or_accents() {
        let option = |id: &str, name: &str| SelectOption {
            id: id.into(),
            name: name.into(),
            color: OptionColor::Gray,
        };
        let property = Property {
            id: "p".into(),
            name: "Estado".into(),
            kind: PropertyKind::Select,
            options: vec![
                option("1", "Leído"),
                option("2", "Leyendo"),
                option("3", "Pendiente"),
            ],
        };
        let ids = |(matches, _): (Vec<&SelectOption>, Option<&SelectOption>)| -> Vec<String> {
            matches.iter().map(|o| o.id.clone()).collect()
        };
        assert_eq!(ids(property.matching_options("")), ["1", "2", "3"]);
        assert_eq!(ids(property.matching_options("le")), ["1", "2"]);
        assert_eq!(ids(property.matching_options("LEIDO")), ["1"]);
        assert_eq!(
            property
                .matching_options(" leido ")
                .1
                .map(|o| o.id.as_str()),
            Some("1"),
            "exact, so no «Crear»"
        );
        assert_eq!(property.matching_options("lei").1, None);
        assert!(property.matching_options("nada").0.is_empty());
    }

    #[test]
    fn a_select_shows_its_option_name() {
        let property = Property {
            id: "p".into(),
            name: "Estado".into(),
            kind: PropertyKind::Select,
            options: vec![SelectOption {
                id: "o1".into(),
                name: "Leído".into(),
                color: OptionColor::Green,
            }],
        };
        assert_eq!(CellValue::Select("o1".into()).display(&property), "Leído");
        assert_eq!(CellValue::Select("gone".into()).display(&property), "");
        assert_eq!(CellValue::Number(1.5).display(&property), "1,5");
    }
}
