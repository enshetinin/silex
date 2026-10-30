//! Schema upgrades from databases created by earlier versions of Silex.
use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use silex_desktop::db::{Database, backup};

const SCHEMA_V1: &str = include_str!("../src/db/migrations/0001_init.sql");
const SCHEMA_V2: &str = include_str!("../src/db/migrations/0002_favorites.sql");
const LATEST: i64 = 14;

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

/// A database exactly as Silex v2 left it (v1 content plus a favorite).
fn write_v2_database(path: &Path) -> anyhow::Result<()> {
    write_v1_database(path)?;
    let conn = Connection::open(path)?;
    conn.execute_batch(SCHEMA_V2)?;
    conn.execute_batch(
        "UPDATE pages SET favorite_at = '2026-02-01T00:00:00.000Z' WHERE id = 'p2';
         -- A block deleted in the middle leaves a rowid gap, as real use does
         -- (deleting the last row would let SQLite reuse its rowid).
         INSERT INTO blocks (id, page_id, kind, content, position) VALUES
             ('borrado', 'p1', 'paragraph', 'temporal', 9),
             ('b3', 'p1', 'heading', 'Panes del mundo', 0);
         DELETE FROM blocks WHERE id = 'borrado';
         PRAGMA user_version = 2;",
    )?;
    Ok(())
}

fn block_rowids(path: &Path) -> anyhow::Result<Vec<(String, i64)>> {
    let conn = Connection::open(path)?;
    let rows = conn
        .prepare("SELECT id, rowid FROM blocks ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
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

mod v3 {
    use super::*;
    use silex_desktop::model::BlockKind;

    #[test]
    fn v2_database_gets_block_kinds_and_indent_keeping_rowids() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v2_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;

        let db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(
            block_rowids(&tmp.db())?,
            rowids_before,
            "FTS depends on rowids"
        );
        let heading = &db.list_blocks("p1")?[0];
        assert_eq!(heading.kind, BlockKind::Heading1, "heading → heading_1");
        assert_eq!(heading.indent, 0);
        let bread = db.list_blocks("p2")?;
        assert_eq!(bread[0].kind, BlockKind::Paragraph);
        assert_eq!(bread[1].kind, BlockKind::Todo);
        assert!(bread[1].checked);
        // Search, favorites and trash survive the table rebuild.
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.search_pages("mundo")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert_eq!(db.list_trash()?.len(), 1);
        drop(db);
        // The rebuilt FTS index matches its content table.
        backup::verify_backup(&tmp.db())?;
        Ok(())
    }

    #[test]
    fn rebuilt_table_keeps_constraints_and_triggers() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v2_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        // Foreign key and cascade still work on the rebuilt table.
        assert!(db.add_block("no-existe", BlockKind::Paragraph).is_err());
        // FTS triggers were recreated: edits are searchable.
        let mut blocks = db.list_blocks("p2")?;
        blocks[0].content = "Masa de espelta".into();
        db.save_document("p2", "Pan de masa madre", &blocks)?;
        assert_eq!(db.search_pages("espelta")?.len(), 1);
        assert!(db.search_pages("centeno")?.is_empty());
        drop(db);
        let conn = Connection::open(tmp.db())?;
        assert!(
            conn.execute(
                "INSERT INTO blocks (id, page_id, kind, indent) VALUES ('x', 'p1', 'paragraph', 7)",
                []
            )
            .is_err(),
            "indent CHECK"
        );
        assert!(
            conn.execute(
                "INSERT INTO blocks (id, page_id, kind) VALUES ('y', 'p1', 'heading')",
                []
            )
            .is_err(),
            "old kind name rejected"
        );
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v3() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(has_column(&tmp.db(), "blocks", "indent")?);
        assert!(has_column(&tmp.db(), "pages", "favorite_at")?);
        assert_eq!(db.list_blocks("p2")?[0].content, "Harina de centeno 🌾");
        Ok(())
    }

    #[test]
    fn failed_rebuild_leaves_the_v2_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v2_database(&tmp.db())?;
        // A table already named `settings` makes the next migration (0004)
        // fail after `blocks` was rebuilt, in the same transaction: all must
        // roll back. (Until v14 an orphan block made the copy itself fail,
        // but foreign keys are off while migrating since then, ADR 0018.)
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE settings (x);")?;

        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 4"), "{err:#}");

        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 2);
        let leftover: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'blocks_v3')",
            [],
            |r| r.get(0),
        )?;
        assert!(!leftover, "new table rolled back");
        drop(conn);
        assert!(!has_column(&tmp.db(), "blocks", "indent")?);
        let blocks: i64 =
            Connection::open(tmp.db())?
                .query_row("SELECT COUNT(*) FROM blocks", [], |r| r.get(0))?;
        assert_eq!(blocks, 3, "nothing lost");
        Ok(())
    }

    #[test]
    fn v2_backup_restores_into_v3() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v2_database(&old_db)?;
        let v2_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;

        let mut db = Database::open(tmp.db())?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v2_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(db.list_blocks("p1")?[0].kind, BlockKind::Heading1);
        assert_eq!(db.search_pages("mundo")?.len(), 1);
        Ok(())
    }
}

mod v4 {
    use super::*;
    use silex_desktop::settings::{Settings, ThemeMode};

    const SCHEMA_V3: &str = include_str!("../src/db/migrations/0003_block_kinds.sql");

