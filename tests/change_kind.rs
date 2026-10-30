//! Changing a property's type (H13.4, ADR 0019): conversion in one
//! transaction, nothing deleted, and an exact undo.
use silex_desktop::db::{Database, KindChange};
use silex_desktop::property::{CellValue, OptionColor, PropertyKind};
use silex_desktop::view::{Direction, Filter, FilterOp, Sort, ViewConfig};

/// A database «Lecturas» with one row per title, in order.
fn base(titles: &[&str]) -> anyhow::Result<(Database, String, Vec<String>)> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let rows = titles
        .iter()
        .map(|t| db.create_row(&base, t))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok((db, base, rows))
}

/// What each row shows in `property`, in the order of `rows`.
fn shown(
    db: &Database,
    base: &str,
    rows: &[String],
    property: &str,
) -> anyhow::Result<Vec<String>> {
    let properties = db.database_properties(base)?;
    let property = properties
        .iter()
        .find(|p| p.id == property)
        .expect("live property");
    let values = db.database_values(base)?;
    Ok(rows
        .iter()
        .map(|row| {
            values
                .get(row)
                .and_then(|cells| cells.get(&property.id))
                .map(|v| v.display(property))
                .unwrap_or_default()
        })
        .collect())
}

fn kind_of(db: &Database, base: &str, property: &str) -> anyhow::Result<PropertyKind> {
    Ok(db
        .database_properties(base)?
        .into_iter()
        .find(|p| p.id == property)
        .expect("live property")
        .kind)
}

fn text(t: &str) -> CellValue {
    CellValue::Text(t.into())
}

fn change(converted: usize, restored: usize, emptied: usize, new_options: &[&str]) -> KindChange {
    KindChange {
        converted,
        restored,
        emptied,
        new_options: new_options.iter().map(|s| (*s).to_owned()).collect(),
        filters: 0,
    }
}

#[test]
fn what_does_not_fit_is_kept_hidden_and_comes_back_exactly() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["Rayuela", "Ficciones", "Aleph"])?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Text)?;
    db.set_value(&rows[0], &paginas, Some(&text("12.50")))?;
    db.set_value(&rows[1], &paginas, Some(&text("doce")))?;

    let preview = db.preview_kind_change(&paginas, PropertyKind::Number)?;
    assert_eq!(preview, change(1, 0, 1, &[]));
    db.change_property_kind(&paginas, PropertyKind::Number)?;
    assert_eq!(kind_of(&db, &base, &paginas)?, PropertyKind::Number);
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["12,5", "", ""]);

    // Back to text: both come back as they were typed, not as converted.
    let preview = db.preview_kind_change(&paginas, PropertyKind::Text)?;
    assert_eq!(preview, change(0, 2, 0, &[]));
    db.change_property_kind(&paginas, PropertyKind::Text)?;
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["12.50", "doce", ""]);
    Ok(())
}

#[test]
fn an_edited_cell_loses_what_was_hidden_and_converts_again() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["Rayuela"])?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Text)?;
    db.set_value(&rows[0], &paginas, Some(&text("doce")))?;
    db.change_property_kind(&paginas, PropertyKind::Number)?;
    db.set_value(&rows[0], &paginas, Some(&CellValue::Number(7.0)))?;
    assert_eq!(
        db.preview_kind_change(&paginas, PropertyKind::Text)?,
        change(1, 0, 0, &[])
    );
    db.change_property_kind(&paginas, PropertyKind::Text)?;
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["7"]);
    Ok(())
}

