//! Abrupt exits (H9.1): a child process saves, attaches, backs up,
//! migrates, compacts or exports in a loop and is killed (SIGKILL on Unix,
//! TerminateProcess on Windows) after varying delays. Afterwards the
//! database must pass the same checks as a backup (integrity, schema, FTS
//! indexes) and keep at least what the child reported as saved.
//!
//! This is a process crash, not a power cut: the operating system still
//! writes what it had in its cache. A power cut relies on SQLite with WAL
//! and `synchronous=FULL` (and `sync_all` before renaming exports), which
//! cannot be simulated here.
//!
//! The child is this same test binary, run again with `SILEX_CRASH_CHILD`
//! naming the scenario: the test `crash_child` runs it and does nothing
//! otherwise.
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use silex_desktop::attachment::NewAttachment;
use silex_desktop::db::{Database, backup};
use silex_desktop::model::{Block, BlockKind};

const CHILD: &str = "SILEX_CRASH_CHILD";
const DB: &str = "SILEX_CRASH_DB";
const PAGE: &str = "SILEX_CRASH_PAGE";
/// Lines the child prints for the parent start with this.
const MARK: &str = "SILEX ";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-crash-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- The child ----

/// Runs the scenario in `SILEX_CRASH_CHILD`, if any; otherwise nothing.
#[test]
fn crash_child() -> anyhow::Result<()> {
    let Ok(scenario) = std::env::var(CHILD) else {
        return Ok(());
    };
    let path = PathBuf::from(std::env::var(DB)?);
    let say = |line: String| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{MARK}{line}");
        let _ = out.flush();
    };
    match scenario.as_str() {
        "save" => {
            let page = std::env::var(PAGE)?;
            let mut db = Database::open(&path)?;
            say("ready".into());
            for n in 1.. {
                let mut blocks = blocks_for(&page, n);
                // Every few saves, a file too: a 1 MB attachment.
                if n % 5 == 0 {
                    let data = vec![(n % 251) as u8; 1_000_000];
                    let file = db.add_attachment(&NewAttachment::from_bytes(
                        &format!("archivo {n}.bin"),
                        data,
                    )?)?;
                    blocks.push(Block {
                        kind: BlockKind::File,
                        content: file.name.clone(),
                        attachment: Some(file.id),
                        ..block(&page, "f", "")
                    });
                }
                db.save_document(&page, &format!("Diario {n}"), &blocks)?;
                say(format!("saved {n}"));
            }
        }
        "backup" => {
            let dir = backup::backups_dir(&path);
            say("ready".into());
            loop {
                backup::create_backup(&path, &dir)?;
                say("backup".into());
                // Backups are named by the second.
                std::thread::sleep(Duration::from_millis(1001));
            }
        }
        "migrate" | "compact" => {
            say("ready".into());
            let mut db = Database::open(&path)?;
            if scenario == "compact" {
                silex_desktop::space::free_space(&mut db, &HashSet::new())?;
            }
            say("done".into());
        }
        "export" => {
            let db = Database::open(&path)?;
            let root = path.with_file_name("exportaciones");
            say("ready".into());
            loop {
                silex_desktop::export::export_workspace(&db, &root)?;
                say("exported".into());
                std::thread::sleep(Duration::from_millis(1001));
            }
        }
        other => anyhow::bail!("escenario desconocido: {other}"),
    }
    Ok(())
}

