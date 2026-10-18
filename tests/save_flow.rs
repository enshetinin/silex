//! Save failures: nothing is lost in memory or on disk, and a retry recovers.
use std::path::PathBuf;
use std::time::Instant;

use silex_desktop::db::Database;
use silex_desktop::save_state::{SaveState, SaveStatus};

struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("silex-save-{}.sqlite3", uuid::Uuid::new_v4())))
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
fn save_to_page_trashed_elsewhere_fails_and_keeps_disk_and_memory() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let mut db = Database::open(&tmp.0)?;
    let page_id = db.create_page("Informe", None)?;
    db.add_block(&page_id, "paragraph")?;
    let mut blocks = db.list_blocks(&page_id)?;
    blocks[0].content = "versión guardada".into();
    db.save_document(&page_id, "Informe", &blocks)?;

    // Another connection (e.g. a second window) moves the page to the trash.
    Database::open(&tmp.0)?.trash_page(&page_id)?;

    let mut state = SaveState::default();
    blocks[0].content = "edición sin guardar".into();
    state.mark_dirty(Instant::now());
    let result = db.save_document(&page_id, "Informe editado", &blocks);
    let err = result.expect_err("saving a trashed page must fail");
    state.record_failure(Instant::now(), format!("{err:#}"));

    assert!(matches!(state.status(), SaveStatus::Failed { .. }));
    assert!(
        !state.can_close(),
        "must not close silently with unsaved edits"
    );
    assert_eq!(blocks[0].content, "edición sin guardar");

    drop(db);
    let reopened = Database::open(&tmp.0)?;
    assert_eq!(
        reopened.list_blocks(&page_id)?[0].content,
        "versión guardada"
    );
    Ok(())
}

#[test]
fn retry_after_failed_save_persists_changes() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let page_id = {
        let mut db = Database::open(&tmp.0)?;
        let page_id = db.create_page("Notas", None)?;
        db.add_block(&page_id, "todo")?;
        let mut blocks = db.list_blocks(&page_id)?;
        let mut state = SaveState::default();

        blocks[0].content = "Llamar al banco".into();
        blocks[0].checked = true;
        state.mark_dirty(Instant::now());

        // First attempt fails (a stale block makes the transaction roll back).
        let mut stale = blocks.clone();
        let mut ghost = stale[0].clone();
        ghost.id = "bloque-borrado".into();
        stale.push(ghost);
        let err = db
            .save_document(&page_id, "Notas", &stale)
            .expect_err("stale block must fail");
        state.record_failure(Instant::now(), format!("{err:#}"));
        assert!(!state.can_close());

        // Retry with the current document succeeds and clears the error.
        db.save_document(&page_id, "Notas", &blocks)?;
        state.record_success();
        assert_eq!(state.status(), SaveStatus::Saved);
        assert!(state.can_close());
        page_id
    };

    let db = Database::open(&tmp.0)?;
    let blocks = db.list_blocks(&page_id)?;
    assert_eq!(blocks[0].content, "Llamar al banco");
    assert!(blocks[0].checked);
    Ok(())
}