#[test]
fn text_becomes_options_found_by_name_or_created_once() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b", "c", "d"])?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Text)?;
    for (row, value) in rows.iter().zip(["Leído", " leido ", "Pendiente"]) {
        db.set_value(row, &estado, Some(&text(value)))?;
    }
    let preview = db.preview_kind_change(&estado, PropertyKind::Select)?;
    assert_eq!(preview, change(3, 0, 0, &["Leído", "Pendiente"]));
    db.change_property_kind(&estado, PropertyKind::Select)?;
    let property = db
        .database_properties(&base)?
        .into_iter()
        .find(|p| p.id == estado)
        .expect("live");
    let names: Vec<&str> = property.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["Leído", "Pendiente"]);
    // Colors in turn, as when they are created by hand.
    assert_eq!(property.options[0].color, OptionColor::for_new_option(0));
    assert_eq!(property.options[1].color, OptionColor::for_new_option(1));
    assert_eq!(
        shown(&db, &base, &rows, &estado)?,
        ["Leído", "Leído", "Pendiente", ""]
    );

    // A list of names into a multi-select, reusing the options it has.
    let generos = db.add_property(&base, "Géneros", PropertyKind::Text)?;
    db.add_property(&base, "Otra", PropertyKind::Select)?;
    db.set_value(&rows[0], &generos, Some(&text("Novela, cuento")))?;
    db.set_value(&rows[1], &generos, Some(&text("Cuento,Ensayo, novela")))?;
    assert_eq!(
        db.preview_kind_change(&generos, PropertyKind::MultiSelect)?,
        change(2, 0, 0, &["Novela", "cuento", "Ensayo"])
    );
    db.change_property_kind(&generos, PropertyKind::MultiSelect)?;
    assert_eq!(
        shown(&db, &base, &rows, &generos)?,
        ["Novela, cuento", "Novela, cuento, Ensayo", "", ""]
    );
    Ok(())
}

#[test]
fn selects_keep_their_options_both_ways() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b"])?;
    let generos = db.add_property(&base, "Géneros", PropertyKind::MultiSelect)?;
    let novela = db.add_select_option(&generos, "Novela", OptionColor::Blue)?;
    let cuento = db.add_select_option(&generos, "Cuento", OptionColor::Green)?;
    db.set_value(
        &rows[0],
        &generos,
        Some(&CellValue::MultiSelect(vec![
            cuento.clone(),
            novela.clone(),
        ])),
    )?;
    db.set_value(
        &rows[1],
        &generos,
        Some(&CellValue::MultiSelect(vec![cuento])),
    )?;
    // To a single select: the first option, in the property's order.
    assert_eq!(
        db.preview_kind_change(&generos, PropertyKind::Select)?,
        change(2, 0, 0, &[])
    );
    db.change_property_kind(&generos, PropertyKind::Select)?;
    assert_eq!(shown(&db, &base, &rows, &generos)?, ["Novela", "Cuento"]);
    // Back: every option comes back.
    assert_eq!(
        db.preview_kind_change(&generos, PropertyKind::MultiSelect)?,
        change(0, 2, 0, &[])
    );
    db.change_property_kind(&generos, PropertyKind::MultiSelect)?;
    assert_eq!(
        shown(&db, &base, &rows, &generos)?,
        ["Novela, Cuento", "Cuento"]
    );

    // A select into a multi-select: the same option.
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let leido = db.add_select_option(&estado, "Leído", OptionColor::Blue)?;
    db.set_value(&rows[0], &estado, Some(&CellValue::Select(leido.clone())))?;
    db.change_property_kind(&estado, PropertyKind::MultiSelect)?;
    assert_eq!(
        db.database_values(&base)?[&rows[0]][&estado],
        CellValue::MultiSelect(vec![leido])
    );
    Ok(())
}

