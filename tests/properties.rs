//! Typed properties (H6.2, ADR 0006): the columns of a database and the
//! values of its rows.
use silex_desktop::db::{Database, Shift};
use silex_desktop::property::{CellValue, Date, OptionColor, Property, PropertyKind};

fn names(properties: &[Property]) -> Vec<&str> {
    properties.iter().map(|p| p.name.as_str()).collect()
}

/// The value of one cell, as the table reads it.
fn cell(db: &Database, base: &str, row: &str, property: &str) -> anyhow::Result<Option<CellValue>> {
    Ok(db
        .database_values(base)?
        .get(row)
        .and_then(|values| values.get(property))
        .cloned())
}

fn date(input: &str) -> CellValue {
    CellValue::Date(Date::parse(input).expect("valid date"))
}

#[test]
fn properties_are_added_renamed_and_moved_in_order() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    assert!(db.database_properties(&base)?.is_empty(), "only Nombre");
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let properties = db.database_properties(&base)?;
    assert_eq!(names(&properties), ["Estado", "Páginas", "Leído"]);
    assert_eq!(
        properties.iter().map(|p| p.kind).collect::<Vec<_>>(),
        [
            PropertyKind::Select,
            PropertyKind::Number,
            PropertyKind::Checkbox
        ]
    );

    db.rename_property(&paginas, "Nº de páginas")?;
    assert!(db.move_property(&leido, Shift::Up)?);
    assert_eq!(
        names(&db.database_properties(&base)?),
        ["Estado", "Leído", "Nº de páginas"]
    );
    assert!(!db.move_property(&estado, Shift::Up)?, "already first");
    assert!(!db.move_property(&paginas, Shift::Down)?, "already last");
    assert!(db.move_property(&estado, Shift::Down)?);
    assert_eq!(
        names(&db.database_properties(&base)?),
        ["Leído", "Estado", "Nº de páginas"]
    );

    // Each database has its own columns.
    let other = db.create_database("Proyectos", None)?;
    assert!(db.database_properties(&other)?.is_empty());
    Ok(())
}

#[test]
fn properties_only_go_into_live_databases() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Notas", None)?;
    assert!(
        db.add_property(&page, "Estado", PropertyKind::Text)
            .is_err()
    );
    assert!(
        db.add_property("no-existe", "Estado", PropertyKind::Text)
            .is_err()
    );
    let base = db.create_database("Lecturas", None)?;
    db.trash_page(&base)?;
    assert!(
        db.add_property(&base, "Estado", PropertyKind::Text)
            .is_err()
    );
    assert!(db.rename_property("no-existe", "x").is_err());
    assert!(db.delete_property("no-existe").is_err());
    assert!(db.move_property("no-existe", Shift::Up).is_err());
    db.restore_page(&base)?;
    assert!(
        db.database_properties(&base)?.is_empty(),
        "nothing was added"
    );
    Ok(())
}

#[test]
fn each_kind_keeps_its_own_value() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let fecha = db.add_property(&base, "Terminado", PropertyKind::Date)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let terminado = db.add_select_option(&estado, "Terminado", OptionColor::Green)?;

    let values = [
        (&autor, CellValue::Text("Julio Cortázar".into())),
        (&paginas, CellValue::Number(736.5)),
        (&leido, CellValue::Checkbox(true)),
        (&fecha, date("1963-06-28")),
        (&estado, CellValue::Select(terminado.clone())),
    ];
    for (property, value) in &values {
        db.set_value(&row, property, Some(value))?;
    }
    for (property, value) in &values {
        assert_eq!(cell(&db, &base, &row, property)?.as_ref(), Some(value));
    }

    // Overwrite, then empty in each way: no value, empty text, unticked box.
    db.set_value(&row, &paginas, Some(&CellValue::Number(-2.0)))?;
    assert_eq!(
        cell(&db, &base, &row, &paginas)?,
        Some(CellValue::Number(-2.0))
    );
    db.set_value(&row, &paginas, None)?;
    db.set_value(&row, &autor, Some(&CellValue::Text(String::new())))?;
    db.set_value(&row, &leido, Some(&CellValue::Checkbox(false)))?;
    for property in [&paginas, &autor, &leido] {
        assert_eq!(cell(&db, &base, &row, property)?, None);
    }
    assert_eq!(db.database_values(&base)?[&row].len(), 2, "date and select");

    // A row with no values is not in the map at all.
    let empty = db.create_row(&base, "Ficciones")?;
    assert!(!db.database_values(&base)?.contains_key(&empty));
    Ok(())
}

