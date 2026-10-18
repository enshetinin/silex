//! Schema upgrades from databases created by earlier versions of Silex.
use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use silex_desktop::db::{Database, backup};

const SCHEMA_V1: &str = include_str!("../src/db/migrations/0001_init.sql");
const LATEST: i64 = 2;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-migr-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn db(&self) -> PathBuf {
        self.0.join("silex.sqlite3")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A database exactly as Silex v1 left it: v1 schema, WAL, some content.
fn write_v1_database(path: &Path) -> anyhow::Result<()> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(SCHEMA_V1)?;
    conn.execute_batch(
        "INSERT INTO pages (id, parent_id, title, sort_order) VALUES
             ('p1', NULL, 'Recetas', 0),
             ('p2', 'p1', 'Pan de masa madre', 0),
             ('p3', NULL, 'En la papelera', 1);
         UPDATE pages SET deleted_at = '2026-01-01T00:00:00.000Z' WHERE id = 'p3';
         INSERT INTO blocks (id, page_id, kind, content, checked, position) VALUES
             ('b1', 'p2', 'paragraph', 'Harina de centeno 🌾', 0, 0),
             ('b2', 'p2', 'todo', 'Alimentar la masa', 1, 1);
         PRAGMA user_version = 1;",
    )?;
    Ok(())
}

fn has_column(path: &Path, table: &str, column: &str) -> anyhow::Result<bool> {
    let conn = Connection::open(path)?;
    let found: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        [table, column],
        |r| r.get(0),
    )?;
    Ok(found)
}

#[test]
fn new_database_starts_at_latest_schema() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let db = Database::open(tmp.db())?;
    assert_eq!(db.schema_version()?, LATEST);
    assert!(has_column(&tmp.db(), "pages", "favorite_at")?);
    // Fresh databases are not backed up: there is nothing to protect.
    assert!(backup::list_backups(&backup::backups_dir(&tmp.db()))?.is_empty());
    Ok(())
}

#[test]
fn v1_database_is_upgraded_preserving_everything() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    write_v1_database(&tmp.db())?;

    let db = Database::open(tmp.db())?;
    assert_eq!(db.schema_version()?, LATEST);
    let child = db.get_page("p2")?.expect("page survives");
    assert_eq!(child.title, "Pan de masa madre");
    assert_eq!(child.parent_id.as_deref(), Some("p1"));
    let blocks = db.list_blocks("p2")?;
    assert_eq!(blocks[0].content, "Harina de centeno 🌾");
    assert!(blocks[1].checked);
    assert_eq!(db.search_pages("centeno")?.len(), 1);
    assert_eq!(db.list_trash()?.len(), 1, "trash survives");
    assert!(db.list_favorites()?.is_empty());
    Ok(())
}

#[test]
fn upgrade_makes_a_verified_v1_backup_first() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    write_v1_database(&tmp.db())?;
    let _db = Database::open(tmp.db())?;

    let backups = backup::list_backups(&backup::backups_dir(&tmp.db()))?;
    assert_eq!(backups.len(), 1);
    backup::verify_backup(&backups[0])?;
    let version: i64 =
        Connection::open(&backups[0])?.pragma_query_value(None, "user_version", |r| r.get(0))?;
    assert_eq!(version, 1, "the backup keeps the old schema");
    assert!(!has_column(&backups[0], "pages", "favorite_at")?);
    Ok(())
}

#[test]
fn failed_migration_leaves_the_database_untouched() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    write_v1_database(&tmp.db())?;
    // 0002 adds the column (succeeds) and then creates its index (fails,
    // the name is taken): the whole upgrade must roll back, column included.
    Connection::open(tmp.db())?
        .execute_batch("CREATE INDEX pages_favorite_idx ON pages(title);")?;

    let err = Database::open(tmp.db()).err().expect("migration must fail");
    assert!(format!("{err:#}").contains("esquema 2"), "{err:#}");

    let conn = Connection::open(tmp.db())?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    assert_eq!(version, 1);
    drop(conn);
    assert!(
        !has_column(&tmp.db(), "pages", "favorite_at")?,
        "ALTER TABLE was rolled back"
    );
    let pages: i64 =
        Connection::open(tmp.db())?.query_row("SELECT COUNT(*) FROM pages", [], |r| r.get(0))?;
    assert_eq!(pages, 3);
    Ok(())
}

#[test]
fn reopening_an_upgraded_database_does_not_back_up_again() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    write_v1_database(&tmp.db())?;
    drop(Database::open(tmp.db())?);
    drop(Database::open(tmp.db())?);
    assert_eq!(
        backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
        1
    );
    Ok(())
}

#[test]
fn v1_backup_verifies_and_restores_into_current_schema() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let old_dir = tmp.0.join("old");
    fs::create_dir_all(&old_dir)?;
    let old_db = old_dir.join("silex.sqlite3");
    write_v1_database(&old_db)?;
    // A v1 backup as Silex v1 would have made it.
    let v1_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
    backup::verify_backup(&v1_backup)?;

    let mut db = Database::open(tmp.db())?;
    db.create_page("Página nueva", None)?;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    db.restore_from_backup(&v1_backup)?;

    assert_eq!(db.schema_version()?, LATEST);
    assert_eq!(db.list_blocks("p2")?[0].content, "Harina de centeno 🌾");
    assert!(db.search_pages("nueva")?.is_empty());
    db.set_favorite("p2", true)?;
    assert_eq!(db.list_favorites()?.len(), 1);
    Ok(())
}

#[test]
fn backups_with_unknown_schema_are_rejected() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let future = tmp.0.join("futura.sqlite3");
    write_v1_database(&future)?;
    Connection::open(&future)?.execute_batch("PRAGMA user_version = 99;")?;
    assert!(backup::verify_backup(&future).is_err());
    Ok(())
}
