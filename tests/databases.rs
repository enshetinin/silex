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
