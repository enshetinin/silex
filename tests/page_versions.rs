//! Page history (ADR 0009): a page saved after 10 minutes keeps how it was
//! as a version, old versions are thinned out, and their files are kept.
use std::collections::HashSet;
use std::path::PathBuf;

use silex_desktop::attachment::NewAttachment;
use silex_desktop::db::Database;
use silex_desktop::model::{Block, BlockKind};

fn block(page: &str, content: &str) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind: BlockKind::Paragraph,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
    }
}

/// `2026-10-20T10:MM:00.000Z`, `minutes` after 10:00 (may pass the hour).
fn at(minutes: u32) -> String {
    format!(
        "2026-10-20T{:02}:{:02}:00.000Z",
        10 + minutes / 60,
        minutes % 60
    )
}

/// The texts of a version.
fn texts(db: &Database, id: i64) -> Vec<String> {
    db.version_blocks(id)
        .expect("read")
        .expect("exists")
        .expect("readable")
        .into_iter()
        .map(|b| b.content)
        .collect()
}

/// «Diario» with «uno»; returns (db, page, its block).
fn diario() -> anyhow::Result<(Database, String, Block)> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Diario", None)?;
    let uno = block(&page, "uno");
    db.set_clock(Some(&at(0)));
    db.save_document(&page, "Diario", std::slice::from_ref(&uno))?;
    Ok((db, page, uno))
}

#[test]
fn a_save_keeps_how_the_page_was_at_most_every_ten_minutes() -> anyhow::Result<()> {
    let (mut db, page, uno) = diario()?;
    // The page had no blocks: nothing worth keeping.
    assert!(db.page_versions(&page)?.is_empty());

    let edit = |db: &mut Database, minute: u32, text: &str| {
        db.set_clock(Some(&at(minute)));
        let changed = Block {
            content: text.into(),
            ..uno.clone()
        };
        db.save_document(&page, "Diario", &[changed])
    };
    edit(&mut db, 1, "dos")?;
    let versions = db.page_versions(&page)?;
    assert_eq!(versions.len(), 1, "the state before the first edit");
    assert_eq!(texts(&db, versions[0].id), ["uno"]);
    assert_eq!(versions[0].created_at, at(1));
    assert_eq!(versions[0].title, "Diario");

    // Typing on for 9 more minutes: no new version.
    for minute in 2..=10 {
        edit(&mut db, minute, &format!("texto {minute}"))?;
    }
    assert_eq!(db.page_versions(&page)?.len(), 1);
    // Saving without changes never makes one, however late.
    db.set_clock(Some(&at(30)));
    db.save_document(&page, "Diario", &db.list_blocks(&page)?)?;
    assert_eq!(db.page_versions(&page)?.len(), 1);
    // The next edit, more than 10 minutes later, keeps the state before it.
    edit(&mut db, 31, "tres")?;
    let versions = db.page_versions(&page)?;
    assert_eq!(versions.len(), 2);
    assert_eq!(texts(&db, versions[0].id), ["texto 10"], "newest first");
    // A title change counts too.
    db.set_clock(Some(&at(45)));
    db.save_document(&page, "Diario de octubre", &db.list_blocks(&page)?)?;
    let versions = db.page_versions(&page)?;
    assert_eq!(versions.len(), 3);
    assert_eq!(texts(&db, versions[0].id), ["tres"]);
    assert_eq!(versions[0].title, "Diario");
    Ok(())
}

#[test]
fn saving_a_version_by_hand_skips_the_wait_but_not_copies() -> anyhow::Result<()> {
    let (mut db, page, _) = diario()?;
    db.set_clock(Some(&at(1)));
    assert!(
        db.save_version(&page)?,
        "before restoring, whatever the time"
    );
    assert!(!db.save_version(&page)?, "the same as the last one");
    let mut blocks = db.list_blocks(&page)?;
    blocks.push(block(&page, "dos"));
    db.save_document(&page, "Diario", &blocks)?;
    assert_eq!(db.page_versions(&page)?.len(), 1, "within 10 minutes");
    assert!(db.save_version(&page)?);
    let versions = db.page_versions(&page)?;
    assert_eq!(texts(&db, versions[0].id), ["uno", "dos"]);
    assert!(db.save_version("no-existe").is_err());
    Ok(())
}

