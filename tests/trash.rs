use rusqlite::Connection;
use silex_desktop::db::Database;
use silex_desktop::model::BlockKind;

/// Notas ─ Viaje ─ Islandia ; Trabajo
fn tree(db: &Database) -> anyhow::Result<[String; 4]> {
    let notas = db.create_page("Notas", None)?;
    let viaje = db.create_page("Viaje", Some(&notas))?;
    let islandia = db.create_page("Islandia", Some(&viaje))?;
    let trabajo = db.create_page("Trabajo", None)?;
    Ok([notas, viaje, islandia, trabajo])
}

fn live_titles(db: &Database) -> anyhow::Result<Vec<String>> {
    Ok(db.list_pages()?.into_iter().map(|p| p.title).collect())
}

fn trash_summary(db: &Database) -> anyhow::Result<Vec<(String, usize)>> {
    Ok(db
        .list_trash()?
        .into_iter()
        .map(|t| (t.page.title, t.subpages))
        .collect())
}

#[test]
fn trashing_a_page_takes_its_whole_branch() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    let [notas, viaje, _, _] = tree(&db)?;
    db.add_block(&viaje, BlockKind::Paragraph)?;

    assert_eq!(db.trash_page(&notas)?, 3);
    assert_eq!(live_titles(&db)?, ["Trabajo"]);
    assert!(db.search_pages("Islandia")?.is_empty());
    assert_eq!(trash_summary(&db)?, [("Notas".to_owned(), 2)]);
    assert_eq!(db.trash_count()?, 1);
    Ok(())
}

#[test]
fn restoring_brings_back_the_branch_with_its_structure() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let [notas, viaje, islandia, _] = tree(&db)?;
    db.trash_page(&notas)?;
    db.restore_page(&notas)?;

    assert!(db.list_trash()?.is_empty());
    assert_eq!(db.get_page(&viaje)?.and_then(|p| p.parent_id), Some(notas));
    assert_eq!(
        db.get_page(&islandia)?.and_then(|p| p.parent_id),
        Some(viaje)
    );
    assert_eq!(db.search_pages("Islandia")?.len(), 1);
    Ok(())
}

#[test]
fn earlier_trashed_subpage_stays_in_trash_when_parent_is_restored() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let [notas, viaje, islandia, _] = tree(&db)?;
    // Back-to-back operations: they must not merge into one branch.
    db.trash_page(&islandia)?;
    db.trash_page(&notas)?;
    assert_eq!(
        trash_summary(&db)?,
        [("Notas".to_owned(), 1), ("Islandia".to_owned(), 0)]
    );

    db.restore_page(&notas)?;
    assert!(db.get_page(&viaje)?.is_some());
    assert!(db.get_page(&islandia)?.is_none(), "trashed separately");

    // Its parent is live again, so it returns to its place.
    db.restore_page(&islandia)?;
    assert_eq!(
        db.get_page(&islandia)?.and_then(|p| p.parent_id),
        Some(viaje)
    );
    Ok(())
}

#[test]
fn page_whose_parent_is_still_trashed_returns_to_root() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let [notas, viaje, islandia, _] = tree(&db)?;
    db.trash_page(&viaje)?;
    db.trash_page(&notas)?;
    // Restore Viaje (and Islandia with it) while Notas stays in the trash.
    db.restore_page(&viaje)?;

    let restored = db.get_page(&viaje)?.expect("restored");
    assert_eq!(restored.parent_id, None);
    assert_eq!(
        db.get_page(&islandia)?.and_then(|p| p.parent_id),
        Some(viaje)
    );
    // list_pages is ordered by sort_order globally, not in tree order.
    let mut live = live_titles(&db)?;
    live.sort();
    assert_eq!(live, ["Islandia", "Trabajo", "Viaje"]);
    assert_eq!(trash_summary(&db)?, [("Notas".to_owned(), 0)]);
    Ok(())
}

#[test]
fn restoring_a_live_or_missing_page_is_an_error() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let [notas, ..] = tree(&db)?;
    assert!(db.restore_page(&notas).is_err());
    assert!(db.restore_page("no-existe").is_err());
    assert_eq!(db.trash_page("no-existe")?, 0);
    Ok(())
}

#[test]
fn each_trash_operation_gets_a_distinct_timestamp() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("silex-trash-{}.sqlite3", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let db = Database::open(&path)?;
        let ids: Vec<String> = (0..20)
            .map(|i| db.create_page(&format!("P{i}"), None))
            .collect::<anyhow::Result<_>>()?;
        for id in &ids {
            db.trash_page(id)?;
        }
        let distinct: i64 = Connection::open(&path)?.query_row(
            "SELECT COUNT(DISTINCT deleted_at) FROM pages",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(distinct, 20);
        assert_eq!(db.list_trash()?.len(), 20);
        Ok(())
    })();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    result
}

#[test]
fn trash_and_restore_persist_after_reopen() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("silex-trash-{}.sqlite3", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let (notas, islandia) = {
            let db = Database::open(&path)?;
            let [notas, _, islandia, _] = tree(&db)?;
            db.trash_page(&notas)?;
            (notas, islandia)
        };
        {
            let mut db = Database::open(&path)?;
            assert_eq!(trash_summary(&db)?, [("Notas".to_owned(), 2)]);
            db.restore_page(&notas)?;
        }
        let db = Database::open(&path)?;
        assert!(db.get_page(&islandia)?.is_some());
        assert!(db.list_trash()?.is_empty());
        Ok(())
    })();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    result
}

// ---- H11.3, ADR 0013: deleting for good ----

