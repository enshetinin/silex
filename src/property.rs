//! Typed database properties (H6.2, ADR 0006): their kinds, the colors of
//! select options, cell values and how text typed in a cell becomes a value.
//! Pure: no SQL and no egui.
use std::{collections::HashMap, fmt};

use crate::editor::fold;
use crate::marks::normalize_url;

/// The type of a property. Stored as text; see [`PropertyKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyKind {
    Text,
    Number,
    Checkbox,
    Date,
    Select,
    /// Zero or more options of the property (H13.2, ADR 0018).
    MultiSelect,
    /// A web address, opened with a click (H13.3).
    Url,
    /// When the row was created: read only, from its page (H13.3).
    Created,
    /// When the row was last edited: read only, from its page (H13.3).
    Edited,
}

impl PropertyKind {
    /// Menu order.
    pub const ALL: [PropertyKind; 9] = [
        PropertyKind::Text,
        PropertyKind::Number,
        PropertyKind::Checkbox,
        PropertyKind::Date,
        PropertyKind::Select,
        PropertyKind::MultiSelect,
        PropertyKind::Url,
        PropertyKind::Created,
        PropertyKind::Edited,
    ];

    /// Name stored in SQLite (must match the CHECK in migrations 0005 and
    /// 0014).
    pub fn as_str(self) -> &'static str {
        match self {
            PropertyKind::Text => "text",
            PropertyKind::Number => "number",
            PropertyKind::Checkbox => "checkbox",
            PropertyKind::Date => "date",
            PropertyKind::Select => "select",
            PropertyKind::MultiSelect => "multi_select",
            PropertyKind::Url => "url",
            PropertyKind::Created => "created",
            PropertyKind::Edited => "edited",
        }
    }

    /// Whether its cells take options of the property (and it has some).
    pub fn has_options(self) -> bool {
        matches!(self, PropertyKind::Select | PropertyKind::MultiSelect)
    }

    /// Whether Silex fills its cells from the row's page (when it was
    /// created or last edited): they are read only and never stored.
    pub fn is_computed(self) -> bool {
        matches!(self, PropertyKind::Created | PropertyKind::Edited)
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
            PropertyKind::MultiSelect => "Selección múltiple",
            PropertyKind::Url => "URL",
            PropertyKind::Created => "Creado",
            PropertyKind::Edited => "Editado",
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

    /// Name shown in the UI.
    pub fn label(self) -> &'static str {
        match self {
            OptionColor::Gray => "Gris",
            OptionColor::Blue => "Azul",
            OptionColor::Green => "Verde",
            OptionColor::Yellow => "Amarillo",
            OptionColor::Orange => "Naranja",
            OptionColor::Red => "Rojo",
            OptionColor::Purple => "Morado",
            OptionColor::Pink => "Rosa",
        }
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

/// A typed column of a database. `options` is empty unless it is a select
/// or a multi-select.
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

impl Date {
    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// Day of the week: 0 is Monday, 6 is Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday (3).
        (self.days_since_epoch() + 3).rem_euclid(7) as u8
    }

    /// Today by the system clock, in UTC (the standard library has no
    /// local time zone). `None` if the clock is before 1970 or past 9999.
    pub fn today_utc() -> Option<Self> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        Self::from_days_since_epoch(i64::try_from(seconds / 86_400).ok()?)
    }

    /// Days from 1970-01-01 (negative before), by H. Hinnant's
    /// `days_from_civil`.
    fn days_since_epoch(self) -> i64 {
        let (month, day) = (i64::from(self.month), i64::from(self.day));
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The inverse of [`Self::days_since_epoch`] (`civil_from_days`).
    fn from_days_since_epoch(days: i64) -> Option<Self> {
        let days = days + 719_468;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self::new(
            u16::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
    }
}

/// A month as a calendar shows it (the date picker of a date cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarMonth {
    year: u16,
    month: u8,
}

const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

impl CalendarMonth {
    /// The month a date is in.
    pub fn of(date: Date) -> Self {
        Self {
            year: date.year,
            month: date.month,
        }
    }

