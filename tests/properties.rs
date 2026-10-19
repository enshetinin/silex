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

// ---- H6.2b: options and the values of one row ----

#[test]
fn options_are_renamed_and_recolored_without_touching_cells() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let leido = db.add_select_option(&estado, "Leido", OptionColor::Gray)?;
    db.set_value(&row, &estado, Some(&CellValue::Select(leido.clone())))?;

    db.rename_select_option(&leido, "Leído")?;
    db.set_option_color(&leido, OptionColor::Green)?;
    let option = db.database_properties(&base)?[0].options[0].clone();
    assert_eq!(
        (option.name.as_str(), option.color),
        ("Leído", OptionColor::Green)
    );
    assert_eq!(
        cell(&db, &base, &row, &estado)?,
        Some(CellValue::Select(leido.clone())),
        "the cell keeps pointing at it"
    );
    assert!(db.rename_select_option("no-existe", "x").is_err());
    assert!(db.set_option_color("no-existe", OptionColor::Red).is_err());
    Ok(())
}

#[test]
fn deleting_an_option_empties_its_cells_only() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let first = db.create_row(&base, "Rayuela")?;
    let second = db.create_row(&base, "Ficciones")?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let leido = db.add_select_option(&estado, "Leído", OptionColor::Green)?;
    let pendiente = db.add_select_option(&estado, "Pendiente", OptionColor::Gray)?;
    db.set_value(&first, &estado, Some(&CellValue::Select(leido.clone())))?;
    db.set_value(
        &second,
        &estado,
        Some(&CellValue::Select(pendiente.clone())),
    )?;

    db.delete_select_option(&leido)?;
    assert_eq!(cell(&db, &base, &first, &estado)?, None, "emptied");
    assert_eq!(
        cell(&db, &base, &second, &estado)?,
        Some(CellValue::Select(pendiente)),
        "other options stay"
    );
    let names: Vec<String> = db.database_properties(&base)?[0]
        .options
        .iter()
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(names, ["Pendiente"]);
    assert!(db.delete_select_option(&leido).is_err(), "already gone");
    // The emptied cell takes a new value as usual.
    let nueva = db.add_select_option(&estado, "Releído", OptionColor::Blue)?;
    db.set_value(&first, &estado, Some(&CellValue::Select(nueva.clone())))?;
    assert_eq!(
        cell(&db, &base, &first, &estado)?,
        Some(CellValue::Select(nueva))
    );
    Ok(())
}

#[test]
fn row_values_are_the_values_of_one_live_row() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    let other = db.create_row(&base, "Ficciones")?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    db.set_value(&row, &autor, Some(&CellValue::Text("Cortázar".into())))?;
    db.set_value(&row, &paginas, Some(&CellValue::Number(736.0)))?;
    db.set_value(&other, &autor, Some(&CellValue::Text("Borges".into())))?;

    let values = db.row_values(&row)?;
    assert_eq!(values.len(), 2);
    assert_eq!(
        values.get(&autor),
        Some(&CellValue::Text("Cortázar".into()))
    );
    // A deleted property's value is kept but not shown.
    db.delete_property(&paginas)?;
    assert_eq!(db.row_values(&row)?.len(), 1);
    // A normal page, a trashed row or an unknown id have none.
    let page = db.create_page("Notas", None)?;
    assert!(db.row_values(&page)?.is_empty());
    assert!(db.row_values("no-existe")?.is_empty());
    db.trash_page(&row)?;
    assert!(db.row_values(&row)?.is_empty());
    Ok(())
}

// ---- H6.3a: a view's sort, filters and hidden properties ----

#[test]
fn a_views_config_is_stored_and_survives_damage() -> anyhow::Result<()> {
    use silex_desktop::view::{Direction, Filter, FilterOp, NAME, Sort, ViewConfig};
    let dir = std::env::temp_dir().join(format!("silex-views-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("silex.sqlite3");
    let result = (|| -> anyhow::Result<()> {
        let (base, view, config) = {
            let mut db = Database::open(&path)?;
            let base = db.create_database("Lecturas", None)?;
            let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
            let views = db.database_views(&base)?;
            assert_eq!(
                views[0].config,
                ViewConfig::default(),
                "a new view shows all"
            );
            let mut config = ViewConfig::default();
            config.sorts.push(Sort {
                property: NAME.into(),
                direction: Direction::Descending,
            });
            config.filters.push(Filter {
                property: paginas.clone(),
                op: FilterOp::Greater,
                value: "100".into(),
            });
            config.set_hidden(&paginas, true);
            db.set_view_config(&views[0].id, &config)?;
            (base, views[0].id.clone(), config)
        };
        let db = Database::open(&path)?;
        assert_eq!(
            db.database_views(&base)?[0].config,
            config,
            "after reopening"
        );
        assert!(db.set_view_config("no-existe", &config).is_err());
        drop(db);

        // Written by hand (another tool, a bug): the database still opens.
        let conn = rusqlite::Connection::open(&path)?;
        conn.execute(
            "UPDATE db_views SET config = 'no es json' WHERE id = ?1",
            [&view],
        )?;
        drop(conn);
        let db = Database::open(&path)?;
        let views = db.database_views(&base)?;
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].config, ViewConfig::default());
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

// ---- H6.3b: several views per database ----

#[test]
fn views_are_added_renamed_moved_and_deleted_but_never_the_last() -> anyhow::Result<()> {
    use silex_desktop::view::{FilterOp, ViewConfig};
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let first = db.database_views(&base)?[0].id.clone();
    let names = |db: &Database| -> anyhow::Result<Vec<String>> {
        Ok(db
            .database_views(&base)?
            .into_iter()
            .map(|v| v.name)
            .collect())
    };

    // A copy keeps the settings of the view it copies.
    let mut config = ViewConfig::default();
    config.set_hidden("x", true);
    config.filters.push(silex_desktop::view::Filter {
        property: silex_desktop::view::NAME.into(),
        op: FilterOp::Contains,
        value: "a".into(),
    });
    let copy = db.add_view(&base, "Tabla (copia)", &config)?;
    let empty = db.add_view(&base, "Tabla 2", &ViewConfig::default())?;
    assert_eq!(names(&db)?, ["Tabla", "Tabla (copia)", "Tabla 2"]);
    assert_eq!(db.database_views(&base)?[1].config, config);

    db.rename_view(&empty, "Pendientes")?;
    assert!(db.move_view(&empty, Shift::Up)?);
    assert!(!db.move_view(&first, Shift::Up)?, "already first");
    assert_eq!(names(&db)?, ["Tabla", "Pendientes", "Tabla (copia)"]);

    db.delete_view(&copy)?;
    db.delete_view(&first)?;
    assert_eq!(names(&db)?, ["Pendientes"]);
    let err = db.delete_view(&empty).expect_err("the last view stays");
    assert!(format!("{err:#}").contains("última"), "{err:#}");
    assert_eq!(names(&db)?, ["Pendientes"]);

    // Unknown ids and pages that are not databases are refused.
    assert!(db.rename_view("no-existe", "x").is_err());
    assert!(db.delete_view("no-existe").is_err());
    let page = db.create_page("Notas", None)?;
    assert!(db.add_view(&page, "x", &ViewConfig::default()).is_err());
    Ok(())
}