#[test]
fn filters_leave_the_views_and_undo_puts_everything_back() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b", "c"])?;
    let estado = db.add_property(&base, "Estado", PropertyKind::MultiSelect)?;
    let otra = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let leido = db.add_select_option(&estado, "Leído", OptionColor::Blue)?;
    let pendiente = db.add_select_option(&estado, "Pendiente", OptionColor::Red)?;
    db.set_value(
        &rows[0],
        &estado,
        Some(&CellValue::MultiSelect(vec![
            leido.clone(),
            pendiente.clone(),
        ])),
    )?;
    db.set_value(
        &rows[1],
        &estado,
        Some(&CellValue::MultiSelect(vec![pendiente])),
    )?;
    let view = db.database_views(&base)?[0].id.clone();
    let mut config = ViewConfig::default();
    config.sorts = vec![Sort {
        property: estado.clone(),
        direction: Direction::Descending,
    }];
    config.filters = vec![
        Filter {
            property: estado.clone(),
            op: FilterOp::Contains,
            value: leido.clone(),
        },
        Filter {
            property: otra.clone(),
            op: FilterOp::Contains,
            value: "Borges".into(),
        },
    ];
    db.set_view_config(&view, &config)?;
    let values = db.database_values(&base)?;
    let properties = db.database_properties(&base)?;

    // To text, then (a new transaction) to a select with new options.
    let preview = db.preview_kind_change(&estado, PropertyKind::Text)?;
    assert_eq!(preview.filters, 1);
    let to_text = db.change_property_kind(&estado, PropertyKind::Text)?;
    assert_eq!(to_text.kind(), PropertyKind::MultiSelect);
    assert_eq!(to_text.property(), estado);
    let config_now = db.database_views(&base)?[0].config.clone();
    assert_eq!(config_now.filters.len(), 1, "only the other property's");
    assert_eq!(config_now.filters[0].property, otra);
    assert_eq!(config_now.sorts, config.sorts, "sorts stay");
    assert_eq!(
        shown(&db, &base, &rows, &estado)?,
        ["Leído, Pendiente", "Pendiente", ""]
    );
    db.set_value(&rows[2], &estado, Some(&text("Abandonado")))?;
    let to_select = db.change_property_kind(&estado, PropertyKind::Select)?;

    // Undone in reverse order: exactly as it was.
    db.restore_property_kind(&to_select)?;
    assert_eq!(kind_of(&db, &base, &estado)?, PropertyKind::Text);
    assert_eq!(
        shown(&db, &base, &rows, &estado)?,
        ["Leído, Pendiente", "Pendiente", "Abandonado"]
    );
    db.set_value(&rows[2], &estado, None)?;
    db.restore_property_kind(&to_text)?;
    assert_eq!(
        db.database_properties(&base)?,
        properties,
        "kind and options"
    );
    assert_eq!(db.database_values(&base)?, values);
    assert_eq!(db.database_views(&base)?[0].config, config);
    Ok(())
}

#[test]
fn undo_deletes_the_options_it_created_and_keeps_hidden_values() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b"])?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    db.set_value(&rows[0], &paginas, Some(&CellValue::Number(12.5)))?;
    db.change_property_kind(&paginas, PropertyKind::Text)?;
    db.change_property_kind(&paginas, PropertyKind::Number)?;
    let before = db.database_values(&base)?;
    let snapshot = db.change_property_kind(&paginas, PropertyKind::Select)?;
    let options = |db: &Database| -> anyhow::Result<usize> {
        Ok(db
            .database_properties(&base)?
            .into_iter()
            .find(|p| p.id == paginas)
            .map_or(0, |p| p.options.len()))
    };
    assert_eq!(options(&db)?, 1, "«12,5»");
    db.restore_property_kind(&snapshot)?;
    assert_eq!(db.database_values(&base)?, before);
    db.change_property_kind(&paginas, PropertyKind::Select)?;
    assert_eq!(
        options(&db)?,
        1,
        "the undone option is gone, not reused twice"
    );
    // The text of the first change is still there underneath.
    db.change_property_kind(&paginas, PropertyKind::Text)?;
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["12,5", ""]);
    Ok(())
}

#[test]
fn created_and_edited_hide_stored_values_and_give_their_dates_as_text() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b"])?;
    let nota = db.add_property(&base, "Nota", PropertyKind::Text)?;
    db.set_value(&rows[0], &nota, Some(&text("importante")))?;
    assert_eq!(
        db.preview_kind_change(&nota, PropertyKind::Created)?,
        change(0, 0, 1, &[])
    );
    db.change_property_kind(&nota, PropertyKind::Created)?;
    let values = db.database_values(&base)?;
    assert!(
        rows.iter()
            .all(|row| matches!(values[row][&nota], CellValue::Created(_)))
    );
    // Back to text: what was typed comes back; the other row gets its date.
    assert_eq!(
        db.preview_kind_change(&nota, PropertyKind::Text)?,
        change(1, 1, 0, &[])
    );
    db.change_property_kind(&nota, PropertyKind::Text)?;
    let shown_now = shown(&db, &base, &rows, &nota)?;
    assert_eq!(shown_now[0], "importante");
    let created = values[&rows[1]][&nota].moment().expect("a date");
    assert_eq!(shown_now[1], created.to_string());
    // «Editado» did not move: a type change does not edit the rows.
    let editado = db.add_property(&base, "Editado", PropertyKind::Edited)?;
    let edited = db.database_values(&base)?[&rows[1]][&editado].clone();
    db.set_clock(Some("2030-06-15T12:00:00.000Z"));
    db.change_property_kind(&nota, PropertyKind::Date)?;
    assert_eq!(db.database_values(&base)?[&rows[1]][&editado], edited);
    Ok(())
}