    /// A database exactly as Silex v3 left it.
    fn write_v3_database(path: &Path) -> anyhow::Result<()> {
        write_v2_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V3)?;
        conn.execute_batch("PRAGMA user_version = 3;")?;
        Ok(())
    }

    #[test]
    fn v3_database_gets_settings_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v3_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;

        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        assert_eq!(db.load_settings()?, Settings::default());
        assert_eq!(db.list_blocks("p2")?[0].content, "Harina de centeno 🌾");
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert_eq!(db.list_trash()?.len(), 1);
        // A verified v3 copy was made before upgrading.
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1
        );
        db.save_settings(&Settings {
            theme: ThemeMode::Light,
            zoom_percent: 110,
            ..Settings::default()
        })?;
        drop(db);
        backup::verify_backup(&tmp.db())?;
        Ok(())
    }

    #[test]
    fn settings_survive_reopening() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let wanted = Settings {
            theme: ThemeMode::Dark,
            zoom_percent: 125,
            outline: true,
        };
        let mut db = Database::open(tmp.db())?;
        db.save_settings(&Settings::default())?;
        db.save_settings(&wanted)?; // overwrites, no duplicate rows
        drop(db);
        let db = Database::open(tmp.db())?;
        assert_eq!(db.load_settings()?, wanted);
        drop(db);
        let rows: i64 =
            Connection::open(tmp.db())?
                .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?;
        assert_eq!(rows, 3, "theme, size and outline");
        Ok(())
    }

    #[test]
    fn damaged_settings_fall_back_to_defaults() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        drop(Database::open(tmp.db())?);
        Connection::open(tmp.db())?.execute_batch(
            "INSERT INTO settings (key, value) VALUES
                 ('theme', 'violeta'), ('zoom_percent', '-3'), ('futuro', '1');",
        )?;
        let db = Database::open(tmp.db())?;
        assert_eq!(db.load_settings()?, Settings::default());
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v4() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(db.load_settings()?, Settings::default());
        assert!(db.list_blocks("p2")?[1].checked);
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v3_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v3_database(&tmp.db())?;
        // A table already named `settings` makes the migration fail.
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE settings (x INTEGER);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 4"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 3);
        assert!(
            has_column(&tmp.db(), "settings", "x")?,
            "old table kept as is"
        );
        Ok(())
    }

    #[test]
    fn v3_backup_restores_into_v4() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v3_database(&old_db)?;
        let v3_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;

        let mut db = Database::open(tmp.db())?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v3_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(db.load_settings()?, Settings::default());
        assert_eq!(db.search_pages("mundo")?.len(), 1);
        Ok(())
    }
}

mod v5 {
    use super::*;
    use silex_desktop::model::{PageKind, ViewKind};

    const SCHEMA_V3: &str = include_str!("../src/db/migrations/0003_block_kinds.sql");
    const SCHEMA_V4: &str = include_str!("../src/db/migrations/0004_settings.sql");

    /// A database exactly as Silex v4 left it (v2 content, block kinds, settings).
    pub(super) fn write_v4_database(path: &Path) -> anyhow::Result<()> {
        write_v2_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V3)?;
        conn.execute_batch(SCHEMA_V4)?;
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('theme', 'dark');
             PRAGMA user_version = 4;",
        )?;
        Ok(())
    }

    pub(super) fn table_exists(path: &Path, table: &str) -> anyhow::Result<bool> {
        let conn = Connection::open(path)?;
        Ok(conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |r| r.get(0),
        )?)
    }

    #[test]
    fn v4_database_gets_databases_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v4_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;

        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        for table in [
            "db_properties",
            "db_select_options",
            "db_values",
            "db_views",
        ] {
            assert!(table_exists(&tmp.db(), table)?, "{table}");
        }
        // Existing pages are normal pages, with everything they had.
        assert!(db.list_pages()?.iter().all(|p| p.kind == PageKind::Page));
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        assert_eq!(db.list_blocks("p2")?[0].content, "Harina de centeno 🌾");
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert_eq!(db.list_trash()?.len(), 1);
        assert_eq!(
            db.load_settings()?.theme,
            silex_desktop::settings::ThemeMode::Dark
        );
        // A verified v4 copy was made before upgrading.
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1
        );
        // The new tables work, with their keys.
        let base = db.create_database("Lecturas", Some("p1"))?;
        db.create_row(&base, "Rayuela")?;
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v5() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(db.get_page("p2")?.map(|p| p.kind), Some(PageKind::Page));
        let base = db.create_database("Tareas", None)?;
        assert_eq!(db.database_views(&base)?[0].kind, ViewKind::Table);
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v4_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v4_database(&tmp.db())?;
        // A table already named `db_views` makes the migration fail after
        // `pages` got its new column: that change must be rolled back too.
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE db_views (x INTEGER);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 5"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 4);
        assert!(!has_column(&tmp.db(), "pages", "kind")?, "rolled back");
        assert!(!table_exists(&tmp.db(), "db_properties")?);
        assert!(
            has_column(&tmp.db(), "db_views", "x")?,
            "old table kept as is"
        );
        Ok(())
    }

    #[test]
    fn v4_backup_restores_into_v5() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v4_database(&old_db)?;
        let v4_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;

        let mut db = Database::open(tmp.db())?;
        let base = db.create_database("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v4_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&base)?.is_none(), "the copy had no databases");
        assert_eq!(db.search_pages("mundo")?.len(), 1);
        let new = db.create_database("Después", None)?;
        assert_eq!(db.database_views(&new)?.len(), 1);
        Ok(())
    }
}

