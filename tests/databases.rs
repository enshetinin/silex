//! Databases (H6.1, ADR 0006): a database is a page; its rows are child pages.
use std::collections::HashSet;

use silex_desktop::db::Database;
use silex_desktop::export::export_workspace;
use silex_desktop::markdown::database_to_markdown;
use silex_desktop::model::{Page, PageKind, ViewKind, build_tree, without_database_rows};
use silex_desktop::property::{CellValue, Date, OptionColor, PropertyKind, TableValues};

fn titles(pages: &[Page]) -> Vec<&str> {
    pages.iter().map(|p| p.title.as_str()).collect()
}

#[test]
fn a_new_database_is_a_page_with_a_table_view() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let id = db.create_database("Lecturas", None)?;
    let page = db.get_page(&id)?.expect("live");
    assert_eq!(page.kind, PageKind::Database);
    assert_eq!(page.title, "Lecturas");
    let views = db.database_views(&id)?;
    assert_eq!(views.len(), 1);
    assert_eq!(
        (views[0].name.as_str(), views[0].kind),
        ("Tabla", ViewKind::Table)
    );
    // Normal pages stay normal.
    let other = db.create_page("Notas", None)?;
    assert_eq!(db.get_page(&other)?.map(|p| p.kind), Some(PageKind::Page));
    assert!(db.database_views(&other)?.is_empty());
    Ok(())
}

#[test]
fn rows_are_child_pages_in_order_and_searchable() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    db.create_row(&base, "Rayuela")?;
    let second = db.create_row(&base, "Pedro Páramo")?;
    db.create_row(&base, "")?;
    assert_eq!(
        titles(&db.database_rows(&base)?),
        ["Rayuela", "Pedro Páramo", ""]
    );
    let row = db.get_page(&second)?.expect("live");
    assert_eq!(row.parent_id.as_deref(), Some(base.as_str()));
    assert_eq!(row.kind, PageKind::Page);
    // Rows are pages: they have blocks and the search finds them.
    assert!(db.list_blocks(&second)?.is_empty());
    let hits = db.search("paramo")?;
    assert_eq!(
        titles(&hits.iter().map(|h| h.page.clone()).collect::<Vec<_>>()),
        ["Pedro Páramo"]
    );
    // Trashed rows leave the table.
    db.trash_page(&second)?;
    assert_eq!(titles(&db.database_rows(&base)?), ["Rayuela", ""]);
    Ok(())
}

#[test]
fn rows_only_go_into_live_databases() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Notas", None)?;
    assert!(db.create_row(&page, "x").is_err(), "not a database");
    assert!(db.create_row("no-existe", "x").is_err());
    let base = db.create_database("Lecturas", None)?;
    db.trash_page(&base)?;
    assert!(db.create_row(&base, "x").is_err(), "in the trash");
    assert_eq!(db.list_pages()?.len(), 1, "nothing was created");
    Ok(())
}

#[test]
fn databases_are_no_destination_for_pages_or_databases() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let page = db.create_page("Notas", None)?;
    let err = db
        .move_page(&page, Some(&base))
        .expect_err("not a destination");
    assert!(format!("{err:#}").contains("base de datos"), "{err:#}");
    assert_eq!(db.get_page(&page)?.and_then(|p| p.parent_id), None);
    assert!(db.create_database("Dentro", Some(&base)).is_err());

    // A database moves like any page, and can live inside a page.
    db.move_page(&base, Some(&page))?;
    assert_eq!(
        db.get_page(&base)?.and_then(|p| p.parent_id).as_deref(),
        Some(page.as_str())
    );
    let nested = db.create_database("Proyectos", Some(&page))?;
    assert_eq!(
        db.get_page(&nested)?.map(|p| p.kind),
        Some(PageKind::Database)
    );

    // A row moved out becomes a normal page.
    let row = db.create_row(&base, "Rayuela")?;
    db.move_page(&row, None)?;
    assert!(db.database_rows(&base)?.is_empty());
    assert_eq!(db.get_page(&row)?.map(|p| p.parent_id), Some(None));
    Ok(())
}