#[test]
fn values_must_match_their_property_and_row() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let otra = db.add_property(&base, "Formato", PropertyKind::Select)?;
    let ajena = db.add_select_option(&otra, "Tapa dura", OptionColor::Gray)?;

    let rejected = [
        (&row, &paginas, CellValue::Text("doce".into()), "wrong kind"),
        (&row, &paginas, CellValue::Number(f64::NAN), "NaN"),
        (&row, &paginas, CellValue::Number(f64::INFINITY), "infinite"),
        (
            &row,
            &estado,
            CellValue::Select(ajena.clone()),
            "other property's option",
        ),
        (
            &row,
            &estado,
            CellValue::Select("no-existe".into()),
            "unknown option",
        ),
    ];
    for (row, property, value, why) in rejected {
        assert!(db.set_value(row, property, Some(&value)).is_err(), "{why}");
    }

    // Only rows of the property's own database, live.
    let page = db.create_page("Notas", None)?;
    let other_base = db.create_database("Proyectos", None)?;
    let other_row = db.create_row(&other_base, "Silex")?;
    let trashed = db.create_row(&base, "Borrada")?;
    db.trash_page(&trashed)?;
    let n = CellValue::Number(1.0);
    for (target, why) in [
        (&page, "a normal page"),
        (&other_row, "a row of another database"),
        (&base, "the database itself"),
        (&trashed, "a trashed row"),
    ] {
        assert!(db.set_value(target, &paginas, Some(&n)).is_err(), "{why}");
    }
    assert!(db.set_value(&row, "no-existe", Some(&n)).is_err());

    assert!(db.database_values(&base)?.is_empty(), "nothing was stored");
    Ok(())
}

#[test]
fn a_deleted_property_keeps_its_values_and_comes_back_last() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    db.set_value(&row, &autor, Some(&CellValue::Text("Cortázar".into())))?;

    db.delete_property(&autor)?;
    assert_eq!(names(&db.database_properties(&base)?), ["Páginas"]);
    assert_eq!(names(&db.deleted_properties(&base)?), ["Autor"]);
    assert_eq!(cell(&db, &base, &row, &autor)?, None, "hidden with it");
    let text = CellValue::Text("otro".into());
    assert!(db.set_value(&row, &autor, Some(&text)).is_err());
    assert!(db.rename_property(&autor, "Escritor").is_err());
    assert!(db.delete_property(&autor).is_err(), "already deleted");

    db.restore_property(&autor)?;
    assert_eq!(names(&db.database_properties(&base)?), ["Páginas", "Autor"]);
    assert!(db.deleted_properties(&base)?.is_empty());
    assert_eq!(
        cell(&db, &base, &row, &autor)?,
        Some(CellValue::Text("Cortázar".into()))
    );
    assert!(db.restore_property(&autor).is_err(), "not deleted");
    assert!(db.restore_property(&paginas).is_err(), "never deleted");
    Ok(())
}

#[test]
fn values_follow_their_rows_to_the_trash_and_back() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let n = CellValue::Number(736.0);
    db.set_value(&row, &paginas, Some(&n))?;

    db.trash_page(&row)?;
    assert!(db.database_values(&base)?.is_empty());
    db.restore_page(&row)?;
    assert_eq!(cell(&db, &base, &row, &paginas)?.as_ref(), Some(&n));

    // The whole database, and back.
    db.trash_page(&base)?;
    db.restore_page(&base)?;
    assert_eq!(names(&db.database_properties(&base)?), ["Páginas"]);
    assert_eq!(cell(&db, &base, &row, &paginas)?.as_ref(), Some(&n));

    // A row moved out is a normal page: its values leave the table.
    db.move_page(&row, None)?;
    assert!(db.database_values(&base)?.is_empty());
    assert!(db.set_value(&row, &paginas, Some(&n)).is_err());
    Ok(())
}

#[test]
fn select_options_belong_to_select_properties() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    db.add_select_option(&estado, "Pendiente", OptionColor::Gray)?;
    db.add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
    let options: Vec<(String, OptionColor)> = db.database_properties(&base)?[0]
        .options
        .iter()
        .map(|o| (o.name.clone(), o.color))
        .collect();
    assert_eq!(
        options,
        [
            ("Pendiente".to_owned(), OptionColor::Gray),
            ("Leyendo".to_owned(), OptionColor::Blue)
        ]
    );

    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    assert!(db.add_select_option(&autor, "x", OptionColor::Red).is_err());
    db.delete_property(&estado)?;
    assert!(
        db.add_select_option(&estado, "x", OptionColor::Red)
            .is_err()
    );
    // Deleted properties keep their options for when they come back.
    assert_eq!(db.deleted_properties(&base)?[0].options.len(), 2);
    Ok(())
}

#[test]
fn properties_and_values_survive_reopening_the_file() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("silex-properties-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("silex.sqlite3");
    let result = (|| -> anyhow::Result<()> {
        let (base, row, fecha) = {
            let mut db = Database::open(&path)?;
            let base = db.create_database("Lecturas", None)?;
            let row = db.create_row(&base, "Rayuela")?;
            let fecha = db.add_property(&base, "Terminado", PropertyKind::Date)?;
            db.set_value(&row, &fecha, Some(&date("2026-10-19")))?;
            (base, row, fecha)
        };
        let db = Database::open(&path)?;
        assert_eq!(names(&db.database_properties(&base)?), ["Terminado"]);
        assert_eq!(cell(&db, &base, &row, &fecha)?, Some(date("2026-10-19")));
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}
