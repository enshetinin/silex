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