    /// The month before; January of year 1 stays where it is.
    pub fn previous(self) -> Self {
        match (self.year, self.month) {
            (1, 1) => self,
            (year, 1) => Self {
                year: year - 1,
                month: 12,
            },
            (year, month) => Self {
                year,
                month: month - 1,
            },
        }
    }

    /// The month after; December of 9999 stays where it is.
    pub fn next(self) -> Self {
        match (self.year, self.month) {
            (9999, 12) => self,
            (year, 12) => Self {
                year: year + 1,
                month: 1,
            },
            (year, month) => Self {
                year,
                month: month + 1,
            },
        }
    }

    /// «octubre de 2026».
    pub fn label(self) -> String {
        let name = MONTHS[usize::from(self.month - 1)];
        format!("{name} de {}", self.year)
    }

    /// Its weeks, Monday first: each day in its place, and `None` where a
    /// week starts before the month or ends after it. Four to six weeks.
    pub fn weeks(self) -> Vec<[Option<Date>; 7]> {
        let days = days_in_month(self.year, self.month);
        let Some(first) = Date::new(self.year, self.month, 1) else {
            return Vec::new();
        };
        let mut weeks = Vec::new();
        let mut week = [None; 7];
        let mut slot = usize::from(first.weekday());
        for day in 1..=days {
            week[slot] = Date::new(self.year, self.month, day);
            slot += 1;
            if slot == 7 {
                weeks.push(week);
                week = [None; 7];
                slot = 0;
            }
        }
        if slot > 0 {
            weeks.push(week);
        }
        weeks
    }
}

/// The name of a date, as a screen reader says it: «19 de octubre de 2026».
pub fn spoken_date(date: Date) -> String {
    format!("{} de {}", date.day, CalendarMonth::of(date).label())
}

/// `AAAA-MM-DD`, the format SQLite stores and checks.
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A date and time to the minute in the system's time zone
/// (`AAAA-MM-DD HH:MM`): when a row was created or last edited (H13.3).
/// Ordered in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment {
    date: Date,
    hour: u8,
    minute: u8,
}

impl Moment {
    /// Reads `AAAA-MM-DD HH:MM`, as SQLite's `strftime` writes it.
    pub fn parse(input: &str) -> Option<Self> {
        let (date, time) = input.trim().split_once(' ')?;
        let (hour, minute) = time.split_once(':')?;
        let two_digits = |part: &str| -> Option<u8> {
            (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        };
        let (hour, minute) = (two_digits(hour)?, two_digits(minute)?);
        // Only `AAAA-MM-DD`: `Date::parse` also reads `D/M/AAAA`.
        let iso = date.len() == 10 && date.as_bytes()[4] == b'-';
        (hour < 24 && minute < 60 && iso).then_some(Self {
            date: Date::parse(date)?,
            hour,
            minute,
        })
    }

    pub fn date(self) -> Date {
        self.date
    }
}

/// `AAAA-MM-DD HH:MM`.
impl fmt::Display for Moment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:02}:{:02}", self.date, self.hour, self.minute)
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
    /// Ids of the property's options, in the property's order (ADR 0018).
    /// No ids is an empty cell.
    MultiSelect(Vec<String>),
    /// What was typed: a web address if [`CellValue::web_url`] says so.
    Url(String),
    /// Read from the row's page, never stored (H13.3).
    Created(Moment),
    Edited(Moment),
}

impl CellValue {
    pub fn kind(&self) -> PropertyKind {
        match self {
            CellValue::Text(_) => PropertyKind::Text,
            CellValue::Number(_) => PropertyKind::Number,
            CellValue::Checkbox(_) => PropertyKind::Checkbox,
            CellValue::Date(_) => PropertyKind::Date,
            CellValue::Select(_) => PropertyKind::Select,
            CellValue::MultiSelect(_) => PropertyKind::MultiSelect,
            CellValue::Url(_) => PropertyKind::Url,
            CellValue::Created(_) => PropertyKind::Created,
            CellValue::Edited(_) => PropertyKind::Edited,
        }
    }

