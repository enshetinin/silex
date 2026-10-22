//! Database to CSV (H11.5): a golden file, a read-back with a small
//! RFC 4180 reader, the view's columns and rows, and writing the file.
use std::{fs, path::PathBuf};

use silex_desktop::csv::{BOM, CsvScope, database_to_csv};
use silex_desktop::export::{CsvExportJob, export_text};
use silex_desktop::model::{Page, PageKind};
use silex_desktop::property::{
    CellValue, Date, OptionColor, Property, PropertyKind, SelectOption, TableValues,
};
use silex_desktop::view::{Direction, Filter, FilterOp, NAME, Sort, ViewConfig};

fn row(id: &str, title: &str) -> Page {
    Page {
        id: id.into(),
        parent_id: Some("db".into()),
        title: title.into(),
        kind: PageKind::Page,
    }
}

fn property(id: &str, name: &str, kind: PropertyKind) -> Property {
    let options = if kind == PropertyKind::Select {
        [("p", "Pendiente"), ("l", "Leído, por fin")]
            .into_iter()
            .map(|(id, name)| SelectOption {
                id: id.into(),
                name: name.into(),
                color: OptionColor::Gray,
            })
            .collect()
    } else {
        Vec::new()
    };
    Property {
        id: id.into(),
        name: name.into(),
        kind,
        options,
    }
}

/// Every type, empty cells and the characters that need quoting.
fn library() -> (Vec<Property>, Vec<Page>, TableValues) {
    let properties = vec![
        property("autor", "Autor", PropertyKind::Text),
        property("paginas", "Páginas", PropertyKind::Number),
        property("leido", "¿Leído?", PropertyKind::Checkbox),
        property("fin", "Terminado", PropertyKind::Date),
        property("estado", "Estado", PropertyKind::Select),
        property("notas", "Notas, \"varias\"", PropertyKind::Text),
    ];
    let rows = vec![
        row("1", "Rayuela"),
        row("2", "Pedro Páramo"),
        row("3", "El \"Aleph\", y otros"),
        row("4", ""),
    ];
    let mut values = TableValues::new();
    let mut set = |row: &str, property: &str, value: CellValue| {
        values
            .entry(row.into())
            .or_default()
            .insert(property.into(), value);
    };
    set("1", "autor", CellValue::Text("Julio Cortázar".into()));
    set("1", "paginas", CellValue::Number(736.0));
    set("1", "leido", CellValue::Checkbox(true));
    set(
        "1",
        "fin",
        CellValue::Date(Date::new(2026, 9, 14).expect("date")),
    );
    set("1", "estado", CellValue::Select("l".into()));
    set(
        "1",
        "notas",
        CellValue::Text("Primera línea\nSegunda".into()),
    );
    set("2", "autor", CellValue::Text("Juan Rulfo".into()));
    set("2", "paginas", CellValue::Number(-0.5));
    set("2", "estado", CellValue::Select("p".into()));
    set("3", "paginas", CellValue::Number(1234.25));
    set("3", "notas", CellValue::Text(" con espacios ".into()));
    // An option that no longer exists reads as empty.
    set("4", "estado", CellValue::Select("borrada".into()));
    (properties, rows, values)
}

/// A minimal RFC 4180 reader: CRLF between records, quoted fields with
/// doubled quotes.
fn read_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') => {
                assert_eq!(chars.next(), Some('\n'), "a lone CR ends a record");
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            (false, c) => {
                assert!(c != '\n', "a lone LF outside quotes");
                field.push(c);
            }
        }
    }
    assert!(
        !quoted && field.is_empty() && record.is_empty(),
        "ends with CRLF"
    );
    records
}

#[test]
fn all_matches_the_golden_file() {
    let (properties, rows, values) = library();
    let csv = database_to_csv(&properties, &rows, &values, CsvScope::All);
    let golden = include_str!("golden/database_sample.csv");
    if csv != golden {
        let out = std::env::temp_dir().join("silex_database_sample.actual.csv");
        let _ = fs::write(&out, &csv);
        panic!("CSV changed; actual output written to {}", out.display());
    }
}

#[test]
fn every_value_reads_back() {
    let (properties, rows, values) = library();
    let csv = database_to_csv(&properties, &rows, &values, CsvScope::All);
    let body = csv.strip_prefix(BOM).expect("starts with the BOM");
    let records = read_csv(body);
    assert_eq!(
        records,
        vec![
            vec![
                "Nombre",
                "Autor",
                "Páginas",
                "¿Leído?",
                "Terminado",
                "Estado",
                "Notas, \"varias\""
            ],
            vec![
                "Rayuela",
                "Julio Cortázar",
                "736",
                "true",
                "2026-09-14",
                "Leído, por fin",
                "Primera línea\nSegunda"
            ],
            vec![
                "Pedro Páramo",
                "Juan Rulfo",
                "-0.5",
                "false",
                "",
                "Pendiente",
                ""
            ],
            vec![
                "El \"Aleph\", y otros",
                "",
                "1234.25",
                "false",
                "",
                "",
                " con espacios "
            ],
            vec!["", "", "", "false", "", "", ""],
        ]
    );
}