#[test]
fn a_failed_save_leaves_no_version() -> anyhow::Result<()> {
    let (mut db, page, uno) = diario()?;
    db.set_clock(Some(&at(20)));
    let bad = Block {
        kind: BlockKind::File,
        ..block(&page, "sin archivo")
    };
    assert!(db.save_document(&page, "Diario", &[uno, bad]).is_err());
    assert!(db.page_versions(&page)?.is_empty());
    Ok(())
}

#[test]
fn old_versions_are_thinned_out_by_day_and_week() -> anyhow::Result<()> {
    let (mut db, page, uno) = diario()?;
    // One edit a day at 09:00 and 18:00, for 60 days from 2026-08-01.
    let mut text = 0;
    for day in 0..60 {
        for hour in [9, 18] {
            text += 1;
            let date = chrono_free_date(day);
            db.set_clock(Some(&format!("{date}T{hour:02}:00:00.000Z")));
            let changed = Block {
                content: format!("edición {text}"),
                ..uno.clone()
            };
            db.save_document(&page, "Diario", &[changed])?;
        }
    }
    let versions = db.page_versions(&page)?;
    // The last day: both; then one a day for 29 days; then one a week.
    let days: Vec<&str> = versions.iter().map(|v| &v.created_at[..10]).collect();
    assert_eq!(&days[..2], ["2026-09-29", "2026-09-29"]);
    assert_eq!(days[2], "2026-09-28");
    let daily = days.iter().filter(|d| **d >= "2026-08-31").count();
    assert_eq!(daily, 2 + 29, "{days:?}");
    let weekly = &days[daily..];
    assert!((4..=6).contains(&weekly.len()), "{days:?}");
    // The one kept of each day is its last (18:00).
    assert!(
        versions[2..]
            .iter()
            .all(|v| v.created_at.ends_with("T18:00:00.000Z")),
        "{versions:?}"
    );
    Ok(())
}

/// `2026-08-01` plus `day` days, as `YYYY-MM-DD` (August has 31 days).
fn chrono_free_date(day: u32) -> String {
    let (month, day) = if day < 31 {
        (8, day + 1)
    } else {
        (9, day - 30)
    };
    format!("2026-{month:02}-{day:02}")
}

#[test]
fn files_named_in_a_version_are_kept_when_freeing_space() -> anyhow::Result<()> {
    let (mut db, page, uno) = diario()?;
    let file = db.add_attachment(&NewAttachment::from_bytes(
        "acta.pdf",
        b"%PDF-1.7 acta".to_vec(),
    )?)?;
    let file_block = Block {
        kind: BlockKind::File,
        content: "acta.pdf".into(),
        attachment: Some(file.id.clone()),
        ..block(&page, "")
    };
    let other = db.add_attachment(&NewAttachment::from_bytes(
        "suelto.pdf",
        b"%PDF-1.7 suelto".to_vec(),
    )?)?;
    db.set_clock(Some(&at(1)));
    db.save_document(&page, "Diario", &[uno.clone(), file_block])?;
    // 20 minutes later the file block is deleted: the version from before
    // still has it.
    db.set_clock(Some(&at(21)));
    db.save_document(&page, "Diario", std::slice::from_ref(&uno))?;
    let versions = db.page_versions(&page)?;
    let old = db
        .version_blocks(versions[0].id)?
        .expect("exists")
        .expect("readable");
    assert_eq!(old[1].attachment.as_deref(), Some(file.id.as_str()));

    let usage = db.space_usage()?;
    assert_eq!(usage.used_attachments, 1, "the version uses it");
    assert_eq!(usage.unused_attachments, 1);
    let (count, _) = db.delete_unused_attachments(&HashSet::new())?;
    assert_eq!(count, 1, "only the one nobody names");
    assert!(db.attachment(&file.id)?.is_some());
    assert!(db.attachment(&other.id)?.is_none());
    Ok(())
}

