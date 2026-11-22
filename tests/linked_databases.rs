use silex_desktop::{
    db::Database,
    model::{Block, BlockKind},
    property::{CellValue, PropertyKind},
    view::{Filter, FilterOp, NAME, ViewConfig, visible_rows},
};

fn link(page: &str, target: &str, id: &str) -> Block {
    Block {
        id: id.into(),
        page_id: page.into(),
        kind: BlockKind::PageLink,
        content: String::new(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: Some(target.into()),
        marks: Vec::new(),
    }
}

#[test]
fn linked_views_share_rows_but_keep_their_config_after_reopening() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("silex-linked-{}.sqlite3", uuid::Uuid::new_v4()));
    let (page, base, row, property, blocks);
    {
        let mut db = Database::open(&path)?;
        page = db.create_page("Proyecto", None)?;
        base = db.create_database("Tareas", None)?;
        row = db.create_row(&base, "Escribir")?;
        property = db.add_property(&base, "Hecho", PropertyKind::Checkbox)?;
        blocks = vec![link(&page, &base, "one"), link(&page, &base, "two")];
        db.save_document(&page, "Proyecto", &blocks)?;
        let mut one = ViewConfig::default();
        one.filters.push(Filter {
            property: NAME.into(),
            op: FilterOp::Contains,
            value: "Escribir".into(),
        });
        one.set_column_width(NAME, 415.0);
        one.set_column_width(&property, 140.0);
        db.set_linked_view_config("one", &base, &one)?;
        let mut two = ViewConfig::default();
        two.filters.push(Filter {
            property: NAME.into(),
            op: FilterOp::Contains,
            value: "Otra".into(),
        });
        db.set_linked_view_config("two", &base, &two)?;
        db.set_value(&row, &property, Some(&CellValue::Checkbox(true)))?;
        assert_eq!(db.database_views(&base)?[0].config, ViewConfig::default());
    }
    {
        let mut db = Database::open(&path)?;
        let rows = db.database_rows(&base)?;
        assert_eq!(rows.len(), 1, "no copies of the source rows");
        let properties = db.database_properties(&base)?;
        let values = db.database_values(&base)?;
        let one = db.linked_view_config("one", &base)?;
        assert_eq!(one.column_width(NAME, 80.0), 415.0);
        assert_eq!(visible_rows(&rows, &values, &properties, &one), [0]);
        assert!(
            visible_rows(
                &rows,
                &values,
                &properties,
                &db.linked_view_config("two", &base)?
            )
            .is_empty()
        );
        assert_eq!(values[&row][&property], CellValue::Checkbox(true));
        db.save_document(&page, "Proyecto", &[])?;
        assert_eq!(
            db.database_rows(&base)?.len(),
            1,
            "removing views preserves data"
        );
        db.save_document(&page, "Proyecto", &blocks)?;
        assert_eq!(
            db.linked_view_config("one", &base)?,
            one,
            "undo recovers the view config"
        );
        let copy = db.duplicate_page(&page, false)?;
        let copied = db.list_blocks(&copy)?;
        assert_eq!(db.linked_view_config(&copied[0].id, &base)?, one);
    }
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn relations_validate_targets_follow_names_and_survive_trash_and_duplication() -> anyhow::Result<()>
{
    let mut db = Database::open_in_memory()?;
    let tasks = db.create_database("Tareas", None)?;
    let projects = db.create_database("Proyectos", None)?;
    let task = db.create_row(&tasks, "Diseñar")?;
    let project = db.create_row(&projects, "Casa")?;
    let unrelated = db.create_page("No es proyecto", None)?;
    let relation = db.add_relation(&tasks, "Proyecto", &projects)?;
    assert_eq!(
        db.relation_target(&relation)?.as_deref(),
        Some(projects.as_str())
    );
    let value = CellValue::Relation(vec![project.clone()]);
    db.set_value(&task, &relation, Some(&value))?;
    assert_eq!(db.row_values(&task)?[&relation], value);
    let invalid = CellValue::Relation(vec![project.clone(), unrelated]);
    assert!(db.set_value(&task, &relation, Some(&invalid)).is_err());
    assert_eq!(
        db.row_values(&task)?[&relation],
        value,
        "invalid replacement rolls back"
    );
    assert!(
        db.set_value(&project, &relation, Some(&value)).is_err(),
        "wrong source database"
    );
    db.rename_page(&project, "Nueva casa")?;
    let props = db.database_properties(&tasks)?;
    assert_eq!(value.display(&props[0]), "Nueva casa");
    db.trash_page(&project)?;
    assert!(!db.row_values(&task)?.contains_key(&relation));
    assert!(db.set_value(&task, &relation, Some(&value)).is_err());
    db.restore_page(&project)?;
    assert_eq!(db.row_values(&task)?[&relation], value);
    let copy = db.duplicate_page(&task, false)?;
    assert_eq!(db.row_values(&copy)?[&relation], value);
    let copied_db = db.duplicate_page(&tasks, true)?;
    let copied_property = &db.database_properties(&copied_db)?[0];
    assert_eq!(
        db.relation_target(&copied_property.id)?.as_deref(),
        Some(projects.as_str())
    );
    let copied_row = &db.database_rows(&copied_db)?[0];
    assert_eq!(db.row_values(&copied_row.id)?[&copied_property.id], value);
    let filter = Filter {
        property: relation.clone(),
        op: FilterOp::Contains,
        value: project,
    };
    let mut config = ViewConfig::default();
    config.filters.push(filter);
    assert_eq!(
        visible_rows(
            &db.database_rows(&tasks)?,
            &db.database_values(&tasks)?,
            &props,
            &config
        )
        .len(),
        2
    );
    assert!(
        db.change_property_kind(&relation, PropertyKind::Text)
            .is_err()
    );
    Ok(())
}

#[test]
fn widths_reject_malformed_values_and_preserve_unknown_settings() {
    let mut config =
        ViewConfig::from_json(r#"{"widths":{"name":-10,"bad":"wide","huge":99999},"future":true}"#);
    assert_eq!(config.column_width(NAME, 240.0), 240.0);
    assert_eq!(config.column_width("huge", 120.0), 120.0);
    config.set_column_width(NAME, 450.0);
    config.set_column_width("other", f32::NAN);
    let saved = config.to_json();
    assert!(saved.contains("\"future\":true"));
    assert_eq!(
        ViewConfig::from_json(&saved).column_width(NAME, 240.0),
        450.0
    );
}
