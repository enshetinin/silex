//! Duplicating a page, a row or a database (H11.4).
use rusqlite::Connection;
use silex_desktop::attachment::NewAttachment;
use silex_desktop::db::Database;
use silex_desktop::model::{Block, BlockKind, PageKind, ViewKind};
use silex_desktop::property::{CellValue, OptionColor, PropertyKind};
use silex_desktop::view::{Direction, Filter, FilterOp, Sort, ViewConfig};

fn block(page: &str, kind: BlockKind, content: &str) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
    }
}

fn titles(db: &Database, parent: Option<&str>) -> anyhow::Result<Vec<String>> {
    Ok(db
        .list_pages()?
        .into_iter()
        .filter(|p| p.parent_id.as_deref() == parent)
        .map(|p| p.title)
        .collect())
}

fn foreign_keys_hold(path: &std::path::Path) -> anyhow::Result<()> {
    let conn = Connection::open(path)?;
    let broken: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert!(broken.is_empty(), "foreign keys: {broken:?}");
    Ok(())
}

#[test]
fn a_page_is_copied_below_itself_with_new_blocks_and_the_same_files_and_links() -> anyhow::Result<()>
{
    let mut db = Database::open_in_memory()?;
    let notas = db.create_page("Notas", None)?;
    let recetas = db.create_page("Recetas", None)?;
    db.create_page("Trabajo", None)?;
    db.set_favorite(&notas, true)?;
    let file = db.add_attachment(&NewAttachment::from_bytes(
        "acta.pdf",
        b"%PDF-1.7".to_vec(),
    )?)?;
    let blocks = [
        block(&notas, BlockKind::Heading1, "Ideas"),
        Block {
            attachment: Some(file.id.clone()),
            ..block(&notas, BlockKind::File, "acta.pdf")
        },
        Block {
            link: Some(recetas.clone()),
            ..block(&notas, BlockKind::PageLink, "Recetas")
        },
    ];
    db.save_document(&notas, "Notas", &blocks)?;
    let before = db.space_usage()?;

    let copy = db.duplicate_page(&notas, false)?;
    assert_eq!(
        titles(&db, None)?,
        ["Notas", "Notas (copia)", "Recetas", "Trabajo"],
        "right below"
    );
    let copied = db.list_blocks(&copy)?;
    assert_eq!(copied.len(), 3);
    for (new, old) in copied.iter().zip(&blocks) {
        assert_ne!(new.id, old.id);
        assert_eq!(
            (new.kind, new.content.as_str(), &new.attachment, &new.link),
            (old.kind, old.content.as_str(), &old.attachment, &old.link)
        );
    }
    // The file is shared, not stored twice; the copy is no favorite.
    let after = db.space_usage()?;
    assert_eq!(after.used_attachments, before.used_attachments);
    assert!(db.list_favorites()?.iter().all(|p| p.id != copy));
    // The original is untouched, and so is its own history.
    assert_eq!(db.list_blocks(&notas)?.len(), 3);
    assert!(db.page_versions(&copy)?.is_empty());
    Ok(())
}

#[test]
fn subpages_come_along_only_when_asked() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let viaje = db.create_page("Viaje", None)?;
    let islandia = db.create_page("Islandia", Some(&viaje))?;
    db.create_page("Mapas", Some(&islandia))?;
    db.create_page("Noruega", Some(&viaje))?;

    let alone = db.duplicate_page(&viaje, false)?;
    assert!(titles(&db, Some(&alone))?.is_empty());

    let whole = db.duplicate_page(&viaje, true)?;
    assert_eq!(
        titles(&db, Some(&whole))?,
        ["Islandia", "Noruega"],
        "in order"
    );
    let new_islandia = db
        .list_pages()?
        .into_iter()
        .find(|p| p.parent_id.as_deref() == Some(whole.as_str()) && p.title == "Islandia")
        .expect("copied");
    assert_ne!(new_islandia.id, islandia);
    assert_eq!(titles(&db, Some(&new_islandia.id))?, ["Mapas"]);
    // The originals keep theirs.
    assert_eq!(titles(&db, Some(&viaje))?, ["Islandia", "Noruega"]);
    assert_eq!(
        titles(&db, None)?,
        ["Viaje", "Viaje (copia)", "Viaje (copia)"],
        "each copy right below the original"
    );
    Ok(())
}

#[test]
fn a_row_is_copied_with_its_values_right_below_it() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let rayuela = db.create_row(&base, "Rayuela")?;
    db.create_row(&base, "Ficciones")?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let leyendo = db.add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
    db.set_value(&rayuela, &autor, Some(&CellValue::Text("Cortázar".into())))?;
    db.set_value(&rayuela, &estado, Some(&CellValue::Select(leyendo.clone())))?;

    let copy = db.duplicate_page(&rayuela, false)?;
    let rows: Vec<String> = db
        .database_rows(&base)?
        .into_iter()
        .map(|p| p.title)
        .collect();
    assert_eq!(rows, ["Rayuela", "Rayuela (copia)", "Ficciones"]);
    let values = db.row_values(&copy)?;
    assert_eq!(
        values.get(&autor),
        Some(&CellValue::Text("Cortázar".into()))
    );
    assert_eq!(values.get(&estado), Some(&CellValue::Select(leyendo)));
    assert_eq!(db.get_page(&copy)?.map(|p| p.kind), Some(PageKind::Page));
    Ok(())
}