mod v6 {
    use super::v5::{table_exists, write_v4_database};
    use super::*;
    use silex_desktop::attachment::NewAttachment;
    use silex_desktop::model::BlockKind;

    const SCHEMA_V5: &str = include_str!("../src/db/migrations/0005_databases.sql");

    /// A database exactly as Silex v5 left it (v4 content plus a database).
    pub(super) fn write_v5_database(path: &Path) -> anyhow::Result<()> {
        write_v4_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V5)?;
        conn.execute_batch(
            "INSERT INTO pages (id, title, kind, sort_order) VALUES ('base', 'Lecturas', 'database', 5);
             INSERT INTO db_views (id, database_id, name, kind, position) VALUES ('v', 'base', 'Tabla', 'table', 0);
             PRAGMA user_version = 5;",
        )?;
        Ok(())
    }

    fn file_block(db: &mut Database, page: &str) -> anyhow::Result<String> {
        let file = NewAttachment::from_bytes("informe.pdf", b"%PDF-1.7 hola".to_vec())?;
        let attachment = db.add_attachment(&file)?;
        let mut blocks = db.list_blocks(page)?;
        let id = uuid::Uuid::new_v4().to_string();
        blocks.push(silex_desktop::model::Block {
            id: id.clone(),
            page_id: page.into(),
            kind: BlockKind::File,
            content: attachment.name.clone(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: Some(attachment.id),
            link: None,
            marks: Vec::new(),
        });
        let title = db.get_page(page)?.map(|p| p.title).unwrap_or_default();
        db.save_document(page, &title, &blocks)?;
        Ok(id)
    }

    #[test]
    fn v5_database_gets_attachments_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v5_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;

        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(table_exists(&tmp.db(), "attachments")?);
        assert!(has_column(&tmp.db(), "blocks", "attachment_id")?);
        // Blocks keep their rowids (the search index points at them).
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        let blocks = db.list_blocks("p2")?;
        assert_eq!(blocks[0].content, "Harina de centeno 🌾");
        assert!(blocks.iter().all(|b| b.attachment.is_none()));
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert_eq!(db.list_trash()?.len(), 1);
        assert_eq!(db.database_views("base")?.len(), 1);
        // A verified v5 copy was made before upgrading.
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1
        );
        // A file block works, and is found by its name.
        file_block(&mut db, "p2")?;
        assert_eq!(db.search_pages("informe")?.len(), 1);
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v6() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        file_block(&mut db, "p2")?;
        assert_eq!(db.list_blocks("p2")?.len(), 3);
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v5_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v5_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // The index name is taken, so 0006 fails after `blocks` was rebuilt:
        // the old table must come back as it was.
        Connection::open(tmp.db())?
            .execute_batch("CREATE INDEX blocks_attachment_idx ON pages(title);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 6"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 5);
        drop(conn);
        assert!(!table_exists(&tmp.db(), "attachments")?);
        assert!(!has_column(&tmp.db(), "blocks", "attachment_id")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v5_backup_restores_into_v6() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v5_database(&old_db)?;
        let v5_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;

        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        file_block(&mut db, &page)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v5_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none(), "the copy had no attachments");
        assert_eq!(db.search_pages("mundo")?.len(), 1);
        file_block(&mut db, "p1")?;
        Ok(())
    }
}

mod v7 {
    use super::v5::table_exists;
    use super::v6::write_v5_database;
    use super::*;
    use silex_desktop::model::{Block, BlockKind};

    const SCHEMA_V6: &str = include_str!("../src/db/migrations/0006_attachments.sql");

    /// A database exactly as Silex v6 left it (v5 content plus a file block).
    pub(super) fn write_v6_database(path: &Path) -> anyhow::Result<()> {
        write_v5_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V6)?;
        conn.execute_batch(
            "INSERT INTO attachments (id, name, mime, size, data) VALUES ('a1', 'acta.pdf', 'application/pdf', 3, x'010203');
             INSERT INTO blocks (id, page_id, kind, content, position, attachment_id)
                 VALUES ('f1', 'p1', 'file', 'acta.pdf', 5, 'a1');
             PRAGMA user_version = 6;",
        )?;
        Ok(())
    }

    fn link(page: &str, to: &str) -> Block {
        Block {
            id: uuid::Uuid::new_v4().to_string(),
            page_id: page.into(),
            kind: BlockKind::PageLink,
            content: String::new(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: None,
            link: Some(to.into()),
            marks: Vec::new(),
        }
    }

    #[test]
    fn v6_database_gets_links_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v6_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(has_column(&tmp.db(), "blocks", "link_page_id")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        // The file block keeps its attachment.
        let blocks = db.list_blocks("p1")?;
        let file = blocks.iter().find(|b| b.id == "f1").expect("file block");
        assert_eq!(file.attachment.as_deref(), Some("a1"));
        assert_eq!(db.attachment_data("a1")?, [1, 2, 3]);
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v6 copy first"
        );
        // A link works.
        let mut blocks = db.list_blocks("p1")?;
        blocks.push(link("p1", "p2"));
        db.save_document("p1", "Recetas", &blocks)?;
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v7() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        let mut blocks = db.list_blocks("p2")?;
        blocks.push(link("p2", "p1"));
        db.save_document("p2", "Pan de masa madre", &blocks)?;
        assert_eq!(
            db.list_blocks("p2")?.last().map(|b| b.content.clone()),
            Some("Recetas".into())
        );
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v6_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v6_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // The index name is taken, so 0007 fails after `blocks` was rebuilt.
        Connection::open(tmp.db())?
            .execute_batch("CREATE INDEX blocks_link_idx ON pages(title);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 7"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 6);
        drop(conn);
        assert!(!has_column(&tmp.db(), "blocks", "link_page_id")?);
        assert!(table_exists(&tmp.db(), "attachments")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v6_backup_restores_into_v7() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v6_database(&old_db)?;
        let v6_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v6_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        assert_eq!(db.attachment_data("a1")?, [1, 2, 3]);
        let mut blocks = db.list_blocks("p1")?;
        blocks.push(link("p1", "p2"));
        db.save_document("p1", "Recetas", &blocks)?;
        Ok(())
    }
}

