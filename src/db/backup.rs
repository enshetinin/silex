//! Consistent, verified copies of the database using the SQLite backup API.
//!
//! The backup API copies database pages, so implicit `rowid`s (which link the
//! FTS indexes to their tables) are preserved, unlike `VACUUM INTO`.
//! A copy is written to a `.partial` file, verified, and only then renamed to
//! its final name, so a finished-looking backup is always a verified one.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use uuid::Uuid;

use super::LATEST_SCHEMA_VERSION;

/// Number of automatic backups kept; older ones are deleted after a new verified copy.
pub const KEEP: usize = 10;
/// A new automatic backup is made at startup when the newest one is older than this.
pub const MAX_AGE_HOURS: u32 = 24;

const PREFIX: &str = "silex-";
const EXTENSION: &str = ".sqlite3";
const PARTIAL_EXTENSION: &str = ".partial";

/// Backups live in `backups/` next to the database file.
pub fn backups_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

/// Creates a verified backup of `source` in `dir` and returns its path.
pub fn create_backup(source: &Path, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("No se puede crear la carpeta de copias {}", dir.display()))?;
    // Copies hold everything: only their owner reads them (ADR 0012).
    crate::privacy::restrict(dir, true)?;
    remove_stale_partials(dir);

    let src = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("No se puede abrir {} para copiarla", source.display()))?;
    src.busy_timeout(Duration::from_secs(5))?;
    // Copies are named by the second. One already made this second (two
    // in a row, or a start right after a crash during an upgrade, H9.1):
    // wait for the next second once.
    let target_now = || -> Result<PathBuf> {
        let stamp: String =
            src.query_row("SELECT strftime('%Y%m%d-%H%M%S', 'now')", [], |r| r.get(0))?;
        Ok(dir.join(format!("{PREFIX}{stamp}{EXTENSION}")))
    };
    let mut target = target_now()?;
    if target.exists() {
        std::thread::sleep(Duration::from_millis(1000));
        target = target_now()?;
    }
    if target.exists() {
        bail!("Ya existe una copia de este mismo segundo; inténtalo de nuevo");
    }

    let partial = dir.join(format!("{PREFIX}{}{PARTIAL_EXTENSION}", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        src.backup(MAIN_DB, &partial, None)
            .context("La copia de SQLite falló")?;
        crate::privacy::restrict(&partial, false)?;
        make_standalone(&partial)?;
        verify_backup(&partial)?;
        fs::rename(&partial, &target).context("No se puede finalizar la copia")?;
        Ok(())
    })();
    if result.is_err() {
        remove_with_sidecars(&partial);
    }
    result.map(|()| target)
}

/// Checks that a backup is a healthy Silex database: SQLite integrity, the
/// expected schema version and FTS indexes consistent with their tables.
pub fn verify_backup(path: &Path) -> Result<()> {
    // READ_WRITE without CREATE: a missing file is an error, never a new database.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| format!("No se puede abrir la copia {}", path.display()))?;
    let problems: Vec<String> = conn
        .prepare("PRAGMA integrity_check")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()
        .context("La copia no es una base SQLite válida")?;
    ensure!(
        problems == ["ok"],
        "La copia está dañada: {}",
        problems.join("; ")
    );

    // Older schema versions are valid: restoring upgrades them.
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(
        (1..=LATEST_SCHEMA_VERSION).contains(&version),
        "La copia tiene un esquema desconocido ({version})"
    );

    // Table names are constants, not user input. rank = 1 also compares the
    // index with the content table, which catches rowid drift.
    for table in ["page_search", "block_search"] {
        conn.execute(
            &format!("INSERT INTO {table}({table}, rank) VALUES ('integrity-check', 1)"),
            [],
        )
        .with_context(|| format!("Índice de búsqueda inconsistente en la copia ({table})"))?;
    }
    Ok(())
}