#[test]
fn an_empty_database_is_its_header() {
    let csv = database_to_csv(&[], &[], &TableValues::new(), CsvScope::All);
    assert_eq!(csv, format!("{BOM}Nombre\r\n"));
}

#[test]
fn a_view_exports_its_columns_and_its_rows_in_its_order() {
    let (properties, rows, values) = library();
    // Rows with a number of pages, the longest first.
    let mut config = ViewConfig::default();
    config.filters = vec![Filter {
        property: "paginas".into(),
        op: FilterOp::NotEmpty,
        value: String::new(),
    }];
    config.sorts = vec![Sort {
        property: "paginas".into(),
        direction: Direction::Descending,
    }];
    for hidden in ["autor", "leido", "fin", "estado", "notas"] {
        config.set_hidden(hidden, true);
    }
    let csv = database_to_csv(&properties, &rows, &values, CsvScope::View(&config));
    let records = read_csv(csv.strip_prefix(BOM).expect("BOM"));
    assert_eq!(
        records,
        vec![
            vec!["Nombre", "Páginas"],
            vec!["El \"Aleph\", y otros", "1234.25"],
            vec!["Rayuela", "736"],
            vec!["Pedro Páramo", "-0.5"],
        ]
    );
    // «Todo» ignores the view: every row and column, manual order.
    let all = read_csv(
        database_to_csv(&properties, &rows, &values, CsvScope::All)
            .strip_prefix(BOM)
            .expect("BOM"),
    );
    assert_eq!(all.len(), 5);
    assert_eq!(all[0].len(), 7);
    assert_eq!(all[1][0], "Rayuela");
}

#[test]
fn a_view_sorted_by_name_keeps_every_row() {
    let (properties, rows, values) = library();
    let mut config = ViewConfig::default();
    config.sorts = vec![Sort {
        property: NAME.into(),
        direction: Direction::Ascending,
    }];
    let csv = database_to_csv(&properties, &rows, &values, CsvScope::View(&config));
    let names: Vec<String> = read_csv(csv.strip_prefix(BOM).expect("BOM"))
        .into_iter()
        .skip(1)
        .map(|r| r[0].clone())
        .collect();
    assert_eq!(names.len(), 4);
    assert_eq!(names.last().map(String::as_str), Some(""), "{names:?}");
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-csv-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_file_has_a_safe_unique_name_and_the_bytes_as_made() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let (properties, rows, values) = library();
    let csv = database_to_csv(&properties, &rows, &values, CsvScope::All);
    let first = export_text(&tmp.0, "Libros: 2026/27", "csv", &csv)?;
    let second = export_text(&tmp.0, "Libros: 2026/27", "csv", &csv)?;
    assert_eq!(first.parent(), Some(tmp.0.as_path()));
    assert_eq!(
        first.file_name().and_then(|n| n.to_str()),
        Some("Libros_ 2026_27.csv")
    );
    assert_eq!(
        second.file_name().and_then(|n| n.to_str()),
        Some("Libros_ 2026_27 (2).csv")
    );
    let bytes = fs::read(&first)?;
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "UTF-8 BOM");
    assert_eq!(bytes, csv.as_bytes());
    Ok(())
}

#[test]
fn the_job_writes_on_the_export_thread() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen = thread.clone();
    let job = CsvExportJob::spawn_csv(
        tmp.0.clone(),
        "Libros".into(),
        format!("{BOM}Nombre\r\n"),
        move || {
            if let Ok(mut name) = seen.lock() {
                *name = std::thread::current().name().map(str::to_owned);
            }
            let _ = tx.send(());
        },
    )?;
    rx.recv_timeout(std::time::Duration::from_secs(10))?;
    let path = loop {
        if let Some(result) = job.poll() {
            break result?;
        }
        std::thread::yield_now();
    };
    assert_eq!(path, tmp.0.join("Libros.csv"));
    assert_eq!(fs::read_to_string(&path)?, format!("{BOM}Nombre\r\n"));
    let name = thread.lock().map(|n| n.clone()).unwrap_or_default();
    assert_eq!(name.as_deref(), Some("silex-export"));
    Ok(())
}