mod v8 {
    use super::v5::table_exists;
    use super::v7::write_v6_database;
    use super::*;

    const SCHEMA_V7: &str = include_str!("../src/db/migrations/0007_page_links.sql");

    /// A database exactly as Silex v7 left it (v6 content plus a link).
    pub(super) fn write_v7_database(path: &Path) -> anyhow::Result<()> {
        write_v6_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V7)?;
        conn.execute_batch(
            "INSERT INTO blocks (id, page_id, kind, content, position, link_page_id)
                 VALUES ('l1', 'p1', 'page_link', 'Pan de masa madre', 6, 'p2');
             PRAGMA user_version = 7;",
        )?;
        Ok(())
    }

    /// Renames p1 in the page itself, which saves its old state as a version.
    fn edit_p1(db: &mut Database) -> anyhow::Result<()> {
        let blocks = db.list_blocks("p1")?;
        db.save_document("p1", "Recetas de siempre", &blocks)?;
        Ok(())
    }

    #[test]
    fn v7_database_gets_versions_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v7_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(table_exists(&tmp.db(), "page_versions")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        let before = db.list_blocks("p1")?;
        let link = before.iter().find(|b| b.id == "l1").expect("link block");
        assert_eq!(link.link.as_deref(), Some("p2"));
        assert_eq!(db.attachment_data("a1")?, [1, 2, 3]);
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v7 copy first"
        );
        assert!(db.page_versions("p1")?.is_empty(), "no history yet");
        // The first edit keeps the page as it was, file and link included.
        edit_p1(&mut db)?;
        let versions = db.page_versions("p1")?;
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].title, "Recetas");
        let old = db.version_blocks(versions[0].id)?.expect("the version");
        // Positions are the list order.
        let same = |b: &silex_desktop::model::Block| {
            (
                b.id.clone(),
                b.kind,
                b.content.clone(),
                b.attachment.clone(),
                b.link.clone(),
            )
        };
        let old: Vec<_> = old.expect("readable").iter().map(same).collect();
        assert_eq!(old, before.iter().map(same).collect::<Vec<_>>());
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v8() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        let blocks = db.list_blocks("p2")?;
        db.save_document("p2", "Pan de centeno", &blocks)?;
        let versions = db.page_versions("p2")?;
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].title, "Pan de masa madre");
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v7_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v7_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // The index name is taken, so 0008 fails after creating its table.
        Connection::open(tmp.db())?
            .execute_batch("CREATE INDEX page_versions_page_idx ON pages(title);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 8"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 7);
        drop(conn);
        assert!(!table_exists(&tmp.db(), "page_versions")?);
        assert!(has_column(&tmp.db(), "blocks", "link_page_id")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v7_backup_restores_into_v8() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v7_database(&old_db)?;
        let v7_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v7_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        assert_eq!(
            db.list_blocks("p1")?
                .iter()
                .filter(|b| b.link.is_some())
                .count(),
            1
        );
        edit_p1(&mut db)?;
        assert_eq!(db.page_versions("p1")?.len(), 1);
        Ok(())
    }
}

mod v9 {
    use super::v5::table_exists;
    use super::v8::write_v7_database;
    use super::*;

    const SCHEMA_V8: &str = include_str!("../src/db/migrations/0008_page_versions.sql");

    /// A database exactly as Silex v8 left it (v7 content plus a version).
    pub(super) fn write_v8_database(path: &Path) -> anyhow::Result<()> {
        write_v7_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V8)?;
        conn.execute_batch(
            "INSERT INTO page_versions (page_id, title, blocks, size)
                 VALUES ('p1', 'Recetas', '[]', 2);
             PRAGMA user_version = 8;",
        )?;
        Ok(())
    }

    #[test]
    fn v8_database_gets_templates_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v8_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(has_column(&tmp.db(), "pages", "template_at")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        // Nothing became a template; everything is where it was.
        assert!(db.list_templates()?.is_empty());
        assert!(db.list_pages()?.iter().any(|p| p.id == "p1"));
        assert_eq!(db.page_versions("p1")?.len(), 1);
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v8 copy first"
        );
        // Templates work.
        let template = db.save_as_template("p2")?;
        assert_eq!(db.list_templates()?.len(), 1);
        db.create_from_template(&template, None, "2026-10-20")?;
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v9() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        let template = db.save_as_template("p2")?;
        assert!(db.is_template(&template)?);
        assert!(!db.is_template("p2")?);
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v8_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v8_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // The index name is taken, so 0009 fails after adding its column.
        Connection::open(tmp.db())?
            .execute_batch("CREATE INDEX pages_template_idx ON blocks(content);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 9"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 8);
        drop(conn);
        assert!(!has_column(&tmp.db(), "pages", "template_at")?);
        assert!(table_exists(&tmp.db(), "page_versions")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v8_backup_restores_into_v9() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v8_database(&old_db)?;
        let v8_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v8_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        assert_eq!(db.page_versions("p1")?.len(), 1);
        assert!(db.list_templates()?.is_empty());
        db.save_as_template("p1")?;
        Ok(())
    }
}