fn block(page: &str, id: &str, content: &str) -> Block {
    Block {
        id: format!("{id}-{}", uuid::Uuid::new_v4()),
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

/// 40 blocks that all say which save they belong to.
fn blocks_for(page: &str, n: u64) -> Vec<Block> {
    (0..40)
        .map(|i| {
            block(
                page,
                "b",
                &format!("guardado {n}, bloque {i}: texto de prueba"),
            )
        })
        .collect()
}

// ---- The parent ----

/// What the child reported before it was killed.
struct Run {
    lines: Vec<String>,
    /// The child finished by itself (it was not killed in time).
    finished: bool,
    /// How it ended, and what it wrote to stderr (to explain a failure).
    status: std::process::ExitStatus,
    stderr: String,
}

impl Run {
    /// A child that ends by itself must have reported `done`: one that
    /// failed would otherwise look like a kill.
    fn check_finished_well(&self) -> anyhow::Result<()> {
        if self.finished && !self.lines.iter().any(|l| l == "done") {
            anyhow::bail!(
                "el proceso hijo terminó sin acabar ({}): {:?}\n{}",
                self.status,
                self.lines,
                self.stderr
            );
        }
        Ok(())
    }

    /// A child that works in a loop never ends by itself.
    fn check_was_killed(&self) -> anyhow::Result<()> {
        if self.finished {
            anyhow::bail!(
                "el proceso hijo se paró solo ({}): {:?}\n{}",
                self.status,
                self.lines,
                self.stderr
            );
        }
        Ok(())
    }
}

/// Runs `scenario` in a child on `db`, kills it `delay` after it is ready
/// (or lets it finish), and returns what it said.
fn run_child(scenario: &str, db: &Path, page: &str, delay: Duration) -> anyhow::Result<Run> {
    let exe = std::env::current_exe()?;
    let mut child = Command::new(exe)
        .args(["crash_child", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, scenario)
        .env(DB, db)
        .env(PAGE, page)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("sin salida"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("sin salida de errores"))?;
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut text);
        text
    });
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            // The test runner may print «test crash_child ... » first, on
            // the same line.
            if let Some(at) = line.find(MARK) {
                let _ = tx.send(line[at + MARK.len()..].to_owned());
            }
        }
    });
    let mut lines = Vec::new();
    // Wait until it is ready (at most 30 s), then the delay.
    let start = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                let ready = line == "ready";
                lines.push(line);
                if ready {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) if start.elapsed() < Duration::from_secs(30) => {}
            Err(_) => {
                // Never leave it running.
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("el proceso hijo no arrancó: {lines:?}");
            }
        }
    }
    std::thread::sleep(delay);
    let finished = child.try_wait()?.is_some();
    if !finished {
        child.kill()?;
    }
    let status = child.wait()?;
    let _ = reader.join();
    let stderr = errors.join().unwrap_or_default();
    lines.extend(rx.try_iter());
    Ok(Run {
        lines,
        finished,
        status,
        stderr,
    })
}

/// Delays after «ready»: from almost at once to well into the work.
const DELAYS_MS: [u64; 8] = [0, 3, 10, 25, 60, 120, 250, 500];
/// Longer ones too, for work that takes seconds (an upgrade, an export):
/// killed after it finished, as well as during it.
const LONG_DELAYS_MS: [u64; 11] = [0, 3, 10, 25, 60, 120, 250, 500, 1000, 2000, 4000];

/// The checks a backup passes (integrity, schema, FTS indexes), on the
/// live file after a crash.
fn assert_healthy(path: &Path) -> anyhow::Result<()> {
    backup::verify_backup(path)
        .map_err(|err| anyhow::anyhow!("la base no está sana tras el cierre: {err:#}"))
}

#[test]
fn killed_while_saving_keeps_the_last_save() -> anyhow::Result<()> {
    let tmp = TempDir::new("save")?;
    let path = tmp.0.join("silex.sqlite3");
    let page = Database::open(&path)?.create_page("Diario 0", None)?;
    for delay in DELAYS_MS {
        let run = run_child("save", &path, &page, Duration::from_millis(delay))?;
        run.check_was_killed()?;
        let acked: u64 = run
            .lines
            .iter()
            .filter_map(|l| l.strip_prefix("saved "))
            .filter_map(|n| n.parse().ok())
            .max()
            .unwrap_or(0);
        assert_healthy(&path)?;
        let db = Database::open(&path)?;
        let title = db.get_page(&page)?.map(|p| p.title).unwrap_or_default();
        let n: u64 = title.trim_start_matches("Diario ").parse()?;
        // The last reported save is there; at most the one in flight
        // after it, whole.
        assert!(
            n >= acked && n <= acked + 1,
            "{title}, último confirmado {acked}"
        );
        eprintln!("guardar, {delay} ms: confirmados {acked}, en la base {n}");
        let blocks = db.list_blocks(&page)?;
        if n > 0 {
            let text: Vec<&Block> = blocks
                .iter()
                .filter(|b| b.kind == BlockKind::Paragraph)
                .collect();
            assert_eq!(text.len(), 40, "{delay} ms");
            assert!(
                text.iter()
                    .all(|b| b.content.starts_with(&format!("guardado {n},"))),
                "{delay} ms: mezcla de guardados"
            );
            assert_eq!(db.search_pages(&format!("guardado {n}"))?.len(), 1);
        }
        // Every file block has its file.
        for file in blocks.iter().filter(|b| b.kind == BlockKind::File) {
            let id = file.attachment.as_deref().unwrap_or_default();
            assert_eq!(db.attachment_data(id)?.len(), 1_000_000);
        }
    }
    Ok(())
}

