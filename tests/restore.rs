use std::{fs, path::PathBuf};

use rusqlite::Connection;
use silex_desktop::db::{Database, backup};
use silex_desktop::model::BlockKind;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-restore-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn db(&self) -> PathBuf {
        self.0.join("silex.sqlite3")
    }

    fn backups(&self) -> PathBuf {
        backup::backups_dir(&self.db())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Version 1 of a document, backed up; then edited into version 2.
struct Scenario {
    tmp: TempDir,
    db: Database,
    page_id: String,
    later_page: String,
    v1_backup: PathBuf,
}

fn scenario() -> anyhow::Result<Scenario> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    let page_id = db.create_page("Presupuesto", None)?;
    db.add_block(&page_id, BlockKind::Paragraph)?;
    let mut blocks = db.list_blocks(&page_id)?;
    blocks[0].content = "versión uno aprobada".into();
    db.save_document(&page_id, "Presupuesto", &blocks)?;
    let v1_backup = backup::create_backup(&tmp.db(), &tmp.backups())?;
    // Backup names have one-second resolution; avoid a name clash with the
    // pre-restore copy made later in the same second.
    std::thread::sleep(std::time::Duration::from_millis(1100));

    blocks[0].content = "versión dos borrador".into();
    db.save_document(&page_id, "Presupuesto revisado", &blocks)?;
    let later_page = db.create_page("Página posterior", None)?;
    Ok(Scenario {
        tmp,
        db,
        page_id,
        later_page,
        v1_backup,
    })
}

#[test]
fn restore_brings_back_backup_content_and_search() -> anyhow::Result<()> {
    let mut s = scenario()?;
    s.db.restore_from_backup(&s.v1_backup)?;

    let page = s.db.get_page(&s.page_id)?.expect("page exists in v1");
    assert_eq!(page.title, "Presupuesto");
    assert_eq!(
        s.db.list_blocks(&s.page_id)?[0].content,
        "versión uno aprobada"
    );
    assert!(s.db.get_page(&s.later_page)?.is_none());
    assert_eq!(s.db.search_pages("aprobada")?.len(), 1);
    assert!(s.db.search_pages("borrador")?.is_empty());
    assert!(s.db.search_pages("revisado")?.is_empty());
    Ok(())
}

#[test]
fn restore_saves_current_state_first_and_it_can_undo_the_restore() -> anyhow::Result<()> {
    let mut s = scenario()?;
    let safety = s.db.restore_from_backup(&s.v1_backup)?;
    // SQLite reports canonical paths (e.g. /private/var on macOS); compare names.
    assert_ne!(safety.file_name(), s.v1_backup.file_name());
    backup::verify_backup(&safety)?;
    let backups = backup::list_backups(&s.tmp.backups())?;
    assert_eq!(backups.len(), 2);
    assert_eq!(
        backups.first().and_then(|p| p.file_name()),
        safety.file_name()
    );

    // Restoring the pre-restore copy brings version 2 back.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    s.db.restore_from_backup(&safety)?;
    assert_eq!(
        s.db.list_blocks(&s.page_id)?[0].content,
        "versión dos borrador"
    );
    assert!(s.db.get_page(&s.later_page)?.is_some());
    Ok(())
}

#[test]
fn corrupt_backup_is_rejected_without_touching_anything() -> anyhow::Result<()> {
    let mut s = scenario()?;
    let bad = s.tmp.backups().join("silex-20000101-000000.sqlite3");
    fs::write(&bad, b"esto no es una copia de Silex")?;
    let before = backup::list_backups(&s.tmp.backups())?;

    assert!(s.db.restore_from_backup(&bad).is_err());
    assert_eq!(backup::list_backups(&s.tmp.backups())?, before);
    assert_eq!(
        s.db.list_blocks(&s.page_id)?[0].content,
        "versión dos borrador"
    );
    assert!(s.db.get_page(&s.later_page)?.is_some());
    Ok(())
}

#[test]
fn missing_backup_is_rejected() -> anyhow::Result<()> {
    let mut s = scenario()?;
    let missing = s.tmp.backups().join("silex-20000101-000000.sqlite3");
    assert!(s.db.restore_from_backup(&missing).is_err());
    assert!(!missing.exists());
    assert!(s.db.get_page(&s.later_page)?.is_some());
    Ok(())
}

#[test]
fn restored_database_keeps_pragmas_and_survives_reopen() -> anyhow::Result<()> {
    let mut s = scenario()?;
    s.db.restore_from_backup(&s.v1_backup)?;
    assert!(
        s.db.add_block("pagina-inexistente", BlockKind::Paragraph)
            .is_err()
    );

    // Writing after restore works and persists.
    let mut blocks = s.db.list_blocks(&s.page_id)?;
    blocks[0].content = "editado tras restaurar".into();
    s.db.save_document(&s.page_id, "Presupuesto", &blocks)?;
    drop(s.db);

    let conn = Connection::open(s.tmp.db())?;
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    assert_eq!(mode, "wal");
    drop(conn);

    let db = Database::open(s.tmp.db())?;
    assert_eq!(
        db.list_blocks(&s.page_id)?[0].content,
        "editado tras restaurar"
    );
    assert!(db.get_page(&s.later_page)?.is_none());
    Ok(())
}

#[test]
fn in_memory_database_cannot_be_restored() -> anyhow::Result<()> {
    let s = scenario()?;
    let mut memory = Database::open_in_memory()?;
    assert!(memory.restore_from_backup(&s.v1_backup).is_err());
    Ok(())
}