    /// Values stored as "no value": empty text, an unticked box and a
    /// multi-select with no options.
    pub fn is_empty(&self) -> bool {
        match self {
            CellValue::Text(text) | CellValue::Url(text) => text.is_empty(),
            CellValue::Checkbox(on) => !on,
            CellValue::MultiSelect(ids) => ids.is_empty(),
            _ => false,
        }
    }

    /// The address a URL cell opens: only web (`http`, `https`) and mail
    /// (`mailto:`) links, as in a block's text (ADR 0015). `None` for
    /// anything else, which is shown but never opened.
    pub fn web_url(&self) -> Option<&str> {
        match self {
            CellValue::Url(url) if normalize_url(url).as_deref() == Some(url.as_str()) => Some(url),
            _ => None,
        }
    }

    /// The date and time of a «Creado» or «Editado» cell.
    pub fn moment(&self) -> Option<Moment> {
        match self {
            CellValue::Created(moment) | CellValue::Edited(moment) => Some(*moment),
            _ => None,
        }
    }

    /// The options of a multi-select (none for any other value or none).
    pub fn option_ids(value: Option<&CellValue>) -> &[String] {
        match value {
            Some(CellValue::MultiSelect(ids)) => ids,
            _ => &[],
        }
    }

    /// A click on `option` in a multi-select's menu: `current` with it
    /// added, or taken out if it was there. `None` if no option is left.
    pub fn toggled(property: &Property, current: Option<&CellValue>, option: &str) -> Option<Self> {
        let mut ids = Self::option_ids(current).to_vec();
        if ids.iter().any(|id| id == option) {
            ids.retain(|id| id != option);
        } else {
            ids.push(option.to_owned());
        }
        Self::multi_select(property, &ids)
    }

    /// `current` with `option` added (a new option created from the cell).
    pub fn with_option(
        property: &Property,
        current: Option<&CellValue>,
        option: &str,
    ) -> Option<Self> {
        let mut ids = Self::option_ids(current).to_vec();
        ids.push(option.to_owned());
        Self::multi_select(property, &ids)
    }

    /// A multi-select of `ids`: those that are options of `property`, once
    /// each and in the property's order. `None` if none is left.
    pub fn multi_select(property: &Property, ids: &[String]) -> Option<Self> {
        let ids: Vec<String> = property
            .options
            .iter()
            .filter(|o| ids.contains(&o.id))
            .map(|o| o.id.clone())
            .collect();
        (!ids.is_empty()).then_some(CellValue::MultiSelect(ids))
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
            PropertyKind::Url => CellValue::Url(text?),
            // Not in `db_values` (ADR 0018): read from `db_multi_values`.
            PropertyKind::MultiSelect => return None,
            // Not stored: read from the row's page.
            PropertyKind::Created | PropertyKind::Edited => return None,
        };
        (!value.is_empty()).then_some(value)
    }

    /// Text for the cell and for screen readers. A select shows its
    /// option's name (empty if the option is gone).
    pub fn display(&self, property: &Property) -> String {
        match self {
            CellValue::Text(text) | CellValue::Url(text) => text.clone(),
            CellValue::Number(n) => format_number(*n),
            CellValue::Checkbox(true) => "Sí".to_owned(),
            CellValue::Checkbox(false) => "No".to_owned(),
            CellValue::Date(date) => date.to_string(),
            CellValue::Select(id) => property
                .option(id)
                .map(|o| o.name.clone())
                .unwrap_or_default(),
            // «Novela, Cuento», in the property's order.
            CellValue::MultiSelect(ids) => property
                .options
                .iter()
                .filter(|o| ids.contains(&o.id))
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            CellValue::Created(moment) | CellValue::Edited(moment) => moment.to_string(),
        }
    }
}

/// The values of a database's rows: row id → property id → value. Empty
/// cells are absent.
pub type TableValues = HashMap<String, HashMap<String, CellValue>>;

