use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

use rusqlite::Connection;
use silex_desktop::db::{
    Database,
    backup::{self, BackupJob},
};

/// Temporary directory holding a database and its `backups/` folder.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-backup-{}", uuid::Uuid::new_v4()));
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

fn names(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(dir)?
        .map(|e| Ok(e?.file_name().to_string_lossy().into_owned()))
        .collect::<anyhow::Result<_>>()?;
    names.sort();
    Ok(names)
}

fn seed(db: &mut Database) -> anyhow::Result<String> {
    let page_id = db.create_page("Diario de viaje", None)?;
    db.add_block(&page_id, "paragraph")?;
    db.add_block(&page_id, "todo")?;
    let mut blocks = db.list_blocks(&page_id)?;
    blocks[0].content = "Llegada a Reikiavik 🌋".into();
    blocks[1].content = "Reservar excursión".into();
    blocks[1].checked = true;
    db.save_document(&page_id, "Diario de viaje", &blocks)?;
    Ok(page_id)
}

#[test]
fn backup_copies_content_including_wal_and_search_works() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    // Keep the source open: recent writes are still in the -wal file.
    let mut db = Database::open(tmp.db())?;
    let page_id = seed(&mut db)?;

    let path = backup::create_backup(&tmp.db(), &tmp.backups())?;

    let copy = Database::open(&path)?;
    assert_eq!(
        copy.get_page(&page_id)?.map(|p| p.title).as_deref(),
        Some("Diario de viaje")
    );
    let blocks = copy.list_blocks(&page_id)?;
    assert_eq!(blocks[0].content, "Llegada a Reikiavik 🌋");
    assert!(blocks[1].checked);
    assert_eq!(copy.search_pages("Reikiavik")?.len(), 1);
    Ok(())
}

#[test]
fn backup_is_a_single_self_contained_file() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    seed(&mut db)?;
    let path = backup::create_backup(&tmp.db(), &tmp.backups())?;

    let file = path.file_name().map(|n| n.to_string_lossy().into_owned());
    assert_eq!(names(&tmp.backups())?, vec![file.unwrap_or_default()]);
    let conn = Connection::open(&path)?;
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    assert_eq!(mode, "delete");
    Ok(())
}

#[test]
fn verify_rejects_damaged_or_foreign_files() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    seed(&mut db)?;
    let good = backup::create_backup(&tmp.db(), &tmp.backups())?;
    backup::verify_backup(&good)?;

    let garbage = tmp.0.join("basura.sqlite3");
    fs::write(
        &garbage,
        b"esto no es una base de datos SQLite, solo texto suelto",
    )?;
    assert!(backup::verify_backup(&garbage).is_err());

    let truncated = tmp.0.join("truncada.sqlite3");
    let bytes = fs::read(&good)?;
    fs::write(&truncated, &bytes[..bytes.len() / 2])?;
    assert!(backup::verify_backup(&truncated).is_err());

    let foreign = tmp.0.join("otra-app.sqlite3");
    Connection::open(&foreign)?.execute_batch("CREATE TABLE t (x);")?;
    assert!(backup::verify_backup(&foreign).is_err());

    assert!(backup::verify_backup(&tmp.0.join("no-existe.sqlite3")).is_err());
    assert!(!tmp.0.join("no-existe.sqlite3").exists());
    Ok(())
}

#[test]
fn verify_detects_search_index_out_of_sync() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    seed(&mut db)?;
    let copy = backup::create_backup(&tmp.db(), &tmp.backups())?;

    // Change a title without its trigger: the FTS index no longer matches.
    Connection::open(&copy)?.execute_batch(
        "DROP TRIGGER pages_search_au; UPDATE pages SET title = 'Título cambiado a escondidas';",
    )?;
    assert!(backup::verify_backup(&copy).is_err());
    Ok(())
}