#[test]
fn trashing_a_database_takes_its_rows_and_restoring_brings_them_back() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    db.create_page("Capítulo 1", Some(&row))?;
    assert_eq!(db.trash_page(&base)?, 3, "database, row and its subpage");
    assert!(db.get_page(&row)?.is_none());
    assert_eq!(db.list_trash()?.len(), 1, "one branch");
    db.restore_page(&base)?;
    assert_eq!(titles(&db.database_rows(&base)?), ["Rayuela"]);
    assert_eq!(db.database_views(&base)?.len(), 1);
    Ok(())
}

#[test]
fn the_sidebar_hides_rows_and_their_subpages() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let notas = db.create_page("Notas", None)?;
    let base = db.create_database("Lecturas", Some(&notas))?;
    let row = db.create_row(&base, "Rayuela")?;
    db.create_page("Capítulo 1", Some(&row))?;
    db.create_page("Ideas", Some(&notas))?;

    let pages = db.list_pages()?;
    assert_eq!(pages.len(), 5);
    let shown = without_database_rows(&pages);
    let everything: HashSet<String> = shown.iter().map(|p| p.id.clone()).collect();
    let tree: Vec<(String, usize)> = build_tree(&shown, &everything)
        .into_iter()
        .map(|r| (r.page.title, r.depth))
        .collect();
    // The row's subpage is hidden too, not shown at the root as an orphan.
    assert_eq!(
        tree,
        [
            ("Notas".to_owned(), 0),
            ("Lecturas".to_owned(), 1),
            ("Ideas".to_owned(), 1)
        ]
    );
    Ok(())
}

#[test]
fn a_database_exports_as_a_table_with_its_rows_as_subpages() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    db.create_row(&base, "Rayuela")?;
    db.create_row(&base, "Cien años | de soledad")?;
    let root = std::env::temp_dir().join(format!("silex-db-export-{}", uuid::Uuid::new_v4()));
    let export = export_workspace(&db, &root)?;
    let result = (|| -> anyhow::Result<()> {
        let table = std::fs::read_to_string(export.dir.join("Lecturas.md"))?;
        assert_eq!(
            table,
            "# Lecturas\n\n| Nombre |\n| --- |\n| Rayuela |\n| Cien años \\| de soledad |\n"
        );
        assert!(export.dir.join("Lecturas").join("Rayuela.md").exists());
        assert_eq!(export.pages, 3);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn row(id: &str, title: &str) -> Page {
    Page {
        id: id.into(),
        parent_id: Some("db".into()),
        title: title.into(),
        kind: PageKind::Page,
    }
}

#[test]
fn database_markdown_keeps_each_row_on_one_line() {
    let none = TableValues::new();
    assert_eq!(
        database_to_markdown("Vacía", &[], &[], &none),
        "# Vacía\n\n| Nombre |\n| --- |\n"
    );
    let rows = [row("1", "una\nlínea"), row("2", "*negrita*"), row("3", "")];
    assert_eq!(
        database_to_markdown("Base", &[], &rows, &none),
        "# Base\n\n| Nombre |\n| --- |\n| una línea |\n| \\*negrita\\* |\n|  |\n"
    );
}

#[test]
fn database_markdown_has_a_column_per_property() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let rayuela = db.create_row(&base, "Rayuela")?;
    db.create_row(&base, "Ficciones")?;
    let autor = db.add_property(&base, "Autor | nombre", PropertyKind::Text)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let fin = db.add_property(&base, "Fin", PropertyKind::Date)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let terminado = db.add_select_option(&estado, "Terminado", OptionColor::Green)?;
    let borrada = db.add_property(&base, "Notas", PropertyKind::Text)?;
    db.set_value(
        &rayuela,
        &autor,
        Some(&CellValue::Text("Cortázar\ny otros".into())),
    )?;
    db.set_value(&rayuela, &paginas, Some(&CellValue::Number(736.5)))?;
    db.set_value(&rayuela, &leido, Some(&CellValue::Checkbox(true)))?;
    db.set_value(
        &rayuela,
        &fin,
        Some(&CellValue::Date(Date::parse("2026-09-14").expect("date"))),
    )?;
    db.set_value(&rayuela, &estado, Some(&CellValue::Select(terminado)))?;
    db.set_value(&rayuela, &borrada, Some(&CellValue::Text("oculta".into())))?;
    db.delete_property(&borrada)?;

    let markdown = database_to_markdown(
        "Lecturas",
        &db.database_properties(&base)?,
        &db.database_rows(&base)?,
        &db.database_values(&base)?,
    );
    assert_eq!(
        markdown,
        "# Lecturas\n\n\
         | Nombre | Autor \\| nombre | Páginas | Leído | Fin | Estado |\n\
         | --- | --- | --- | --- | --- | --- |\n\
         | Rayuela | Cortázar y otros | 736,5 | [x] | 2026-09-14 | Terminado |\n\
         | Ficciones |  |  | [ ] |  |  |\n"
    );
    Ok(())
}

// ---- H11.2: acting on several rows at once ----

#[test]
fn several_rows_go_to_the_trash_together_and_come_back_together() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let rows: Vec<String> = ["Rayuela", "Pedro Páramo", "Ficciones", "Aleph"]
        .into_iter()
        .map(|t| db.create_row(&base, t))
        .collect::<anyhow::Result<_>>()?;
    // A row with a subpage takes it along.
    db.create_page("Notas", Some(&rows[0]))?;
    let chosen = [rows[0].clone(), rows[2].clone(), rows[3].clone()];
    assert_eq!(db.trash_pages(&chosen)?, 4);
    assert_eq!(titles(&db.database_rows(&base)?), ["Pedro Páramo"]);
    // Each row is its own trash entry, and can come back alone…
    assert_eq!(db.list_trash()?.len(), 3);
    db.restore_page(&rows[3])?;
    assert_eq!(titles(&db.database_rows(&base)?), ["Pedro Páramo", "Aleph"]);
    // …or with the others, in one go, in their places.
    db.restore_pages(&chosen[..2])?;
    assert_eq!(
        titles(&db.database_rows(&base)?),
        ["Rayuela", "Pedro Páramo", "Ficciones", "Aleph"]
    );
    assert_eq!(
        titles(&db.list_pages()?)
            .iter()
            .filter(|t| **t == "Notas")
            .count(),
        1
    );
    assert!(db.list_trash()?.is_empty());
    Ok(())
}