#[test]
fn killed_while_backing_up_leaves_only_good_copies() -> anyhow::Result<()> {
    let tmp = TempDir::new("backup")?;
    let path = tmp.0.join("silex.sqlite3");
    {
        let mut db = Database::open(&path)?;
        let page = db.create_page("Notas", None)?;
        // Some weight, so a copy takes a while.
        for n in 0..8 {
            let data = vec![n as u8; 2_000_000];
            let file = db.add_attachment(&NewAttachment::from_bytes(&format!("{n}.bin"), data)?)?;
            let mut blocks = db.list_blocks(&page)?;
            blocks.push(Block {
                kind: BlockKind::File,
                content: file.name.clone(),
                attachment: Some(file.id),
                ..block(&page, "f", "")
            });
            db.save_document(&page, "Notas", &blocks)?;
        }
    }
    let dir = backup::backups_dir(&path);
    for delay in DELAYS_MS {
        let run = run_child("backup", &path, "", Duration::from_millis(delay))?;
        run.check_was_killed()?;
        let partials = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".partial"))
            .count();
        eprintln!(
            "copiar, {delay} ms: copias hechas {}, a medias {partials}",
            run.lines.iter().filter(|l| *l == "backup").count()
        );
        assert_healthy(&path)?;
        for copy in backup::list_backups(&dir)? {
            backup::verify_backup(&copy)
                .map_err(|err| anyhow::anyhow!("{}: {err:#}", copy.display()))?;
        }
    }
    // The next backup works, and no partial copy is left behind.
    std::thread::sleep(Duration::from_millis(1001));
    backup::create_backup(&path, &dir)?;
    let partials = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".partial"))
        .count();
    assert_eq!(partials, 0);
    Ok(())
}

/// A database left by Silex 1 (schema 1) with many blocks, so that the
/// upgrade (three rebuilds of `blocks`) takes a while.
fn write_v1_database(path: &Path, blocks: usize) -> anyhow::Result<()> {
    let conn = rusqlite::Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(include_str!("../src/db/migrations/0001_init.sql"))?;
    conn.execute(
        "INSERT INTO pages (id, title, sort_order) VALUES ('p', 'Antigua', 0)",
        [],
    )?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO blocks (id, page_id, kind, content, position) VALUES (?1, 'p', 'paragraph', ?2, ?3)",
        )?;
        for i in 0..blocks {
            insert.execute(rusqlite::params![
                format!("b{i}"),
                format!("bloque antiguo {i}"),
                i as i64
            ])?;
        }
    }
    tx.commit()?;
    conn.pragma_update(None, "user_version", 1)?;
    Ok(())
}

#[test]
fn killed_while_upgrading_is_either_old_or_new_never_half() -> anyhow::Result<()> {
    const BLOCKS: usize = 30_000;
    for delay in LONG_DELAYS_MS {
        let tmp = TempDir::new("migrate")?;
        let path = tmp.0.join("silex.sqlite3");
        write_v1_database(&path, BLOCKS)?;
        let run = run_child("migrate", &path, "", Duration::from_millis(delay))?;
        run.check_finished_well()?;
        let version: i64 =
            rusqlite::Connection::open(&path)?
                .pragma_query_value(None, "user_version", |r| r.get(0))?;
        assert!(
            version == 1 || version >= 9,
            "{delay} ms: esquema {version}"
        );
        eprintln!(
            "migrar, {delay} ms: esquema {version}, terminado {}",
            run.finished
        );
        // Whatever it was left as, it opens (upgrading if needed) with
        // every block.
        let db = Database::open(&path)?;
        assert_eq!(db.list_blocks("p")?.len(), BLOCKS, "{delay} ms");
        assert_eq!(db.search_pages("antiguo")?.len(), 1);
        drop(db);
        assert_healthy(&path)?;
        for copy in backup::list_backups(&backup::backups_dir(&path))? {
            backup::verify_backup(&copy)?;
        }
    }
    Ok(())
}

