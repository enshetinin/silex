use silex_desktop::db::Database;

fn favorite_titles(db: &Database) -> anyhow::Result<Vec<String>> {
    Ok(db.list_favorites()?.into_iter().map(|p| p.title).collect())
}

#[test]
fn mark_and_unmark_favorites_in_marking_order() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    let a = db.create_page("A", None)?;
    let b = db.create_page("B", None)?;
    let c = db.create_page("C", Some(&a))?;

    db.set_favorite(&c, true)?;
    std::thread::sleep(std::time::Duration::from_millis(5));
    db.set_favorite(&a, true)?;
    assert_eq!(favorite_titles(&db)?, ["C", "A"]);

    // Marking again keeps the original position.
    db.set_favorite(&c, true)?;
    assert_eq!(favorite_titles(&db)?, ["C", "A"]);

    db.set_favorite(&c, false)?;
    db.set_favorite(&b, false)?; // unmarking a non-favorite is fine
    assert_eq!(favorite_titles(&db)?, ["A"]);
    Ok(())
}

#[test]
fn trashed_favorites_are_hidden_and_return_on_restore() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let parent = db.create_page("Proyectos", None)?;
    let child = db.create_page("Silex", Some(&parent))?;
    db.set_favorite(&child, true)?;

    db.trash_page(&parent)?;
    assert!(db.list_favorites()?.is_empty());
    assert!(db.set_favorite(&child, false).is_err(), "trashed page");

    db.restore_page(&parent)?;
    assert_eq!(favorite_titles(&db)?, ["Silex"]);
    Ok(())
}

#[test]
fn missing_page_cannot_be_favorited() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    assert!(db.set_favorite("no-existe", true).is_err());
    Ok(())
}

#[test]
fn favorites_persist_after_reopen() -> anyhow::Result<()> {
    let path = std::env::temp_dir().join(format!("silex-fav-{}.sqlite3", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        {
            let db = Database::open(&path)?;
            let id = db.create_page("Diario", None)?;
            db.set_favorite(&id, true)?;
        }
        let db = Database::open(&path)?;
        assert_eq!(favorite_titles(&db)?, ["Diario"]);
        Ok(())
    })();
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    result
}