#[test]
fn restoring_several_rows_is_all_or_nothing() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let a = db.create_row(&base, "Rayuela")?;
    let b = db.create_row(&base, "Ficciones")?;
    db.trash_pages(std::slice::from_ref(&a))?;
    // `b` is not in the trash: nothing comes back.
    assert!(db.restore_pages(&[a.clone(), b.clone()]).is_err());
    assert_eq!(titles(&db.database_rows(&base)?), ["Ficciones"]);
    // Trashing nothing (or only gone rows) changes nothing.
    assert_eq!(db.trash_pages(&[])?, 0);
    assert_eq!(db.trash_pages(&[a])?, 0);
    Ok(())
}

#[test]
fn several_values_are_set_in_one_transaction_or_not_at_all() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let a = db.create_row(&base, "Rayuela")?;
    let b = db.create_row(&base, "Ficciones")?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let yes = CellValue::Checkbox(true);
    db.set_values(&[
        (a.as_str(), leido.as_str(), Some(&yes)),
        (b.as_str(), leido.as_str(), Some(&yes)),
    ])?;
    let values = db.database_values(&base)?;
    for row in [&a, &b] {
        assert_eq!(values.get(row).and_then(|c| c.get(&leido)), Some(&yes));
    }
    // A value of the wrong kind in the middle: nothing changes.
    let number = CellValue::Number(10.0);
    let wrong = CellValue::Text("diez".into());
    assert!(
        db.set_values(&[
            (a.as_str(), paginas.as_str(), Some(&number)),
            (b.as_str(), paginas.as_str(), Some(&wrong)),
        ])
        .is_err()
    );
    assert!(
        db.database_values(&base)?
            .get(&a)
            .and_then(|c| c.get(&paginas))
            .is_none()
    );
    // `None` clears.
    db.set_values(&[
        (a.as_str(), leido.as_str(), None),
        (b.as_str(), leido.as_str(), None),
    ])?;
    let values = db.database_values(&base)?;
    assert!(values.get(&a).and_then(|c| c.get(&leido)).is_none());
    assert!(values.get(&b).and_then(|c| c.get(&leido)).is_none());
    Ok(())
}

