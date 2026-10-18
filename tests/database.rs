use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::Connection;
use silex_desktop::db::Database;

/// Temporary on-disk database; removes the file and its WAL/SHM even if the test fails.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("silex-test-{}.sqlite3", uuid::Uuid::new_v4())))
    }

    fn path(&self) -> &Path {
        &self.0
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
fn pages_and_blocks_persist_and_are_searchable() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page_id = db.create_page("Proyecto Ártico", None)?;
    let block_id = db.add_block(&page_id, "paragraph")?;
    let mut blocks = db.list_blocks(&page_id)?;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].id, block_id);

    blocks[0].content = "Preparar expedición boreal".into();
    db.save_document(&page_id, "Proyecto Boreal", &blocks)?;

    assert_eq!(db.get_page(&page_id)?.unwrap().title, "Proyecto Boreal");
    assert!(db.search_pages("Boreal")?.iter().any(|p| p.id == page_id));
    assert!(
        db.search_pages("expedición")?
            .iter()
            .any(|p| p.id == page_id)
    );
    assert!(!db.search_pages("Ártico")?.iter().any(|p| p.id == page_id));
    Ok(())
}

#[test]
fn trash_hides_page_from_navigation_and_search() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    let id = db.create_page("Temporal", None)?;
    db.trash_page(&id)?;
    assert!(db.list_pages()?.iter().all(|p| p.id != id));
    assert!(db.search_pages("Temporal")?.iter().all(|p| p.id != id));
    Ok(())
}

#[test]
fn foreign_keys_reject_orphan_blocks() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    assert!(db.add_block("pagina-inexistente", "paragraph").is_err());
    Ok(())
}

#[test]
fn schema_migration_is_idempotent() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let id = {
        let db = Database::open(tmp.path())?;
        db.create_page("Conservar", None)?
    };
    let db = Database::open(tmp.path())?;
    assert!(db.get_page(&id)?.is_some());
    Ok(())
}

#[test]
fn document_roundtrip_survives_reopen() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let page_id = {
        let mut db = Database::open(tmp.path())?;
        let page_id = db.create_page("Borrador", None)?;
        for kind in ["heading", "paragraph", "todo"] {
            db.add_block(&page_id, kind)?;
        }
        let mut blocks = db.list_blocks(&page_id)?;
        blocks[0].content = "Título 🚀".into();
        blocks[1].content = "Línea uno\nLínea dos — ñandú 日本語".into();
        blocks[2].content = "Comprar café".into();
        blocks[2].checked = true;
        db.save_document(&page_id, "Notas de octubre ✨", &blocks)?;
        page_id
    };

    let db = Database::open(tmp.path())?;
    let page = db.get_page(&page_id)?.expect("page must survive reopen");
    assert_eq!(page.title, "Notas de octubre ✨");
    let blocks = db.list_blocks(&page_id)?;
    let summary: Vec<_> = blocks
        .iter()
        .map(|b| (b.kind.as_str(), b.content.as_str(), b.checked, b.position))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("heading", "Título 🚀", false, 0),
            ("paragraph", "Línea uno\nLínea dos — ñandú 日本語", false, 1),
            ("todo", "Comprar café", true, 2),
        ]
    );
    Ok(())
}

#[test]
fn failed_save_rolls_back_title() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page_id = db.create_page("Original", None)?;
    db.add_block(&page_id, "paragraph")?;
    let mut blocks = db.list_blocks(&page_id)?;
    blocks[0].content = "no debe guardarse".into();
    let mut ghost = blocks[0].clone();
    ghost.id = "bloque-inexistente".into();
    blocks.push(ghost);

    assert!(db.save_document(&page_id, "Cambiado", &blocks).is_err());
    assert_eq!(
        db.get_page(&page_id)?.expect("page exists").title,
        "Original"
    );
    assert_eq!(db.list_blocks(&page_id)?[0].content, "");
    Ok(())
}

#[test]
fn newer_schema_is_rejected() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    {
        let conn = Connection::open(tmp.path())?;
        conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 99;")?;
    }
    assert!(Database::open(tmp.path()).is_err());

    let conn = Connection::open(tmp.path())?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    assert_eq!(version, 99);
    let has_pages: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'pages')",
        [],
        |r| r.get(0),
    )?;
    assert!(!has_pages, "a rejected database must not be migrated");
    Ok(())
}