mod v10 {
    use super::v9::write_v8_database;
    use super::*;
    use silex_desktop::marks::{Mark, MarkKind};

    const SCHEMA_V9: &str = include_str!("../src/db/migrations/0009_templates.sql");

    /// A database exactly as Silex v9 left it (v8 content plus a template).
    pub(super) fn write_v9_database(path: &Path) -> anyhow::Result<()> {
        write_v8_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V9)?;
        conn.execute_batch(
            "UPDATE pages SET template_at = '2026-10-20T00:00:00.000Z' WHERE id = 'p3';
             PRAGMA user_version = 9;",
        )?;
        Ok(())
    }

    /// Bold over the first word of `p2`'s first block, saved and read back.
    fn format_first_block(db: &mut Database) -> anyhow::Result<()> {
        let mut blocks = db.list_blocks("p2")?;
        assert!(blocks.iter().all(|b| b.marks.is_empty()));
        blocks[0].marks = vec![Mark::new(0, 6, MarkKind::Bold)];
        let title = db.get_page("p2")?.expect("p2").title;
        db.save_document("p2", &title, &blocks)?;
        assert_eq!(
            db.list_blocks("p2")?[0].marks,
            vec![Mark::new(0, 6, MarkKind::Bold)]
        );
        Ok(())
    }

    #[test]
    fn v9_database_gets_marks_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v9_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(has_column(&tmp.db(), "blocks", "marks")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before, "no rebuild");
        // Everything is where it was, without formatting.
        let blocks = db.list_blocks("p2")?;
        assert_eq!(blocks[0].content, "Harina de centeno 🌾");
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.page_versions("p1")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert!(db.is_template("p3")?);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v9 copy first"
        );
        format_first_block(&mut db)?;
        // Formatting does not touch the search index.
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let nulls: i64 =
            conn.query_row("SELECT COUNT(*) FROM blocks WHERE marks IS NULL", [], |r| {
                r.get(0)
            })?;
        assert_eq!(nulls, rowids_before.len() as i64 - 1);
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v10() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        format_first_block(&mut db)?;
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v9_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v9_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // 0010 is a single ALTER: it fails if another tool already added
        // a `marks` column. The error says which upgrade failed, and the
        // v9 database is left as it was, its own column included.
        Connection::open(tmp.db())?
            .execute_batch("ALTER TABLE blocks ADD COLUMN marks INTEGER DEFAULT 7;")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 10"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 9);
        let foreign: i64 =
            conn.query_row("SELECT COUNT(*) FROM blocks WHERE marks = 7", [], |r| {
                r.get(0)
            })?;
        assert_eq!(foreign, rowids_before.len() as i64);
        drop(conn);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v9_backup_restores_into_v10() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v9_database(&old_db)?;
        let v9_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v9_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        assert!(db.is_template("p3")?);
        format_first_block(&mut db)?;
        Ok(())
    }
}

mod v11 {
    use super::v10::write_v9_database;
    use super::*;
    use silex_desktop::marks::{Mark, MarkKind};
    use silex_desktop::model::BlockKind;

    const SCHEMA_V10: &str = include_str!("../src/db/migrations/0010_inline_marks.sql");