#[test]
fn rows_in_the_trash_are_converted_too() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a", "b"])?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Text)?;
    db.set_value(&rows[1], &paginas, Some(&text("5")))?;
    db.trash_page(&rows[1])?;
    assert_eq!(
        db.preview_kind_change(&paginas, PropertyKind::Number)?,
        change(1, 0, 0, &[])
    );
    db.change_property_kind(&paginas, PropertyKind::Number)?;
    db.restore_page(&rows[1])?;
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["", "5"]);
    Ok(())
}

#[test]
fn a_change_that_cannot_be_made_changes_nothing() -> anyhow::Result<()> {
    let (mut db, base, rows) = base(&["a"])?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Text)?;
    db.set_value(&rows[0], &paginas, Some(&text("3")))?;
    let err = db
        .change_property_kind(&paginas, PropertyKind::Text)
        .expect_err("same type");
    assert!(format!("{err:#}").contains("ya es de tipo"), "{err:#}");
    assert!(
        db.preview_kind_change("nada", PropertyKind::Number)
            .is_err()
    );
    db.delete_property(&paginas)?;
    assert!(
        db.change_property_kind(&paginas, PropertyKind::Number)
            .is_err()
    );
    db.restore_property(&paginas)?;
    assert_eq!(kind_of(&db, &base, &paginas)?, PropertyKind::Text);
    assert_eq!(shown(&db, &base, &rows, &paginas)?, ["3"]);
    Ok(())
}

#[test]
fn a_change_survives_reopening() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("silex-kind-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("silex.sqlite3");
    let result = (|| -> anyhow::Result<()> {
        let (base, rows, estado) = {
            let mut db = Database::open(&path)?;
            let base = db.create_database("Lecturas", None)?;
            let rows = vec![db.create_row(&base, "a")?];
            let estado = db.add_property(&base, "Estado", PropertyKind::Text)?;
            db.set_value(&rows[0], &estado, Some(&text("Leído")))?;
            db.change_property_kind(&estado, PropertyKind::Select)?;
            (base, rows, estado)
        };
        let db = Database::open(&path)?;
        assert_eq!(kind_of(&db, &base, &estado)?, PropertyKind::Select);
        assert_eq!(shown(&db, &base, &rows, &estado)?, ["Leído"]);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Manual: `cargo test --release --test change_kind scale -- --ignored --nocapture`.
#[test]
#[ignore = "timing; run manually"]
fn scale_of_a_type_change() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Grande", None)?;
    let numero = db.add_property(&base, "Número", PropertyKind::Text)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Text)?;
    for i in 0..10_000 {
        let row = db.create_row(&base, &format!("Fila {i}"))?;
        db.set_value(&row, &numero, Some(&text(&i.to_string())))?;
        db.set_value(&row, &estado, Some(&text(&format!("Estado {}", i % 50))))?;
    }
    for (property, to) in [
        (&numero, PropertyKind::Number),
        (&numero, PropertyKind::Text),
        (&estado, PropertyKind::Select),
    ] {
        let t = std::time::Instant::now();
        let preview = db.preview_kind_change(property, to)?;
        let previewed = t.elapsed();
        let t = std::time::Instant::now();
        let snapshot = db.change_property_kind(property, to)?;
        let changed = t.elapsed();
        let t = std::time::Instant::now();
        db.restore_property_kind(&snapshot)?;
        let undone = t.elapsed();
        db.change_property_kind(property, to)?;
        println!(
            "10000 filas → {}: vista previa {previewed:?}, cambio {changed:?}, deshacer {undone:?} ({} convertidas, {} vuelven, {} opciones)",
            to.label(),
            preview.converted,
            preview.restored,
            preview.new_options.len()
        );
    }
    Ok(())
}