/// Text typed in a cell of a text, number, date or URL property, turned
/// into its value. `Ok(None)` empties the cell; `Err` explains what is
/// expected. An address without a scheme (`ejemplo.com`) gets `https://`;
/// anything else that is not a web address is kept as typed (it is shown,
/// but not opened).
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
        PropertyKind::Url => {
            let typed = input.trim();
            let url = normalize_url(typed).unwrap_or_else(|| typed.to_owned());
            Ok(Some(CellValue::Url(url)))
        }
        PropertyKind::Checkbox | PropertyKind::Select | PropertyKind::MultiSelect => Err(format!(
            "Una {} no se escribe.",
            kind.label().to_lowercase()
        )),
        PropertyKind::Created | PropertyKind::Edited => {
            Err(format!("«{}» lo pone Silex: no se escribe.", kind.label()))
        }
    }
}

/// An option of a converted cell (H13.4, ADR 0019): one the property
/// already has, or the name of one to find or create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionRef {
    Id(String),
    Name(String),
}

/// A cell's value in its property's new type, before it is written
/// (H13.4, ADR 0019). Only the column of the new type is written.
#[derive(Debug, Clone, PartialEq)]
pub enum Converted {
    /// `text_value`, for a text or a URL.
    Text(String),
    Number(f64),
    Checkbox(bool),
    Date(Date),
    /// A select's option (the first) or a multi-select's options: never
    /// empty, no names repeated.
    Options(Vec<OptionRef>),
}

/// Reads a box from text: «sí», «x», «true», «1», «✓» or «[x]» is ticked;
/// «no», «false», «0» or «[ ]» is not; ignoring case and accents. Anything
/// else is not a box.
pub fn parse_checkbox(input: &str) -> Option<bool> {
    match fold(input.trim()).as_str() {
        "si" | "x" | "true" | "1" | "✓" | "[x]" | "yes" => Some(true),
        "no" | "false" | "0" | "[ ]" => Some(false),
        _ => None,
    }
}

/// What `value`, a cell of `from`, becomes when the property changes to
/// type `to` (ADR 0019): mostly the text the cell shows, read as the new
/// type. `None` if it does not fit (the cell is left empty, its value kept
/// hidden), for an empty value, and always for «Creado» and «Editado»,
/// which are not stored.
pub fn convert(value: &CellValue, from: &Property, to: PropertyKind) -> Option<Converted> {
    if value.is_empty() || to.is_computed() {
        return None;
    }
    let text = value.display(from);
    match to {
        PropertyKind::Text | PropertyKind::Url => {
            (!text.is_empty()).then_some(Converted::Text(text))
        }
        PropertyKind::Number => match value {
            CellValue::Checkbox(on) => Some(Converted::Number(if *on { 1.0 } else { 0.0 })),
            _ => parse_number(&text).map(Converted::Number),
        },
        PropertyKind::Date => match value {
            CellValue::Date(date) => Some(Converted::Date(*date)),
            CellValue::Created(moment) | CellValue::Edited(moment) => {
                Some(Converted::Date(moment.date()))
            }
            _ => Date::parse(&text).map(Converted::Date),
        },
        PropertyKind::Checkbox => match value {
            CellValue::Checkbox(on) => Some(Converted::Checkbox(*on)),
            _ => parse_checkbox(&text).map(Converted::Checkbox),
        },
        PropertyKind::Select => match value {
            CellValue::Select(id) => Some(Converted::Options(vec![OptionRef::Id(id.clone())])),
            // Its first option, in the property's order.
            CellValue::MultiSelect(ids) => ids
                .first()
                .map(|id| Converted::Options(vec![OptionRef::Id(id.clone())])),
            _ => named([text.as_str()]),
        },
        PropertyKind::MultiSelect => match value {
            CellValue::Select(id) => Some(Converted::Options(vec![OptionRef::Id(id.clone())])),
            CellValue::MultiSelect(ids) => (!ids.is_empty()).then(|| {
                Converted::Options(ids.iter().map(|id| OptionRef::Id(id.clone())).collect())
            }),
            // «Novela, Cuento», as a multi-select is written in text.
            CellValue::Text(_) | CellValue::Url(_) => named(text.split(',')),
            _ => named([text.as_str()]),
        },
        PropertyKind::Created | PropertyKind::Edited => None,
    }
}