#[test]
fn killed_while_compacting_is_finished_on_the_next_start() -> anyhow::Result<()> {
    for delay in DELAYS_MS {
        let tmp = TempDir::new("compact")?;
        let path = tmp.0.join("silex.sqlite3");
        let page = {
            let mut db = Database::open(&path)?;
            let page = db.create_page("Notas", None)?;
            let blocks: Vec<Block> = (0..2_000)
                .map(|i| block(&page, "b", &format!("zanahoria {i}")))
                .collect();
            db.save_document(&page, "Notas", &blocks)?;
            // 30 MB nobody uses: «Liberar espacio» deletes and compacts.
            for n in 0..15 {
                db.add_attachment(&NewAttachment::from_bytes(
                    &format!("{n}.bin"),
                    vec![n as u8; 2_000_000],
                )?)?;
            }
            page
        };
        let run = run_child("compact", &path, "", Duration::from_millis(delay))?;
        run.check_finished_well()?;
        let marked: bool = rusqlite::Connection::open(&path)?.query_row(
            "SELECT EXISTS (SELECT 1 FROM settings WHERE key = 'compacting')",
            [],
            |r| r.get(0),
        )?;
        let size = std::fs::metadata(&path)?.len() / 1_000_000;
        eprintln!(
            "compactar, {delay} ms: terminado {}, marca pendiente {marked}, {size} MB",
            run.finished
        );
        // Opening finishes an interrupted compaction (the search indexes).
        let db = Database::open(&path)?;
        assert_eq!(db.list_blocks(&page)?.len(), 2_000, "{delay} ms");
        assert_eq!(db.search_pages("zanahoria")?.len(), 1, "{delay} ms");
        drop(db);
        assert_healthy(&path)?;
    }
    Ok(())
}

#[test]
fn killed_while_exporting_never_leaves_a_half_export() -> anyhow::Result<()> {
    let tmp = TempDir::new("export")?;
    let path = tmp.0.join("silex.sqlite3");
    {
        let mut db = Database::open(&path)?;
        for p in 0..60 {
            let page = db.create_page(&format!("Página {p}"), None)?;
            let blocks: Vec<Block> = (0..30)
                .map(|i| block(&page, "b", &format!("texto {i} de la página {p}")))
                .collect();
            db.save_document(&page, &format!("Página {p}"), &blocks)?;
        }
    }
    let root = tmp.0.join("exportaciones");
    let mut finished = 0;
    for delay in LONG_DELAYS_MS {
        let run = run_child("export", &path, "", Duration::from_millis(delay))?;
        run.check_was_killed()?;
        let hidden = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".silex-"))
            .count();
        eprintln!(
            "exportar, {delay} ms: exportaciones {}, temporales {hidden}",
            run.lines.iter().filter(|l| *l == "exported").count()
        );
        assert_healthy(&path)?;
        // Every finished export is whole; only hidden temporaries may be
        // left by a kill.
        for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".silex-") {
                continue;
            }
            assert!(name.starts_with("Silex "), "{name}");
            let pages = std::fs::read_dir(entry.path())?.count();
            assert_eq!(pages, 60, "{name}: exportación a medias");
            finished += 1;
        }
    }
    eprintln!("exportar: {finished} exportaciones completas comprobadas");
    // What the kills left is hidden, and the next export clears it once it
    // is old (here: at once).
    assert!(silex_desktop::export::remove_stale_temporaries(&root, Duration::ZERO) > 0);
    let left = std::fs::read_dir(&root)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(".silex-"))
        .count();
    assert_eq!(left, 0);
    Ok(())
}