#[test]
fn versions_stay_with_a_page_in_the_trash() -> anyhow::Result<()> {
    let (mut db, page, _) = diario()?;
    db.save_version(&page)?;
    db.trash_page(&page)?;
    assert_eq!(db.page_versions(&page)?.len(), 1);
    db.restore_page(&page)?;
    assert_eq!(db.page_versions(&page)?.len(), 1);
    Ok(())
}

#[test]
fn an_unreadable_version_says_so_and_the_others_still_read() -> anyhow::Result<()> {
    let path = TempPath::new();
    let page = {
        let mut db = Database::open(&path.0)?;
        let page = db.create_page("Diario", None)?;
        db.save_document(&page, "Diario", &[block(&page, "uno")])?;
        db.save_version(&page)?;
        page
    };
    // A newer Silex wrote a format this one does not know.
    rusqlite::Connection::open(&path.0)?.execute(
        "INSERT INTO page_versions (page_id, title, format, blocks, size)
         VALUES (?1, 'Diario', 2, '{}', 2)",
        [&page],
    )?;
    let db = Database::open(&path.0)?;
    let versions = db.page_versions(&page)?;
    assert_eq!(versions.len(), 2);
    let read: Vec<bool> = versions
        .iter()
        .map(|v| matches!(db.version_blocks(v.id), Ok(Some(Ok(_)))))
        .collect();
    assert_eq!(read.iter().filter(|ok| **ok).count(), 1);
    let unreadable = versions
        .iter()
        .find_map(|v| db.version_blocks(v.id).ok().flatten().and_then(Result::err))
        .expect("one unreadable");
    assert!(unreadable.contains("más reciente"), "{unreadable}");
    assert_eq!(db.version_blocks(-1)?, None);
    // The schema refuses damaged JSON or a wrong size.
    let conn = rusqlite::Connection::open(&path.0)?;
    for (blocks, size) in [("no es json", 10), ("[]", 3)] {
        assert!(
            conn.execute(
                "INSERT INTO page_versions (page_id, title, blocks, size) VALUES (?1, 'x', ?2, ?3)",
                rusqlite::params![page, blocks, size],
            )
            .is_err()
        );
    }
    Ok(())
}

struct TempPath(PathBuf);

impl TempPath {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("silex-versions-{}.sqlite3", uuid::Uuid::new_v4())))
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

/// Manual: `cargo test --release --test page_versions -- --ignored --nocapture`.
/// A page of 100 blocks edited every 10 minutes, 8 hours a day, for a
/// year. Prints timings and sizes, never fails on speed.
#[test]
#[ignore = "scale smoke test; run manually"]
fn a_year_of_editing_scale_smoke() -> anyhow::Result<()> {
    use std::time::{Duration, Instant};
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Diario", None)?;
    let mut blocks: Vec<Block> = (0..100)
        .map(|i| {
            block(
                &page,
                &format!("Una línea de texto normal, la número {i}, como un párrafo."),
            )
        })
        .collect();
    let mut slowest = Duration::ZERO;
    let start = Instant::now();
    let mut saves = 0;
    for day in 0..365u32 {
        for slot in 0..48u32 {
            // 2025-10-20 plus `day` days, from 09:00, every 10 minutes.
            let minutes = slot * 10;
            let stamp: String = rusqlite::Connection::open_in_memory()?.query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', '2025-10-20T09:00:00Z', ?1, ?2)",
                [format!("+{day} days"), format!("+{minutes} minutes")],
                |r| r.get(0),
            )?;
            db.set_clock(Some(&stamp));
            blocks[(slot as usize) % 100].content.push('.');
            let t = Instant::now();
            db.save_document(&page, "Diario", &blocks)?;
            slowest = slowest.max(t.elapsed());
            saves += 1;
        }
    }
    let versions = db.page_versions(&page)?;
    let bytes: u64 = versions.iter().map(|v| v.size).sum();
    println!(
        "{saves} guardados en {:?} (el más lento, {slowest:?}); quedan {} versiones, {} KB",
        start.elapsed(),
        versions.len(),
        bytes / 1024
    );
    Ok(())
}