/// Options by name: trimmed, without empty ones or repeats (ignoring case
/// and accents). `None` if no name is left.
fn named<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Converted> {
    let mut seen = Vec::new();
    let mut options = Vec::new();
    for name in names {
        let name = name.trim();
        let key = fold(name);
        if !name.is_empty() && !seen.contains(&key) {
            seen.push(key);
            options.push(OptionRef::Name(name.to_owned()));
        }
    }
    (!options.is_empty()).then_some(Converted::Options(options))
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
    fn weekdays_and_days_since_1970_are_right() {
        let date = |s: &str| Date::parse(s).expect("date");
        // 2026-10-19 is a Monday; 1970-01-01 a Thursday; 2000-02-29 a Tuesday.
        assert_eq!(date("2026-10-19").weekday(), 0);
        assert_eq!(date("2026-10-25").weekday(), 6);
        assert_eq!(date("1970-01-01").weekday(), 3);
        assert_eq!(date("2000-02-29").weekday(), 1);
        assert_eq!(date("0001-01-01").weekday(), 0);
        for s in [
            "1970-01-01",
            "1969-12-31",
            "2000-02-29",
            "0001-01-01",
            "9999-12-31",
            "2026-10-19",
        ] {
            let d = date(s);
            assert_eq!(
                Date::from_days_since_epoch(d.days_since_epoch()),
                Some(d),
                "{s}"
            );
        }
        assert_eq!(date("1970-01-02").days_since_epoch(), 1);
        assert_eq!(Date::from_days_since_epoch(i64::MAX / 2), None);
        let today = Date::today_utc().expect("a clock after 1970");
        assert!(today >= date("2026-01-01"));
    }

    #[test]
    fn a_calendar_month_lays_out_its_weeks_from_monday() {
        let month = CalendarMonth::of(Date::parse("2026-10-19").expect("date"));
        assert_eq!(month.label(), "octubre de 2026");
        let weeks = month.weeks();
        // October 2026 starts on a Thursday and ends on a Saturday.
        assert_eq!(weeks.len(), 5);
        assert_eq!(weeks[0][..3], [None, None, None]);
        assert_eq!(weeks[0][3].map(Date::day), Some(1));
        assert_eq!(weeks[4][5].map(Date::day), Some(31));
        assert_eq!(weeks[4][6], None);
        let days: Vec<u8> = weeks.iter().flatten().flatten().map(|d| d.day()).collect();
        assert_eq!(days, (1..=31).collect::<Vec<u8>>());
        // February 2027 starts on a Monday: exactly four weeks.
        let february = CalendarMonth::of(Date::parse("2027-02-01").expect("date"));
        assert_eq!(february.weeks().len(), 4);
        // Months go round the year and stop at the ends of the calendar.
        assert_eq!(month.next().next().next().label(), "enero de 2027");
        assert_eq!(february.previous().previous().label(), "diciembre de 2026");
        let first = CalendarMonth::of(Date::parse("0001-01-01").expect("date"));
        assert_eq!(first.previous(), first);
        let last = CalendarMonth::of(Date::parse("9999-12-31").expect("date"));
        assert_eq!(last.next(), last);
        assert_eq!(
            spoken_date(Date::parse("1/5/2026").expect("date")),
            "1 de mayo de 2026"
        );
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

    fn generos() -> Property {
        Property {
            id: "g".into(),
            name: "Géneros".into(),
            kind: PropertyKind::MultiSelect,
            options: ["novela", "cuento", "ensayo"]
                .into_iter()
                .map(|id| SelectOption {
                    id: id.into(),
                    name: id.to_uppercase(),
                    color: OptionColor::Gray,
                })
                .collect(),
        }
    }

    fn multi(ids: &[&str]) -> CellValue {
        CellValue::MultiSelect(ids.iter().map(|id| (*id).to_owned()).collect())
    }

    #[test]
    fn a_click_adds_or_takes_out_an_option_in_the_property_order() {
        let property = generos();
        let one = CellValue::toggled(&property, None, "ensayo");
        assert_eq!(one, Some(multi(&["ensayo"])));
        let two = CellValue::toggled(&property, one.as_ref(), "novela");
        assert_eq!(two, Some(multi(&["novela", "ensayo"])));
        let back = CellValue::toggled(&property, two.as_ref(), "novela");
        assert_eq!(back, one);
        assert_eq!(CellValue::toggled(&property, one.as_ref(), "ensayo"), None);
        // Unknown ids are left out; adding one twice keeps one.
        assert_eq!(CellValue::toggled(&property, None, "otra"), None);
        assert_eq!(
            CellValue::with_option(&property, one.as_ref(), "ensayo"),
            one
        );
        assert_eq!(
            CellValue::with_option(&property, one.as_ref(), "cuento"),
            Some(multi(&["cuento", "ensayo"]))
        );
    }

    #[test]
    fn a_multi_select_shows_its_names_and_empty_is_no_value() {
        let property = generos();
        assert_eq!(
            multi(&["novela", "ensayo"]).display(&property),
            "NOVELA, ENSAYO"
        );
        assert!(multi(&[]).is_empty());
        assert_eq!(multi(&["cuento"]).kind(), PropertyKind::MultiSelect);
        assert!(PropertyKind::MultiSelect.has_options());
        assert!(!PropertyKind::Text.has_options());
        // Never in `db_values`.
        assert_eq!(
            CellValue::from_columns(
                PropertyKind::MultiSelect,
                None,
                None,
                None,
                None,
                Some("x".into())
            ),
            None
        );
        assert!(parse_input(PropertyKind::MultiSelect, "x").is_err());
    }

    #[test]
    fn a_url_gets_https_and_only_web_addresses_open() {
        let url = |input: &str| match parse_input(PropertyKind::Url, input) {
            Ok(Some(CellValue::Url(url))) => url,
            other => panic!("{input:?}: {other:?}"),
        };
        assert_eq!(url(" ejemplo.com/a "), "https://ejemplo.com/a");
        assert_eq!(url("http://a.es"), "http://a.es");
        assert_eq!(url("mailto:ana@a.es"), "mailto:ana@a.es");
        // Kept as typed, but never opened.
        assert_eq!(url(" file:///etc/passwd "), "file:///etc/passwd");
        assert_eq!(url("no es una web"), "no es una web");
        assert_eq!(parse_input(PropertyKind::Url, "  "), Ok(None));
        let opens = |text: &str| CellValue::Url(text.into()).web_url().map(str::to_owned);
        assert_eq!(
            opens("https://a.es/x?y=1"),
            Some("https://a.es/x?y=1".into())
        );
        assert_eq!(opens("mailto:ana@a.es"), Some("mailto:ana@a.es".into()));
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ejemplo.com",
            "no es una web",
            "",
        ] {
            assert_eq!(opens(refused), None, "{refused:?}");
        }
        assert_eq!(CellValue::Text("https://a.es".into()).web_url(), None);
        assert!(CellValue::Url(String::new()).is_empty());
        assert_eq!(
            CellValue::from_columns(
                PropertyKind::Url,
                Some("https://a.es".into()),
                None,
                None,
                None,
                None
            ),
            Some(CellValue::Url("https://a.es".into()))
        );
    }

    #[test]
    fn moments_read_and_show_to_the_minute_in_order() {
        let moment = |s: &str| Moment::parse(s).expect(s);
        assert_eq!(moment("2026-10-30 09:05").to_string(), "2026-10-30 09:05");
        assert_eq!(
            moment("2026-10-30 23:59").date(),
            Date::parse("2026-10-30").expect("date")
        );
        assert!(moment("2026-10-30 09:05") < moment("2026-10-30 10:00"));
        assert!(moment("2026-10-30 23:59") < moment("2026-10-31 00:00"));
        assert!(moment("2025-12-31 23:59") < moment("2026-01-01 00:00"));
        for bad in [
            "",
            "2026-10-30",
            "2026-10-30 24:00",
            "2026-10-30 12:60",
            "2026-10-30 9:05",
            "2026-02-30 10:00",
            "30/10/2026 10:00",
            "2026-10-30T10:00",
            "2026-10-30 10:00:00",
        ] {
            assert_eq!(Moment::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn created_and_edited_are_read_only_and_never_stored() {
        let at = Moment::parse("2026-10-30 14:05").expect("moment");
        for kind in [PropertyKind::Created, PropertyKind::Edited] {
            assert!(kind.is_computed());
            assert!(!kind.has_options());
            let err = parse_input(kind, "2026-10-30 14:05").expect_err("read only");
            assert!(err.contains(kind.label()), "{err}");
            assert_eq!(
                CellValue::from_columns(
                    kind,
                    Some("x".into()),
                    Some(1.0),
                    Some(1),
                    Some("2026-10-30".into()),
                    None
                ),
                None
            );
        }
        assert!(!PropertyKind::Url.is_computed());
        let property = Property {
            id: "c".into(),
            name: "Creado".into(),
            kind: PropertyKind::Created,
            options: Vec::new(),
        };
        let created = CellValue::Created(at);
        assert_eq!(created.display(&property), "2026-10-30 14:05");
        assert_eq!(created.kind(), PropertyKind::Created);
        assert_eq!(CellValue::Edited(at).kind(), PropertyKind::Edited);
        assert_eq!(created.moment(), Some(at));
        assert!(!created.is_empty());
        assert_eq!(CellValue::Url("x".into()).moment(), None);
    }

    // ---- H13.4, ADR 0019: changing a property's type ----

    fn of(kind: PropertyKind) -> Property {
        let mut property = generos();
        property.kind = kind;
        property
    }

    fn name(n: &str) -> OptionRef {
        OptionRef::Name(n.into())
    }

    fn id(i: &str) -> OptionRef {
        OptionRef::Id(i.into())
    }

    #[test]
    fn every_value_becomes_the_text_it_shows() {
        use PropertyKind as K;
        let at = Moment::parse("2026-10-30 14:05").expect("moment");
        let date = Date::parse("2026-10-30").expect("date");
        let cases = [
            (K::Number, CellValue::Number(-12.5), "-12,5"),
            (K::Checkbox, CellValue::Checkbox(true), "Sí"),
            (K::Date, CellValue::Date(date), "2026-10-30"),
            (K::Select, CellValue::Select("cuento".into()), "CUENTO"),
            (
                K::MultiSelect,
                multi(&["novela", "ensayo"]),
                "NOVELA, ENSAYO",
            ),
            (
                K::Url,
                CellValue::Url("https://a.es".into()),
                "https://a.es",
            ),
            (K::Created, CellValue::Created(at), "2026-10-30 14:05"),
            (K::Edited, CellValue::Edited(at), "2026-10-30 14:05"),
            (K::Url, CellValue::Url(" x ".into()), " x "),
        ];
        for (from, value, shown) in cases {
            for to in [K::Text, K::Url] {
                assert_eq!(
                    convert(&value, &of(from), to),
                    Some(Converted::Text(shown.into())),
                    "{from:?} → {to:?}"
                );
            }
        }
        // An option that is gone shows nothing: nothing to convert.
        assert_eq!(
            convert(&CellValue::Select("gone".into()), &of(K::Select), K::Text),
            None
        );
    }

    #[test]
    fn numbers_dates_and_boxes_are_read_from_the_text() {
        use PropertyKind as K;
        let text = |t: &str| CellValue::Text(t.into());
        let to = |value: CellValue, from: K, to: K| convert(&value, &of(from), to);
        assert_eq!(
            to(text(" 12,5 "), K::Text, K::Number),
            Some(Converted::Number(12.5))
        );
        assert_eq!(to(text("doce"), K::Text, K::Number), None);
        assert_eq!(
            to(CellValue::Checkbox(true), K::Checkbox, K::Number),
            Some(Converted::Number(1.0))
        );
        assert_eq!(
            to(CellValue::Select("x".into()), K::Select, K::Number),
            None,
            "an option that is gone"
        );
        let date = Date::parse("2026-10-30").expect("date");
        assert_eq!(
            to(text("30/10/2026"), K::Text, K::Date),
            Some(Converted::Date(date))
        );
        assert_eq!(to(text("mañana"), K::Text, K::Date), None);
        assert_eq!(to(CellValue::Number(3.0), K::Number, K::Date), None);
        let at = Moment::parse("2026-10-30 23:59").expect("moment");
        assert_eq!(
            to(CellValue::Edited(at), K::Edited, K::Date),
            Some(Converted::Date(date))
        );
        for (input, on) in [
            ("Sí", true),
            ("X", true),
            ("true", true),
            ("[x]", true),
            ("no", false),
            ("0", false),
        ] {
            assert_eq!(
                to(text(input), K::Text, K::Checkbox),
                Some(Converted::Checkbox(on)),
                "{input}"
            );
        }
        assert_eq!(to(text("quizá"), K::Text, K::Checkbox), None);
        assert_eq!(
            to(CellValue::Number(1.0), K::Number, K::Checkbox),
            Some(Converted::Checkbox(true))
        );
        assert_eq!(
            to(CellValue::Number(0.0), K::Number, K::Checkbox),
            Some(Converted::Checkbox(false))
        );
        assert_eq!(to(CellValue::Number(2.0), K::Number, K::Checkbox), None);
        assert_eq!(to(CellValue::Date(date), K::Date, K::Checkbox), None);
    }

    #[test]
    fn options_keep_their_ids_or_come_by_name() {
        use PropertyKind as K;
        let options = |refs: Vec<OptionRef>| Some(Converted::Options(refs));
        let to = |value: CellValue, from: K, to: K| convert(&value, &of(from), to);
        // Between selects, the same options.
        assert_eq!(
            to(
                CellValue::Select("cuento".into()),
                K::Select,
                K::MultiSelect
            ),
            options(vec![id("cuento")])
        );
        assert_eq!(
            to(multi(&["novela", "ensayo"]), K::MultiSelect, K::Select),
            options(vec![id("novela")])
        );
        // Anything else, by the text it shows.
        assert_eq!(
            to(CellValue::Text(" Leído ".into()), K::Text, K::Select),
            options(vec![name("Leído")])
        );
        assert_eq!(
            to(CellValue::Number(3.0), K::Number, K::Select),
            options(vec![name("3")])
        );
        assert_eq!(
            to(CellValue::Checkbox(true), K::Checkbox, K::MultiSelect),
            options(vec![name("Sí")])
        );
        // Text into a multi-select: split by commas, once each.
        assert_eq!(
            to(
                CellValue::Text("Novela, cuento ,, NOVELA,Ensayo".into()),
                K::Text,
                K::MultiSelect
            ),
            options(vec![name("Novela"), name("cuento"), name("Ensayo")])
        );
        assert_eq!(
            to(CellValue::Text(" , ".into()), K::Text, K::MultiSelect),
            None
        );
        // A comma inside a single select's name stays.
        assert_eq!(
            to(CellValue::Text("Sí, claro".into()), K::Text, K::Select),
            options(vec![name("Sí, claro")])
        );
    }

    #[test]
    fn nothing_converts_into_created_or_edited_nor_from_empty() {
        use PropertyKind as K;
        for to in [K::Created, K::Edited] {
            assert_eq!(
                convert(&CellValue::Text("x".into()), &of(K::Text), to),
                None
            );
        }
        assert_eq!(
            convert(&CellValue::Text(String::new()), &of(K::Text), K::Number),
            None
        );
        assert_eq!(
            convert(&CellValue::Checkbox(false), &of(K::Checkbox), K::Text),
            None
        );
        assert_eq!(convert(&multi(&[]), &of(K::MultiSelect), K::Select), None);
        assert_eq!(parse_checkbox(" SÍ "), Some(true));
        assert_eq!(parse_checkbox(""), None);
    }
}