#[test]
fn a_database_is_copied_with_its_schema_and_views_and_rows_if_asked() -> anyhow::Result<()> {
    let path =
        std::env::temp_dir().join(format!("silex-duplicate-{}.sqlite3", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut db = Database::open(&path)?;
        let base = db.create_database("Lecturas", None)?;
        let rayuela = db.create_row(&base, "Rayuela")?;
        let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
        let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
        let leyendo = db.add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
        db.set_value(&rayuela, &estado, Some(&CellValue::Select(leyendo.clone())))?;
        db.set_value(&rayuela, &paginas, Some(&CellValue::Number(736.0)))?;
        // A board grouped by «Estado», filtered by «Leyendo», sorted by pages.
        let mut config = ViewConfig::default();
        config.group_by = Some(estado.clone());
        config.filters.push(Filter {
            property: estado.clone(),
            op: FilterOp::Is,
            value: leyendo.clone(),
        });
        config.sorts.push(Sort {
            property: paginas.clone(),
            direction: Direction::Descending,
        });
        config.set_hidden(&paginas, true);
        db.add_view(&base, "Tablero", ViewKind::Board, &config)?;

        let empty = db.duplicate_page(&base, false)?;
        assert_eq!(
            db.get_page(&empty)?.map(|p| (p.title, p.kind)),
            Some(("Lecturas (copia)".into(), PageKind::Database))
        );
        assert!(db.database_rows(&empty)?.is_empty(), "only the structure");

        let full = db.duplicate_page(&base, true)?;
        let properties = db.database_properties(&full)?;
        let names: Vec<&str> = properties.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Estado", "Páginas"]);
        let new_estado = &properties[0];
        let new_paginas = &properties[1];
        assert_ne!(new_estado.id, estado);
        let new_leyendo = &new_estado.options[0];
        assert_eq!(
            (new_leyendo.name.as_str(), new_leyendo.color),
            ("Leyendo", OptionColor::Blue)
        );
        assert_ne!(new_leyendo.id, leyendo);

        // The views point at the copy's properties and options.
        let views = db.database_views(&full)?;
        assert_eq!(
            views.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["Tabla", "Tablero"]
        );
        let board = &views[1].config;
        assert_eq!(views[1].kind, ViewKind::Board);
        assert_eq!(board.group_by.as_deref(), Some(new_estado.id.as_str()));
        assert_eq!(board.filters[0].property, new_estado.id);
        assert_eq!(board.filters[0].value, new_leyendo.id);
        assert_eq!(board.sorts[0].property, new_paginas.id);
        assert!(board.is_hidden(&new_paginas.id));

        // Its row, with values for the copy's properties.
        let rows = db.database_rows(&full)?;
        assert_eq!(rows.len(), 1);
        let values = db.row_values(&rows[0].id)?;
        assert_eq!(
            values.get(&new_estado.id),
            Some(&CellValue::Select(new_leyendo.id.clone()))
        );
        assert_eq!(values.get(&new_paginas.id), Some(&CellValue::Number(736.0)));
        // The original is as it was.
        assert_eq!(db.database_rows(&base)?.len(), 1);
        assert_eq!(
            db.row_values(&rayuela)?.get(&estado),
            Some(&CellValue::Select(leyendo))
        );
        drop(db);
        foreign_keys_hold(&path)
    })();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    result
}

#[test]
fn templates_trashed_and_missing_pages_are_not_duplicated() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Notas", None)?;
    let template = db.save_as_template(&page)?;
    assert!(db.duplicate_page(&template, false).is_err());
    db.trash_page(&page)?;
    assert!(db.duplicate_page(&page, false).is_err());
    assert!(db.duplicate_page("no-existe", false).is_err());
    assert_eq!(db.list_pages()?.len(), 0);
    Ok(())
}

#[test]
fn several_rows_are_copied_in_one_go() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let a = db.create_row(&base, "Rayuela")?;
    db.create_row(&base, "Aleph")?;
    let c = db.create_row(&base, "Ficciones")?;
    let copies = db.duplicate_pages(&[a, c])?;
    assert_eq!(copies.len(), 2);
    let rows: Vec<String> = db
        .database_rows(&base)?
        .into_iter()
        .map(|p| p.title)
        .collect();
    assert_eq!(
        rows,
        [
            "Rayuela",
            "Rayuela (copia)",
            "Aleph",
            "Ficciones",
            "Ficciones (copia)"
        ]
    );
    // One that cannot be copied: none is.
    assert!(
        db.duplicate_pages(&[copies[0].clone(), "no-existe".into()])
            .is_err()
    );
    assert_eq!(db.database_rows(&base)?.len(), 5);
    Ok(())
}