/// Raw access to the same file, for integrity checks.
fn check_integrity(path: &std::path::Path) -> anyhow::Result<()> {
    let conn = Connection::open(path)?;
    let broken: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert!(broken.is_empty(), "foreign keys: {broken:?}");
    silex_desktop::db::backup::verify_backup(path)?;
    Ok(())
}

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("silex-purge-{}.sqlite3", uuid::Uuid::new_v4()))
}

fn remove_db(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn deleting_a_trash_item_removes_its_branch_and_everything_on_it() -> anyhow::Result<()> {
    use silex_desktop::model::Block;
    use silex_desktop::property::{CellValue, PropertyKind};
    let path = temp_db();
    let result = (|| -> anyhow::Result<()> {
        let mut db = Database::open(&path)?;
        let [notas, viaje, islandia, trabajo] = tree(&db)?;
        let base = db.create_database("Lecturas", Some(&viaje))?;
        let row = db.create_row(&base, "Rayuela")?;
        let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
        db.set_value(&row, &autor, Some(&CellValue::Text("Cortázar".into())))?;
        let link = Block {
            id: uuid::Uuid::new_v4().to_string(),
            page_id: trabajo.clone(),
            kind: BlockKind::PageLink,
            content: String::new(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: None,
            link: Some(islandia.clone()),
            marks: Vec::new(),
        };
        let text = Block {
            id: uuid::Uuid::new_v4().to_string(),
            page_id: islandia.clone(),
            kind: BlockKind::Paragraph,
            content: "géiseres".into(),
            link: None,
            ..link.clone()
        };
        db.save_document(&trabajo, "Trabajo", std::slice::from_ref(&link))?;
        db.save_document(&islandia, "Islandia", std::slice::from_ref(&text))?;
        db.save_version(&islandia)?;

        // A live page cannot be deleted for good.
        assert!(db.purge_trash_item(&notas).is_err());
        db.trash_page(&notas)?;
        // Nor a page inside a trashed branch: only the branch, from its head.
        assert!(db.purge_trash_item(&viaje).is_err());
        assert_eq!(db.purge_trash_item(&notas)?, 5);

        assert!(db.list_trash()?.is_empty());
        for id in [&notas, &viaje, &islandia, &base, &row] {
            assert!(db.get_page(id)?.is_none() && db.restore_page(id).is_err());
        }
        assert!(db.search("géiseres")?.is_empty());
        assert!(db.search("Rayuela")?.is_empty());
        assert!(db.page_versions(&islandia)?.is_empty());
        assert!(db.database_properties(&base)?.is_empty());
        // The link in «Trabajo» stays, pointing nowhere…
        let blocks = db.list_blocks(&trabajo)?;
        assert_eq!(
            (blocks[0].kind, blocks[0].link.clone()),
            (BlockKind::PageLink, None)
        );
        // …and a document still holding the old id saves anyway.
        db.save_document(&trabajo, "Trabajo", std::slice::from_ref(&link))?;
        assert_eq!(db.list_blocks(&trabajo)?[0].link, None);
        assert_eq!(live_titles(&db)?, ["Trabajo"]);
        drop(db);
        check_integrity(&path)
    })();
    remove_db(&path);
    result
}

#[test]
fn emptying_the_trash_deletes_every_item_and_keeps_live_pages() -> anyhow::Result<()> {
    let path = temp_db();
    let result = (|| -> anyhow::Result<()> {
        let mut db = Database::open(&path)?;
        let [notas, viaje, _, trabajo] = tree(&db)?;
        db.add_block(&trabajo, BlockKind::Paragraph)?;
        // Two items: «Viaje» (with Islandia) first, then «Notas» alone.
        db.trash_page(&viaje)?;
        db.trash_page(&notas)?;
        assert_eq!(trash_summary(&db)?.len(), 2);
        assert_eq!(db.empty_trash()?, 3);
        assert!(db.list_trash()?.is_empty());
        assert_eq!(live_titles(&db)?, ["Trabajo"]);
        assert_eq!(db.list_blocks(&trabajo)?.len(), 1);
        // Nothing to empty: nothing happens.
        assert_eq!(db.empty_trash()?, 0);
        drop(db);
        check_integrity(&path)
    })();
    remove_db(&path);
    result
}

#[test]
fn a_loose_row_of_a_deleted_database_comes_back_as_a_page_at_the_root() -> anyhow::Result<()> {
    use silex_desktop::model::PageKind;
    let path = temp_db();
    let result = (|| -> anyhow::Result<()> {
        let mut db = Database::open(&path)?;
        let base = db.create_database("Lecturas", None)?;
        let loose = db.create_row(&base, "Rayuela")?;
        db.create_row(&base, "Ficciones")?;
        // The row goes to the trash first, on its own; then the database.
        db.trash_page(&loose)?;
        db.trash_page(&base)?;
        let lecturas = db
            .list_trash()?
            .into_iter()
            .find(|t| t.page.id == base)
            .expect("in the trash");
        assert_eq!(lecturas.subpages, 1, "only Ficciones went with it");
        assert_eq!(db.purge_trash_item(&base)?, 2);
        // The loose row is still in the trash, and comes back at the root.
        assert_eq!(trash_summary(&db)?, [("Rayuela".to_owned(), 0)]);
        db.restore_page(&loose)?;
        let page = db.get_page(&loose)?.expect("live");
        assert_eq!((page.parent_id, page.kind), (None, PageKind::Page));
        drop(db);
        check_integrity(&path)
    })();
    remove_db(&path);
    result
}