    /// A database exactly as Silex v10 left it (v9 content plus bold on
    /// the first word of `p2`).
    pub(super) fn write_v10_database(path: &Path) -> anyhow::Result<()> {
        write_v9_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V10)?;
        conn.execute_batch(
            r#"UPDATE blocks SET marks = '[{"start":0,"end":6,"type":"bold"}]'
                 WHERE page_id = 'p2' AND position = 0;
               PRAGMA user_version = 10;"#,
        )?;
        Ok(())
    }

    /// A divider and a callout after `p2`'s blocks, saved and read back.
    fn add_divider_and_callout(db: &mut Database) -> anyhow::Result<()> {
        let mut blocks = db.list_blocks("p2")?;
        let template = blocks[0].clone();
        for (kind, content) in [(BlockKind::Divider, ""), (BlockKind::Callout, "Ojo 🌾")] {
            blocks.push(silex_desktop::model::Block {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                content: content.into(),
                marks: if kind == BlockKind::Callout {
                    vec![Mark::new(0, 3, MarkKind::Italic)]
                } else {
                    Vec::new()
                },
                ..template.clone()
            });
        }
        let title = db.get_page("p2")?.expect("p2").title;
        db.save_document("p2", &title, &blocks)?;
        let saved = db.list_blocks("p2")?;
        let n = saved.len();
        assert_eq!(saved[n - 2].kind, BlockKind::Divider);
        assert_eq!(saved[n - 1].kind, BlockKind::Callout);
        assert_eq!(saved[n - 1].marks, vec![Mark::new(0, 3, MarkKind::Italic)]);
        Ok(())
    }

    #[test]
    fn v10_database_gets_dividers_and_callouts_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v10_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before, "rowids kept");
        // Everything is where it was, formatting included.
        let blocks = db.list_blocks("p2")?;
        assert_eq!(blocks[0].content, "Harina de centeno 🌾");
        assert_eq!(blocks[0].marks, vec![Mark::new(0, 6, MarkKind::Bold)]);
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.page_versions("p1")?.len(), 1);
        assert_eq!(db.list_favorites()?.len(), 1);
        assert!(db.is_template("p3")?);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v10 copy first"
        );
        add_divider_and_callout(&mut db)?;
        // The callout's text is searchable; the indexes and triggers are back.
        assert_eq!(db.search_pages("ojo")?.len(), 1);
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        let indexes: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE tbl_name = 'blocks'
                   AND type IN ('index', 'trigger') AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(
            indexes,
            [
                "blocks_attachment_idx",
                "blocks_link_idx",
                "blocks_page_position_idx",
                "blocks_search_ad",
                "blocks_search_ai",
                "blocks_search_au",
            ]
        );
        // A divider never has text or formatting.
        let with_text = conn.execute(
            "INSERT INTO blocks (id, page_id, kind, content) VALUES ('d', 'p2', 'divider', 'x')",
            [],
        );
        assert!(with_text.is_err());
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v11() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        add_divider_and_callout(&mut db)?;
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v10_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v10_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        // The new table's name is taken, so 0011 fails at its first step.
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE blocks_v11 (x);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 11"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 10);
        let marks: String = conn.query_row(
            "SELECT marks FROM blocks WHERE page_id = 'p2' AND position = 0",
            [],
            |r| r.get(0),
        )?;
        assert!(marks.contains("bold"));
        drop(conn);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v10_backup_restores_into_v11() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v10_database(&old_db)?;
        let v10_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v10_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        assert_eq!(
            db.list_blocks("p2")?[0].marks,
            vec![Mark::new(0, 6, MarkKind::Bold)]
        );
        add_divider_and_callout(&mut db)?;
        Ok(())
    }
}

mod v12 {
    use super::v11::write_v10_database;
    use super::*;
    use silex_desktop::marks::{Mark, MarkKind};
    use silex_desktop::model::{Block, BlockKind};

    const SCHEMA_V11: &str = include_str!("../src/db/migrations/0011_divider_callout.sql");

    /// A database exactly as Silex v11 left it (v10 content plus a
    /// divider at the end of `p2`).
    pub(super) fn write_v11_database(path: &Path) -> anyhow::Result<()> {
        write_v10_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V11)?;
        conn.execute_batch(
            "INSERT INTO blocks (id, page_id, kind, position) VALUES ('div', 'p2', 'divider', 99);
             PRAGMA user_version = 11;",
        )?;
        Ok(())
    }

    /// A toggle with a block inside it at the end of `p2`, saved and read
    /// back.
    fn add_toggle(db: &mut Database) -> anyhow::Result<String> {
        let mut blocks = db.list_blocks("p2")?;
        let template = blocks[0].clone();
        let toggle = uuid::Uuid::new_v4().to_string();
        blocks.push(Block {
            id: toggle.clone(),
            kind: BlockKind::Toggle,
            content: "Más detalles".into(),
            marks: vec![Mark::new(0, 3, MarkKind::Bold)],
            indent: 0,
            ..template.clone()
        });
        blocks.push(Block {
            id: uuid::Uuid::new_v4().to_string(),
            kind: BlockKind::Paragraph,
            content: "dentro".into(),
            marks: Vec::new(),
            indent: 1,
            ..template
        });
        let title = db.get_page("p2")?.expect("p2").title;
        db.save_document("p2", &title, &blocks)?;
        let saved = db.list_blocks("p2")?;
        let n = saved.len();
        assert_eq!(saved[n - 2].kind, BlockKind::Toggle);
        assert_eq!(saved[n - 2].marks, vec![Mark::new(0, 3, MarkKind::Bold)]);
        assert_eq!(saved[n - 1].indent, 1);
        Ok(toggle)
    }

    #[test]
    fn v11_database_gets_toggles_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v11_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before, "rowids kept");
        let blocks = db.list_blocks("p2")?;
        assert_eq!(blocks[0].marks, vec![Mark::new(0, 6, MarkKind::Bold)]);
        assert!(blocks.iter().any(|b| b.kind == BlockKind::Divider));
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert_eq!(db.page_versions("p1")?.len(), 1);
        assert!(db.is_template("p3")?);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v11 copy first"
        );
        add_toggle(&mut db)?;
        assert_eq!(db.search_pages("detalles")?.len(), 1);
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        // The divider rule is still there.
        assert!(
            conn.execute("UPDATE blocks SET content = 'x' WHERE id = 'div'", [])
                .is_err()
        );
        let broken: Vec<String> = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert!(broken.is_empty(), "{broken:?}");
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v12() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        add_toggle(&mut db)?;
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v11_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v11_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE blocks_v12 (x);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 12"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 11);
        drop(conn);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before);
        Ok(())
    }

    #[test]
    fn v11_backup_restores_into_v12() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v11_database(&old_db)?;
        let v11_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v11_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        add_toggle(&mut db)?;
        Ok(())
    }

    /// Folded toggles are view state in `settings` (ADR 0016): they survive
    /// reopening, and only toggles count.
    #[test]
    fn folded_toggles_are_remembered() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v11_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.folded_blocks()?, None);
        let toggle = add_toggle(&mut db)?;
        db.save_folded_blocks(&format!(r#"["{toggle}"]"#))?;
        let ids = db.toggle_block_ids()?;
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&toggle));
        drop(db);
        let db = Database::open(tmp.db())?;
        assert_eq!(db.folded_blocks()?, Some(format!(r#"["{toggle}"]"#)));
        Ok(())
    }
}

mod v13 {
    use super::v12::write_v11_database;
    use super::*;

    const SCHEMA_V12: &str = include_str!("../src/db/migrations/0012_toggle.sql");

    /// A database exactly as Silex v12 left it (v11 content plus a toggle).
    pub(super) fn write_v12_database(path: &Path) -> anyhow::Result<()> {
        write_v11_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V12)?;
        conn.execute_batch(
            "INSERT INTO blocks (id, page_id, kind, content, position)
                 VALUES ('tog', 'p2', 'toggle', 'Más', 100);
             PRAGMA user_version = 12;",
        )?;
        Ok(())
    }

    #[test]
    fn v12_database_gets_page_icons_keeping_everything() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v12_database(&tmp.db())?;
        let rowids_before = block_rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(has_column(&tmp.db(), "pages", "icon")?);
        assert_eq!(block_rowids(&tmp.db())?, rowids_before, "no rebuild");
        assert!(db.page_icons()?.is_empty(), "no page has one yet");
        assert_eq!(db.search_pages("centeno")?.len(), 1);
        assert!(db.is_template("p3")?);
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v12 copy first"
        );
        db.set_page_icon("p2", Some("🌾"))?;
        assert_eq!(db.page_icons()?.get("p2").map(String::as_str), Some("🌾"));
        drop(db);
        backup::verify_backup(&tmp.db())?;
        // The CHECK keeps any value short.
        let conn = Connection::open(tmp.db())?;
        assert!(
            conn.execute(
                "UPDATE pages SET icon = ?1 WHERE id = 'p1'",
                ["muy largo para un icono"]
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v13() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        db.set_page_icon("p2", Some("📒"))?;
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v12_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v12_database(&tmp.db())?;
        // Another tool already added an `icon` column: 0013 fails.
        Connection::open(tmp.db())?.execute_batch("ALTER TABLE pages ADD COLUMN icon INTEGER;")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 13"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 12);
        Ok(())
    }

    #[test]
    fn v12_backup_restores_into_v13() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v12_database(&old_db)?;
        let v12_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        let page = db.create_page("Se pierde al restaurar", None)?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v12_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert!(db.get_page(&page)?.is_none());
        db.set_page_icon("p2", Some("🌾"))?;
        Ok(())
    }
}