/// Finished backups in `dir`, newest first. Other files are ignored.
pub fn list_backups(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).context("No se puede leer la carpeta de copias")? {
        let path = entry?.path();
        if stamp_of(&path).is_some() {
            found.push(path);
        }
    }
    // Names embed a fixed-width UTC timestamp, so name order is age order.
    found.sort();
    found.reverse();
    Ok(found)
}

/// UTC timestamp (`YYYYMMDD-HHMMSS`) of the newest backup, if any.
pub fn newest_stamp(dir: &Path) -> Result<Option<String>> {
    Ok(list_backups(dir)?
        .first()
        .and_then(|p| stamp_of(p))
        .map(str::to_owned))
}

/// How many backups `dir` holds and their total size in bytes (H7.4).
pub fn backups_size(dir: &Path) -> Result<(usize, u64)> {
    let backups = list_backups(dir)?;
    let bytes = backups
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
    Ok((backups.len(), bytes))
}

/// Deletes the oldest backups beyond `keep`; returns how many were removed.
pub fn rotate(dir: &Path, keep: usize) -> Result<usize> {
    let mut removed = 0;
    for old in list_backups(dir)?.iter().skip(keep) {
        fs::remove_file(old)
            .with_context(|| format!("No se puede borrar la copia antigua {}", old.display()))?;
        removed += 1;
    }
    Ok(removed)
}

/// A backup running on a worker thread so the UI never waits for disk I/O.
pub struct BackupJob {
    rx: mpsc::Receiver<Result<PathBuf>>,
}

impl BackupJob {
    /// Starts backup + rotation; `on_done` runs on the worker when finished
    /// (the UI uses it to request a repaint).
    pub fn spawn(
        source: PathBuf,
        dir: PathBuf,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("silex-backup".into())
            .spawn(move || {
                let result = create_backup(&source, &dir).and_then(|path| {
                    rotate(&dir, KEEP).with_context(|| {
                        format!(
                            "Copia creada en {}, pero no se pudieron borrar las antiguas",
                            path.display()
                        )
                    })?;
                    Ok(path)
                });
                let _ = tx.send(result);
                on_done();
            })
            .context("No se puede iniciar el hilo de copias")?;
        Ok(Self { rx })
    }

    /// `None` while running; the result once, when finished.
    pub fn poll(&self) -> Option<Result<PathBuf>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow!("El hilo de copias terminó inesperadamente")))
            }
        }
    }
}

/// Copies inherit WAL mode from the source; switch to a single self-contained file.
fn make_standalone(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mode: String = conn.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
    ensure!(
        mode.eq_ignore_ascii_case("delete"),
        "No se pudo cerrar el diario de la copia ({mode})"
    );
    Ok(())
}

/// `silex-YYYYMMDD-HHMMSS.sqlite3` → `YYYYMMDD-HHMMSS`; `None` for other files.
pub fn stamp_of(path: &Path) -> Option<&str> {
    let stamp = path
        .file_name()?
        .to_str()?
        .strip_prefix(PREFIX)?
        .strip_suffix(EXTENSION)?;
    let valid = stamp.len() == 15
        && stamp
            .char_indices()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() });
    valid.then_some(stamp)
}

/// `YYYYMMDD-HHMMSS` → `YYYY-MM-DD HH:MM UTC` for display.
pub fn describe_stamp(stamp: &str) -> String {
    match (
        stamp.get(0..4),
        stamp.get(4..6),
        stamp.get(6..8),
        stamp.get(9..11),
        stamp.get(11..13),
    ) {
        (Some(y), Some(mo), Some(d), Some(h), Some(mi)) => format!("{y}-{mo}-{d} {h}:{mi} UTC"),
        _ => stamp.to_owned(),
    }
}

/// Leftovers from an interrupted backup (e.g. the app closed mid-copy).
fn remove_stale_partials(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        let is_partial = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(PARTIAL_EXTENSION));
        if is_partial {
            remove_with_sidecars(&path);
        }
    }
}

fn remove_with_sidecars(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