/// H12.11: what the card of a database in a page shows.
#[test]
fn the_preview_follows_the_view_last_open() -> anyhow::Result<()> {
    use silex_desktop::view::{Direction, Sort, ViewConfig};
    let mut db = Database::open_in_memory()?;
    let notas = db.create_page("Notas", None)?;
    let base = db.create_database("Lecturas", Some(&notas))?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let nota = db.add_property(&base, "Nota", PropertyKind::Number)?;
    for (title, author, mark) in [
        ("Solaris", "Lem", 8.0),
        ("Dune", "Herbert", 9.0),
        ("Neuromante", "Gibson", 7.0),
    ] {
        let row = db.create_row(&base, title)?;
        db.set_value(&row, &autor, Some(&CellValue::Text(author.into())))?;
        db.set_value(&row, &nota, Some(&CellValue::Number(mark)))?;
    }
    // The first view, as it is: manual order, every column.
    let preview = db.database_preview(&base, 2)?.expect("a database");
    assert_eq!((preview.view.as_str(), preview.total), ("Tabla", 3));
    let titles: Vec<&str> = preview.rows.iter().map(|(p, _)| p.title.as_str()).collect();
    assert_eq!(titles, ["Solaris", "Dune"]);
    assert_eq!(
        preview.rows[1].1.get(&autor),
        Some(&CellValue::Text("Herbert".into()))
    );
    assert_eq!(preview.columns.len(), 2);
    // The view last open, sorted by mark and without «Autor».
    let mut config = ViewConfig::default();
    config.sorts = vec![Sort {
        property: nota.clone(),
        direction: Direction::Descending,
    }];
    config.set_hidden(&autor, true);
    let best = db.add_view(&base, "Mejores", ViewKind::Table, &config)?;
    db.set_last_view(&base, &best)?;
    let preview = db.database_preview(&base, 2)?.expect("a database");
    assert_eq!(preview.view, "Mejores");
    let titles: Vec<&str> = preview.rows.iter().map(|(p, _)| p.title.as_str()).collect();
    assert_eq!(titles, ["Dune", "Solaris"]);
    let columns: Vec<&str> = preview.columns.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(columns, ["Nota"]);
    // A page, or a database in the trash: no preview.
    assert!(db.database_preview(&notas, 2)?.is_none());
    db.trash_page(&base)?;
    assert!(db.database_preview(&base, 2)?.is_none());
    Ok(())
}

// ---- H13.1: rows in their manual order ----

/// Temporary on-disk database; removes the file and its WAL/SHM even if the test fails.
struct TempDb(std::path::PathBuf);

impl TempDb {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("silex-rows-{}.sqlite3", uuid::Uuid::new_v4())))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

#[test]
fn rows_are_arranged_by_hand_and_the_order_is_kept() -> anyhow::Result<()> {
    let file = TempDb::new();
    let path = file.0.clone();
    let mut db = Database::open(&path)?;
    let base = db.create_database("Lecturas", None)?;
    let rows: Vec<String> = ["Rayuela", "Pedro Páramo", "Ficciones", "Aleph"]
        .into_iter()
        .map(|t| db.create_row(&base, t))
        .collect::<anyhow::Result<_>>()?;
    // Aleph between Rayuela and Pedro Páramo: only the span that changed.
    let span = [rows[3].clone(), rows[1].clone(), rows[2].clone()];
    assert!(db.arrange_rows(&base, &span)?);
    assert_eq!(
        titles(&db.database_rows(&base)?),
        ["Rayuela", "Aleph", "Pedro Páramo", "Ficciones"]
    );
    // The same order again changes nothing.
    assert!(!db.arrange_rows(&base, &span)?);
    // It is on disk.
    drop(db);
    let mut db = Database::open(&path)?;
    assert_eq!(
        titles(&db.database_rows(&base)?),
        ["Rayuela", "Aleph", "Pedro Páramo", "Ficciones"]
    );
    // Undone with what the span was.
    assert!(db.arrange_rows(&base, &rows[1..])?);
    assert_eq!(
        titles(&db.database_rows(&base)?),
        ["Rayuela", "Pedro Páramo", "Ficciones", "Aleph"]
    );
    Ok(())
}