#[test]
fn search_handles_fts_syntax_safely() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    db.create_page("Plan NEAR OR mañana", None)?;
    for input in [
        "\"",
        "*",
        "NEAR(",
        "OR",
        "-x",
        "'; DROP TABLE pages; --",
        "a\"b",
        "(",
        "^",
        "\u{0}",
    ] {
        db.search_pages(input)?;
    }
    assert_eq!(db.list_pages()?.len(), 1);
    Ok(())
}

#[test]
fn search_tracks_block_updates() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page_id = db.create_page("Página", None)?;
    db.add_block(&page_id, "paragraph")?;
    let mut blocks = db.list_blocks(&page_id)?;

    blocks[0].content = "manzana".into();
    db.save_document(&page_id, "Página", &blocks)?;
    assert_eq!(db.search_pages("manzana")?.len(), 1);

    blocks[0].content = "pera".into();
    db.save_document(&page_id, "Página", &blocks)?;
    assert!(db.search_pages("manzana")?.is_empty());
    assert_eq!(db.search_pages("pera")?.len(), 1);
    Ok(())
}

#[test]
fn pragmas_are_applied() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let _db = Database::open(tmp.path())?;
    // journal_mode persists in the file; check it from an independent connection.
    let conn = Connection::open(tmp.path())?;
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    assert_eq!(mode.to_lowercase(), "wal");
    Ok(())
}

#[test]
fn foreign_keys_are_enforced_on_disk() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let db = Database::open(tmp.path())?;
    assert!(db.add_block("pagina-inexistente", "paragraph").is_err());
    Ok(())
}

#[test]
fn invalid_block_kind_is_rejected() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    let page_id = db.create_page("Tipos", None)?;
    assert!(db.add_block(&page_id, "x").is_err());
    assert!(db.list_blocks(&page_id)?.is_empty());
    Ok(())
}

/// Manual: `cargo test --test database -- --ignored --nocapture`. Prints timings, never fails on speed.
#[test]
#[ignore = "scale smoke test; run manually"]
fn scale_smoke() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let mut db = Database::open(tmp.path())?;
    let start = Instant::now();
    let mut page_ids = Vec::new();
    for p in 0..100 {
        let id = db.create_page(&format!("Página {p}"), None)?;
        for _ in 0..10 {
            db.add_block(&id, "paragraph")?;
        }
        page_ids.push(id);
    }
    println!("crear 100 páginas / 1000 bloques: {:?}", start.elapsed());

    let t = Instant::now();
    assert_eq!(db.list_pages()?.len(), 100);
    println!("list_pages: {:?}", t.elapsed());

    let t = Instant::now();
    let mut blocks = db.list_blocks(&page_ids[50])?;
    println!("list_blocks (10): {:?}", t.elapsed());

    for (i, block) in blocks.iter_mut().enumerate() {
        block.content = format!("contenido buscable número {i}");
    }
    let t = Instant::now();
    db.save_document(&page_ids[50], "Página 50", &blocks)?;
    println!(
        "save_document (10 bloques, synchronous=FULL): {:?}",
        t.elapsed()
    );

    let t = Instant::now();
    assert_eq!(db.search_pages("buscable")?.len(), 1);
    println!("search_pages: {:?}", t.elapsed());

    let backups = std::env::temp_dir().join(format!("silex-scale-{}", uuid::Uuid::new_v4()));
    let t = Instant::now();
    let copy = silex_desktop::db::backup::create_backup(tmp.path(), &backups)?;
    println!("create_backup (verificada): {:?}", t.elapsed());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let t = Instant::now();
    let safety = db.restore_from_backup(&copy)?;
    println!(
        "restore_from_backup (incluye copia previa): {:?}",
        t.elapsed()
    );
    let _ = std::fs::remove_dir_all(&backups);
    // The pre-restore copy lands in backups/ next to the temp database.
    let _ = std::fs::remove_file(&safety);
    let _ = safety.parent().map(std::fs::remove_dir); // only if empty
    Ok(())
}
