//! A database as CSV (H11.5): RFC 4180 with a UTF-8 byte order mark, so
//! Excel reads the accents. Pure: no SQL, no files and no egui.
//!
//! «Nombre» comes first, then one column per property. A date is
//! `AAAA-MM-DD`, a checkbox `true`/`false`, a number with a decimal point
//! (`-0.5`) and a select its option's name: values another program can
//! read back, not the text the app shows.
use crate::model::Page;
use crate::property::{CellValue, Property, PropertyKind, TableValues};
use crate::view::{ViewConfig, visible_rows};

/// The UTF-8 byte order mark that starts the file.
pub const BOM: &str = "\u{feff}";

/// Which part of a database goes into the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvScope<'a> {
    /// What a view shows: its columns (not the hidden ones) and the rows
    /// its filters leave, in its order.
    View(&'a ViewConfig),
    /// Every live property and row, in their manual order.
    All,
}

/// The CSV of a database: `rows` in their manual order, `properties` its
/// live columns in order.
pub fn database_to_csv(
    properties: &[Property],
    rows: &[Page],
    values: &TableValues,
    scope: CsvScope<'_>,
) -> String {
    let (order, columns): (Vec<usize>, Vec<&Property>) = match scope {
        CsvScope::View(config) => (
            visible_rows(rows, values, properties, config),
            properties
                .iter()
                .filter(|p| !config.is_hidden(&p.id))
                .collect(),
        ),
        CsvScope::All => ((0..rows.len()).collect(), properties.iter().collect()),
    };
    let mut out = String::from(BOM);
    let header = std::iter::once("Nombre").chain(columns.iter().map(|p| p.name.as_str()));
    push_record(&mut out, header);
    for row in order.iter().filter_map(|&i| rows.get(i)) {
        let cells = values.get(&row.id);
        let fields: Vec<String> = columns
            .iter()
            .map(|property| field(property, cells.and_then(|c| c.get(&property.id))))
            .collect();
        push_record(
            &mut out,
            std::iter::once(row.title.as_str()).chain(fields.iter().map(String::as_str)),
        );
    }
    out
}

/// One cell's text in the file.
fn field(property: &Property, value: Option<&CellValue>) -> String {
    match (property.kind, value) {
        // A checkbox has no empty value: an empty one is unticked.
        (PropertyKind::Checkbox, v) => matches!(v, Some(CellValue::Checkbox(true))).to_string(),
        (_, Some(CellValue::Number(n))) => format!("{}", n + 0.0), // -0 is 0
        (_, Some(value)) => value.display(property),
        (_, None) => String::new(),
    }
}

/// Writes one line, its fields separated by commas, ended by CRLF.
fn push_record<'a>(out: &mut String, fields: impl Iterator<Item = &'a str>) {
    for (i, text) in fields.enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_field(out, text);
    }
    out.push_str("\r\n");
}

/// A field between double quotes (inner ones doubled) when it holds a
/// comma, a quote or a line break, or starts or ends with a space (which
/// some programs would trim); as it is otherwise.
fn push_field(out: &mut String, text: &str) {
    let quote = text.contains([',', '"', '\n', '\r'])
        || text.starts_with([' ', '\t'])
        || text.ends_with([' ', '\t']);
    if !quote {
        out.push_str(text);
        return;
    }
    out.push('"');
    out.push_str(&text.replace('"', "\"\""));
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(fields: &[&str]) -> String {
        let mut out = String::new();
        push_record(&mut out, fields.iter().copied());
        out
    }

    #[test]
    fn plain_fields_are_written_as_they_are() {
        assert_eq!(line(&["a", "Pedro Páramo", ""]), "a,Pedro Páramo,\r\n");
    }

    #[test]
    fn commas_quotes_and_line_breaks_are_quoted() {
        assert_eq!(line(&["a,b"]), "\"a,b\"\r\n");
        assert_eq!(line(&["dijo \"hola\""]), "\"dijo \"\"hola\"\"\"\r\n");
        assert_eq!(line(&["uno\ndos"]), "\"uno\ndos\"\r\n");
        assert_eq!(line(&["uno\r\ndos"]), "\"uno\r\ndos\"\r\n");
        assert_eq!(line(&[" lado "]), "\" lado \"\r\n");
    }
}