#[test]
fn a_trashed_row_comes_back_to_its_place_after_others_moved() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let rows: Vec<String> = ["A", "B", "C", "D"]
        .into_iter()
        .map(|t| db.create_row(&base, t))
        .collect::<anyhow::Result<_>>()?;
    db.trash_page(&rows[1])?;
    // D to the top; B, in the trash, is left out of the order asked for.
    let wanted = [
        rows[3].clone(),
        rows[1].clone(),
        rows[0].clone(),
        rows[2].clone(),
    ];
    assert!(db.arrange_rows(&base, &wanted)?);
    assert_eq!(titles(&db.database_rows(&base)?), ["D", "A", "C"]);
    // B went along with A, the row before it.
    db.restore_page(&rows[1])?;
    assert_eq!(titles(&db.database_rows(&base)?), ["D", "A", "B", "C"]);
    // A trashed first row stays first.
    db.trash_page(&rows[3])?;
    assert!(db.arrange_rows(&base, &[rows[2].clone(), rows[0].clone()])?);
    db.restore_page(&rows[3])?;
    assert_eq!(titles(&db.database_rows(&base)?), ["D", "C", "B", "A"]);
    Ok(())
}

#[test]
fn rows_are_arranged_only_in_a_live_database_and_only_its_own() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let base = db.create_database("Lecturas", None)?;
    let a = db.create_row(&base, "A")?;
    let b = db.create_row(&base, "B")?;
    let other = db.create_database("Recetas", None)?;
    let x = db.create_row(&other, "X")?;
    let page = db.create_page("Notas", None)?;
    assert!(db.arrange_rows(&page, &[b.clone(), a.clone()]).is_err());
    assert!(
        db.arrange_rows("no-existe", &[b.clone(), a.clone()])
            .is_err()
    );
    // Rows of another database, or unknown ids, are left out.
    assert!(!db.arrange_rows(&base, &[x.clone(), "zz".into()])?);
    assert!(db.arrange_rows(&base, &[x.clone(), b.clone(), a.clone()])?);
    assert_eq!(titles(&db.database_rows(&base)?), ["B", "A"]);
    assert_eq!(titles(&db.database_rows(&other)?), ["X"]);
    db.trash_page(&base)?;
    assert!(db.arrange_rows(&base, &[a, b]).is_err(), "in the trash");
    Ok(())
}

#[test]
fn arranging_rows_repairs_duplicate_numbers() -> anyhow::Result<()> {
    let file = TempDb::new();
    let path = file.0.clone();
    let mut db = Database::open(&path)?;
    let base = db.create_database("Lecturas", None)?;
    let rows: Vec<String> = ["A", "B", "C"]
        .into_iter()
        .map(|t| db.create_row(&base, t))
        .collect::<anyhow::Result<_>>()?;
    drop(db);
    // Two rows with the same number (an older Silex, a manual edit).
    {
        let conn = rusqlite::Connection::open(&path)?;
        conn.execute(
            "UPDATE pages SET sort_order = 0 WHERE parent_id = ?1",
            [&base],
        )?;
    }
    let mut db = Database::open(&path)?;
    // Ties go by creation time and id: whatever the order is, reverse it.
    let mut reversed: Vec<String> = db.database_rows(&base)?.into_iter().map(|p| p.id).collect();
    assert_eq!(reversed.len(), rows.len());
    reversed.reverse();
    assert!(db.arrange_rows(&base, &reversed)?);
    let now: Vec<String> = db.database_rows(&base)?.into_iter().map(|p| p.id).collect();
    assert_eq!(now, reversed);
    let conn = rusqlite::Connection::open(&path)?;
    let numbers: Vec<i64> = conn
        .prepare("SELECT sort_order FROM pages WHERE parent_id = ?1 ORDER BY sort_order")?
        .query_map([&base], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(numbers, [0, 1, 2]);
    Ok(())
}