mod v14 {
    use super::v13::write_v12_database;
    use super::*;
    use silex_desktop::property::{CellValue, PropertyKind};

    const SCHEMA_V13: &str = include_str!("../src/db/migrations/0013_page_icon.sql");

    /// A database exactly as Silex v13 left it (v12 content plus an icon,
    /// and properties, options and values in the database «Lecturas»: what
    /// a rebuild of `db_properties` with foreign keys on would cascade away).
    fn write_v13_database(path: &Path) -> anyhow::Result<()> {
        write_v12_database(path)?;
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V13)?;
        conn.execute_batch(
            "UPDATE pages SET icon = '🌾' WHERE id = 'p2';
             INSERT INTO pages (id, title, parent_id, sort_order) VALUES ('fila', 'Rayuela', 'base', 0);
             INSERT INTO db_properties (id, database_id, name, kind, position) VALUES
                 ('autor', 'base', 'Autor', 'text', 0),
                 ('estado', 'base', 'Estado', 'select', 1);
             INSERT INTO db_properties (id, database_id, name, kind, position, deleted_at) VALUES
                 ('vieja', 'base', 'Vieja', 'number', 2, '2026-01-01T00:00:00.000Z');
             INSERT INTO db_select_options (id, property_id, name, color, position) VALUES
                 ('leyendo', 'estado', 'Leyendo', 'blue', 0),
                 ('hecho', 'estado', 'Hecho', 'green', 1);
             INSERT INTO db_values (page_id, property_id, text_value) VALUES ('fila', 'autor', 'Cortázar');
             INSERT INTO db_values (page_id, property_id, option_id) VALUES ('fila', 'estado', 'hecho');
             INSERT INTO db_values (page_id, property_id, number_value) VALUES ('fila', 'vieja', 7);
             PRAGMA user_version = 13;",
        )?;
        Ok(())
    }

    fn count(path: &Path, table: &str) -> anyhow::Result<i64> {
        Ok(Connection::open(path)?.query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |r| r.get(0),
        )?)
    }

    #[test]
    fn v13_database_gets_multi_selects_keeping_properties_options_and_values() -> anyhow::Result<()>
    {
        let tmp = TempDir::new()?;
        write_v13_database(&tmp.db())?;
        let rowids = |path: &Path| -> anyhow::Result<Vec<(String, i64)>> {
            Ok(Connection::open(path)?
                .prepare("SELECT id, rowid FROM db_properties ORDER BY id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?)
        };
        let before = rowids(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(rowids(&tmp.db())?, before, "rowids kept");
        assert_eq!(count(&tmp.db(), "db_select_options")?, 2, "options kept");
        assert_eq!(count(&tmp.db(), "db_values")?, 3, "values kept");
        // As before: live properties, the deleted one restorable.
        let names: Vec<String> = db
            .database_properties("base")?
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Autor", "Estado"]);
        let values = db.database_values("base")?;
        assert_eq!(values["fila"]["estado"], CellValue::Select("hecho".into()));
        assert_eq!(db.page_icons()?.get("p2").map(String::as_str), Some("🌾"));
        assert_eq!(
            backup::list_backups(&backup::backups_dir(&tmp.db()))?.len(),
            1,
            "a verified v13 copy first"
        );
        // A multi-select works.
        let generos = db.add_property("base", "Géneros", PropertyKind::MultiSelect)?;
        let novela = db.add_select_option(
            &generos,
            "Novela",
            silex_desktop::property::OptionColor::Blue,
        )?;
        db.set_value(
            "fila",
            &generos,
            Some(&CellValue::MultiSelect(vec![novela.clone()])),
        )?;
        drop(db);
        backup::verify_backup(&tmp.db())?;
        let conn = Connection::open(tmp.db())?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let broken: i64 =
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        assert_eq!(broken, 0);
        // Foreign keys still point at the rebuilt table: deleting a
        // property takes its options and values.
        conn.execute("DELETE FROM db_properties WHERE id = 'estado'", [])?;
        assert_eq!(count(&tmp.db(), "db_select_options")?, 1);
        // The CHECK takes the new kinds and nothing else.
        for kind in ["url", "created", "edited"] {
            conn.execute(
                "INSERT INTO db_properties (id, database_id, name, kind, position)
                 VALUES (?1, 'base', ?1, ?1, 9)",
                [kind],
            )?;
        }
        assert!(
            conn.execute(
                "INSERT INTO db_properties (id, database_id, name, kind, position)
                 VALUES ('f', 'base', 'f', 'formula', 9)",
                [],
            )
            .is_err()
        );
        // A cell cannot hold an option of another property.
        assert!(
            conn.execute(
                "INSERT INTO db_multi_values (page_id, property_id, option_id)
                 VALUES ('fila', ?1, 'leyendo')",
                [&generos],
            )
            .is_err()
        );
        // Deleting an option takes it out of every cell.
        conn.execute("DELETE FROM db_select_options WHERE id = ?1", [&novela])?;
        assert_eq!(count(&tmp.db(), "db_multi_values")?, 0);
        Ok(())
    }

    #[test]
    fn v1_database_upgrades_straight_to_v14() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v1_database(&tmp.db())?;
        let mut db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        let base = db.create_database("Lecturas", None)?;
        let row = db.create_row(&base, "Rayuela")?;
        let generos = db.add_property(&base, "Géneros", PropertyKind::MultiSelect)?;
        let novela = db.add_select_option(
            &generos,
            "Novela",
            silex_desktop::property::OptionColor::Blue,
        )?;
        db.set_value(
            &row,
            &generos,
            Some(&CellValue::MultiSelect(vec![novela.clone()])),
        )?;
        assert_eq!(
            db.database_values(&base)?[&row][&generos],
            CellValue::MultiSelect(vec![novela])
        );
        Ok(())
    }

    #[test]
    fn failed_upgrade_leaves_the_v13_database_untouched() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v13_database(&tmp.db())?;
        // Another tool already made a `db_multi_values` table: 0014 fails
        // after `db_properties` was rebuilt, so all must roll back.
        Connection::open(tmp.db())?.execute_batch("CREATE TABLE db_multi_values (x);")?;
        let err = Database::open(tmp.db()).err().expect("migration must fail");
        assert!(format!("{err:#}").contains("esquema 14"), "{err:#}");
        let conn = Connection::open(tmp.db())?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert_eq!(version, 13);
        let leftover: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'db_properties_v14')",
            [],
            |r| r.get(0),
        )?;
        assert!(!leftover, "new table rolled back");
        assert_eq!(count(&tmp.db(), "db_values")?, 3, "nothing lost");
        assert_eq!(count(&tmp.db(), "db_select_options")?, 2);
        // The old CHECK is still there.
        assert!(
            conn.execute(
                "INSERT INTO db_properties (id, database_id, name, kind, position)
                 VALUES ('u', 'base', 'u', 'url', 9)",
                [],
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn an_orphan_from_an_older_version_does_not_stop_the_upgrade() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        write_v13_database(&tmp.db())?;
        Connection::open(tmp.db())?.execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO blocks (id, page_id, kind) VALUES ('huerfano', 'no-existe', 'paragraph');",
        )?;
        let db = Database::open(tmp.db())?;
        assert_eq!(db.schema_version()?, LATEST);
        // Foreign keys are on again: a new orphan is refused.
        assert!(db.create_page("Hija", Some("no-existe")).is_err());
        Ok(())
    }

    #[test]
    fn v13_backup_restores_into_v14() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let old_dir = tmp.0.join("old");
        fs::create_dir_all(&old_dir)?;
        let old_db = old_dir.join("silex.sqlite3");
        write_v13_database(&old_db)?;
        let v13_backup = backup::create_backup(&old_db, &old_dir.join("backups"))?;
        let mut db = Database::open(tmp.db())?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
        db.restore_from_backup(&v13_backup)?;
        assert_eq!(db.schema_version()?, LATEST);
        assert_eq!(
            db.database_values("base")?["fila"]["estado"],
            CellValue::Select("hecho".into())
        );
        db.add_property("base", "Géneros", PropertyKind::MultiSelect)?;
        Ok(())
    }
}