#[test]
fn rotation_keeps_newest_and_ignores_other_files() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let dir = tmp.backups();
    fs::create_dir_all(&dir)?;
    for day in 1..=12 {
        fs::write(
            dir.join(format!("silex-202601{day:02}-120000.sqlite3")),
            b"",
        )?;
    }
    fs::write(dir.join("notas.txt"), b"mio")?;
    fs::write(dir.join("silex-manual.sqlite3"), b"mio")?;

    assert_eq!(backup::rotate(&dir, 10)?, 2);
    let left = names(&dir)?;
    assert!(!left.contains(&"silex-20260101-120000.sqlite3".to_owned()));
    assert!(!left.contains(&"silex-20260102-120000.sqlite3".to_owned()));
    assert!(left.contains(&"silex-20260112-120000.sqlite3".to_owned()));
    assert!(left.contains(&"notas.txt".to_owned()));
    assert!(left.contains(&"silex-manual.sqlite3".to_owned()));
    assert_eq!(backup::list_backups(&dir)?.len(), 10);
    Ok(())
}

#[test]
fn failed_backup_leaves_no_partial_and_keeps_old_copies() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let dir = tmp.backups();
    fs::create_dir_all(&dir)?;
    let old = dir.join("silex-20260101-120000.sqlite3");
    fs::write(&old, b"copia anterior")?;
    fs::write(
        dir.join("silex-abandonada.partial"),
        b"resto de una copia interrumpida",
    )?;
    fs::write(tmp.db(), b"archivo de origen corrupto, no es SQLite")?;

    assert!(backup::create_backup(&tmp.db(), &dir).is_err());
    assert_eq!(names(&dir)?, vec!["silex-20260101-120000.sqlite3"]);
    assert_eq!(fs::read(&old)?, b"copia anterior");
    Ok(())
}

#[test]
fn backup_due_follows_newest_backup_age() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    seed(&mut db)?;
    let dir = tmp.backups();

    assert!(db.backup_due(&dir)?, "no backups yet");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("silex-20000101-000000.sqlite3"), b"")?;
    assert!(db.backup_due(&dir)?, "only a very old backup");

    backup::create_backup(&tmp.db(), &dir)?;
    assert!(!db.backup_due(&dir)?, "fresh backup exists");
    Ok(())
}

#[test]
fn in_memory_database_has_no_path() -> anyhow::Result<()> {
    assert_eq!(Database::open_in_memory()?.path(), None);
    let tmp = TempDir::new()?;
    assert!(Database::open(tmp.db())?.path().is_some());
    Ok(())
}

#[test]
fn background_job_backs_up_rotates_and_notifies() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    let page_id = seed(&mut db)?;
    let dir = tmp.backups();
    fs::create_dir_all(&dir)?;
    for day in 1..=10 {
        fs::write(
            dir.join(format!("silex-202601{day:02}-120000.sqlite3")),
            b"",
        )?;
    }

    let (done_tx, done_rx) = mpsc::channel();
    let job = BackupJob::spawn(tmp.db(), dir.clone(), move || {
        let _ = done_tx.send(());
    })?;
    done_rx.recv_timeout(Duration::from_secs(30))?;
    let path = job.poll().expect("finished job has a result")?;

    assert!(Database::open(&path)?.get_page(&page_id)?.is_some());
    let kept = backup::list_backups(&dir)?;
    assert_eq!(kept.len(), backup::KEEP);
    assert_eq!(kept.first(), Some(&path));
    assert!(!dir.join("silex-20260101-120000.sqlite3").exists());
    Ok(())
}

#[test]
fn backup_names_are_parsed_strictly() {
    assert_eq!(
        backup::stamp_of(Path::new("/x/silex-20261018-093005.sqlite3")),
        Some("20261018-093005")
    );
    for other in [
        "silex-manual.sqlite3",
        "silex-20261018-093005.sqlite3-wal",
        "silex-2026101-093005.sqlite3",
        "silex-20261018_093005.sqlite3",
        "otra-20261018-093005.sqlite3",
    ] {
        assert_eq!(backup::stamp_of(Path::new(other)), None, "{other}");
    }
    assert_eq!(
        backup::describe_stamp("20261018-093005"),
        "2026-10-18 09:30 UTC"
    );
}
