//! All SQL lives here. Callers never construct SQL from user input.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use directories::ProjectDirs;
use rusqlite::{Connection, MAIN_DB, OptionalExtension, params};
use uuid::Uuid;

use crate::model::{
    Block, BlockKind, DatabaseView, MAX_INDENT, NewBlock, Page, PageKind, TrashItem, ViewKind,
};
use crate::search::{self, BlockMatch, Candidate, SearchHit};
use crate::settings::Settings;

pub mod backup;

/// Published migrations, in order. Never edit one that has shipped: add a new file.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("db/migrations/0001_init.sql")),
    (2, include_str!("db/migrations/0002_favorites.sql")),
    (3, include_str!("db/migrations/0003_block_kinds.sql")),
    (4, include_str!("db/migrations/0004_settings.sql")),
    (5, include_str!("db/migrations/0005_databases.sql")),
];
const LATEST_SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].0;

/// Whether to back up an existing database before upgrading its schema.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PreMigrationBackup {
    Yes,
    No,
}

/// Direction for [`Database::reorder_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shift {
    Up,
    Down,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// Where the user's database lives. `SILEX_DB_PATH` overrides it
    /// (development and manual testing, so real data stays untouched).
    /// The folder is `dev.silex.Silex` on macOS, matching the app's bundle
    /// identifier (ADR 0005).
    pub fn default_path() -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("SILEX_DB_PATH") {
            return Ok(PathBuf::from(path));
        }
        let dirs = ProjectDirs::from("dev", "silex", "Silex")
            .context("No se encuentra el directorio de datos del sistema")?;
        Ok(dirs.data_local_dir().join("silex.sqlite3"))
    }

    /// Opens the user's database (see [`Database::default_path`]).
    pub fn open_default() -> Result<Self> {
        let path = Self::default_path()?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).context("No se puede crear el directorio de datos")?;
        }
        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("No se puede abrir SQLite")?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate(PreMigrationBackup::Yes)?;
        Ok(db)
    }

    /// Current UTC time as `YYYYMMDD-HHMMSS` (from SQLite, like backup names).
    pub fn utc_stamp(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT strftime('%Y%m%d-%H%M%S', 'now')", [], |r| r.get(0))?)
    }

    /// File backing this database; `None` for in-memory databases.
    pub fn path(&self) -> Option<PathBuf> {
        self.conn
            .path()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    }

    /// True when the newest backup in `dir` is missing or older than
    /// [`backup::MAX_AGE_HOURS`]. Uses SQLite's UTC clock, like backup names.
    pub fn backup_due(&self, dir: &Path) -> Result<bool> {
        let threshold: String = self.conn.query_row(
            "SELECT strftime('%Y%m%d-%H%M%S', 'now', ?1)",
            [format!("-{} hours", backup::MAX_AGE_HOURS)],
            |r| r.get(0),
        )?;
        Ok(backup::newest_stamp(dir)?.is_none_or(|newest| newest < threshold))
    }

    /// Replaces the whole database with `backup_file`.
    ///
    /// Order matters for safety: the chosen copy is verified first, then the
    /// current state is saved as a new verified backup, and only then is the
    /// live database overwritten. SQLite's backup API rolls back the
    /// destination if the copy does not finish, so a failure leaves the
    /// current data as it was. Returns the path of the pre-restore backup.
    pub fn restore_from_backup(&mut self, backup_file: &Path) -> Result<PathBuf> {
        let source = self
            .path()
            .context("No se puede restaurar sobre una base en memoria")?;
        backup::verify_backup(backup_file)
            .context("La copia elegida no es válida; no se ha cambiado nada")?;
        let safety = backup::create_backup(&source, &backup::backups_dir(&source)).context(
            "No se pudo guardar el estado actual antes de restaurar; no se ha cambiado nada",
        )?;
        self.conn
            .restore(MAIN_DB, backup_file, None::<fn(rusqlite::backup::Progress)>)
            .with_context(|| {
                format!(
                    "La restauración falló; el estado anterior está en {}",
                    safety.display()
                )
            })?;
        // The copy is a single-file (DELETE journal) database; restore WAL and pragmas.
        self.configure()?;
        // An older copy is upgraded in place. No extra backup: the chosen copy
        // itself is the old-schema version, and `safety` holds the prior state.
        self.migrate(PreMigrationBackup::No).with_context(|| {
            format!(
                "La copia se restauró pero no se pudo actualizar su esquema; el estado anterior está en {}",
                safety.display()
            )
        })?;
        Ok(safety)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate(PreMigrationBackup::No)?;
        Ok(db)
    }

    fn configure(&self) -> Result<()> {
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        self.conn.pragma_update(None, "synchronous", "FULL")?;
        self.conn.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    /// Applies all pending migrations in ONE transaction: the database ends
    /// either fully upgraded or exactly as it was. An existing file database
    /// is backed up (verified copy) before an upgrade.
    fn migrate(&mut self, pre_backup: PreMigrationBackup) -> Result<()> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > LATEST_SCHEMA_VERSION {
            bail!("La base de datos usa un esquema más reciente ({version})");
        }
        if version == LATEST_SCHEMA_VERSION {
            return Ok(());
        }
        if version > 0
            && pre_backup == PreMigrationBackup::Yes
            && let Some(path) = self.path()
        {
            backup::create_backup(&path, &backup::backups_dir(&path)).context(
                "No se pudo crear la copia previa a la actualización; la base no se ha modificado",
            )?;
        }
        let tx = self.conn.transaction()?;
        for (target, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > version) {
            tx.execute_batch(sql)
                .with_context(|| format!("La migración al esquema {target} falló"))?;
            tx.pragma_update(None, "user_version", target)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Schema version of this database (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// User preferences; defaults for anything missing or invalid.
    pub fn load_settings(&self) -> Result<Settings> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Settings::from_pairs(
            rows.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        ))
    }

    /// Stores every preference in one transaction.
    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        let tx = self.conn.transaction()?;
        for (key, value) in settings.to_pairs() {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Marks or unmarks a live page as favorite. Marking keeps the original
    /// date, so re-marking does not move it in the list.
    pub fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE pages SET favorite_at = CASE WHEN ?2
                 THEN COALESCE(favorite_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ELSE NULL END
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, favorite],
        )?;
        ensure!(affected == 1, "La página no existe o está en la papelera");
        Ok(())
    }

    /// Live favorite pages, in the order they were marked. Trashed favorites
    /// are hidden and come back if the page is restored.
    pub fn list_favorites(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title, kind FROM pages
             WHERE favorite_at IS NOT NULL AND deleted_at IS NULL
             ORDER BY favorite_at, id",
        )?;
        let rows = stmt.query_map([], |r| read_page(r, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title, kind FROM pages WHERE deleted_at IS NULL ORDER BY sort_order, created_at, id",
        )?;
        let rows = stmt.query_map([], |r| read_page(r, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_page(&self, title: &str, parent_id: Option<&str>) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO pages (id, title, parent_id, sort_order)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages WHERE parent_id IS ?3))",
            params![id, title, parent_id],
        )?;
        Ok(id)
    }

    /// Moves a page under `new_parent` (or to the root) as its last child.
    /// Rejects missing/trashed pages and any move that would create a cycle.
    pub fn move_page(&mut self, id: &str, new_parent: Option<&str>) -> Result<()> {
        let tx = self.conn.transaction()?;
        let live = |page: &str| -> rusqlite::Result<bool> {
            tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM pages WHERE id = ?1 AND deleted_at IS NULL)",
                [page],
                |r| r.get(0),
            )
        };
        ensure!(live(id)?, "La página no existe o está en la papelera");
        if let Some(parent) = new_parent {
            ensure!(live(parent)?, "El destino no existe o está en la papelera");
            ensure!(
                page_kind(&tx, parent)? != Some(PageKind::Database),
                "Una página no se puede mover dentro de una base de datos"
            );
            // Walk up from the destination; UNION deduplicates, so it ends even on bad data.
            let creates_cycle: bool = tx.query_row(
                "WITH RECURSIVE chain(id) AS (
                     SELECT ?1
                     UNION
                     SELECT p.parent_id FROM pages p JOIN chain c ON p.id = c.id
                     WHERE p.parent_id IS NOT NULL
                 )
                 SELECT EXISTS (SELECT 1 FROM chain WHERE id = ?2)",
                params![parent, id],
                |r| r.get(0),
            )?;
            ensure!(
                !creates_cycle,
                "No se puede mover una página dentro de sí misma ni de sus subpáginas"
            );
        }
        tx.execute(
            "UPDATE pages SET parent_id = ?2,
                 sort_order = (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages
                               WHERE parent_id IS ?2 AND id <> ?1),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![id, new_parent],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Moves a page one place up or down among its siblings (same parent).
    /// Returns `false` when it is already first/last. Siblings are renumbered
    /// 0..n in the same transaction, which also repairs duplicate `sort_order`s.
    pub fn reorder_page(&mut self, id: &str, shift: Shift) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let parent: Option<String> = tx
            .query_row(
                "SELECT parent_id FROM pages WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La página no existe o está en la papelera")?;
        // Same order as list_pages, restricted to live siblings.
        let mut siblings: Vec<String> = tx
            .prepare(
                "SELECT id FROM pages WHERE parent_id IS ?1 AND deleted_at IS NULL
                 ORDER BY sort_order, created_at, id",
            )?
            .query_map([&parent], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let index = siblings
            .iter()
            .position(|s| s == id)
            .context("La página no aparece entre sus hermanas")?;
        let target = match shift {
            Shift::Up => index.checked_sub(1),
            Shift::Down => Some(index + 1).filter(|&i| i < siblings.len()),
        };
        let Some(target) = target else {
            return Ok(false);
        };
        siblings.swap(index, target);
        for (position, sibling) in siblings.iter().enumerate() {
            tx.execute(
                "UPDATE pages SET sort_order = ?2 WHERE id = ?1",
                params![sibling, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Renames a live page. The FTS index follows through its trigger.
    pub fn rename_page(&self, id: &str, title: &str) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE pages SET title = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, title],
        )?;
        ensure!(affected == 1, "La página no existe o está en la papelera");
        Ok(())
    }

    /// Creates a database (ADR 0006): a page of kind `database` with its first
    /// view, «Tabla», in one transaction. It goes last among its siblings.
    /// A database cannot be created inside another one (it would be a row).
    pub fn create_database(&mut self, title: &str, parent_id: Option<&str>) -> Result<String> {
        let tx = self.conn.transaction()?;
        if let Some(parent) = parent_id {
            match page_kind(&tx, parent)? {
                None => bail!("El destino no existe o está en la papelera"),
                Some(PageKind::Database) => {
                    bail!("Una base de datos no se puede crear dentro de otra")
                }
                Some(PageKind::Page) => {}
            }
        }
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO pages (id, title, parent_id, kind, sort_order)
             VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages WHERE parent_id IS ?3))",
            params![id, title, parent_id, PageKind::Database.as_str()],
        )?;
        tx.execute(
            "INSERT INTO db_views (id, database_id, name, kind, position) VALUES (?1, ?2, ?3, ?4, 0)",
            params![
                Uuid::new_v4().to_string(),
                id,
                "Tabla",
                ViewKind::Table.as_str()
            ],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Adds a row (a child page) at the end of a live database.
    pub fn create_row(&mut self, database_id: &str, title: &str) -> Result<String> {
        let tx = self.conn.transaction()?;
        ensure!(
            page_kind(&tx, database_id)? == Some(PageKind::Database),
            "La base de datos no existe o está en la papelera"
        );
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO pages (id, title, parent_id, sort_order)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages WHERE parent_id = ?3))",
            params![id, title, database_id],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Live rows of a database, in their manual order (as subpages are).
    pub fn database_rows(&self, database_id: &str) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title, kind FROM pages
             WHERE parent_id = ?1 AND deleted_at IS NULL
             ORDER BY sort_order, created_at, id",
        )?;
        let rows = stmt.query_map([database_id], |r| read_page(r, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Views of a database, in their order. An unknown view kind is skipped
    /// (a newer Silex may add kinds), never an error that hides the data.
    pub fn database_views(&self, database_id: &str) -> Result<Vec<DatabaseView>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, kind FROM db_views WHERE database_id = ?1 ORDER BY position, id",
        )?;
        let rows = stmt.query_map([database_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut views = Vec::new();
        for row in rows {
            let (id, name, kind) = row?;
            if let Some(kind) = ViewKind::parse(&kind) {
                views.push(DatabaseView { id, name, kind });
            }
        }
        Ok(views)
    }

    /// Creates a page with its blocks in one transaction (import): either the
    /// whole page exists afterwards or nothing was created.
    pub fn create_page_with_blocks(
        &mut self,
        title: &str,
        parent_id: Option<&str>,
        blocks: &[NewBlock],
    ) -> Result<String> {
        let tx = self.conn.transaction()?;
        let page_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO pages (id, title, parent_id, sort_order)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages WHERE parent_id IS ?3))",
            params![page_id, title, parent_id],
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO blocks (id, page_id, kind, content, checked, position, indent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (position, block) in blocks.iter().enumerate() {
                insert.execute(params![
                    Uuid::new_v4().to_string(),
                    page_id,
                    block.kind.as_str(),
                    block.content,
                    block.checked && block.kind == BlockKind::Todo,
                    position as i64,
                    block.indent.min(MAX_INDENT),
                ])?;
            }
        }
        tx.commit()?;
        Ok(page_id)
    }

    pub fn get_page(&self, id: &str) -> Result<Option<Page>> {
        self.conn
            .query_row(
                "SELECT id, parent_id, title, kind FROM pages WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |r| read_page(r, 0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_blocks(&self, page_id: &str) -> Result<Vec<Block>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, page_id, kind, content, checked, position, indent
             FROM blocks WHERE page_id = ?1 ORDER BY position, rowid",
        )?;
        let rows = stmt.query_map([page_id], read_block)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_block(&self, page_id: &str, kind: BlockKind) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO blocks (id, page_id, kind, position)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), -1) + 1 FROM blocks WHERE page_id = ?2))",
            params![id, page_id, kind.as_str()],
        )?;
        Ok(id)
    }

    /// Persists the title and the page's whole block list in one transaction:
    /// new blocks are inserted, changed ones updated, missing ones deleted and
    /// `position` set to the list order. Duplicate ids, blocks of another
    /// page or invalid data abort the save and leave the page untouched.
    pub fn save_document(&mut self, page_id: &str, title: &str, blocks: &[Block]) -> Result<()> {
        let mut seen = HashSet::new();
        for block in blocks {
            ensure!(
                seen.insert(block.id.as_str()),
                "Bloque repetido: {}",
                block.id
            );
            ensure!(
                block.page_id == page_id,
                "El bloque {} pertenece a otra página",
                block.id
            );
            ensure!(
                block.indent <= MAX_INDENT,
                "Sangría no válida en el bloque {}",
                block.id
            );
        }

        let tx = self.conn.transaction()?;
        let affected = tx.execute(
            "UPDATE pages SET title = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![page_id, title],
        )?;
        if affected != 1 {
            bail!("La página ya no existe o está en la papelera");
        }
        let stored: HashMap<String, Block> = {
            let mut stmt = tx.prepare(
                "SELECT id, page_id, kind, content, checked, position, indent
                 FROM blocks WHERE page_id = ?1",
            )?;
            let rows = stmt.query_map([page_id], read_block)?;
            rows.map(|b| b.map(|b| (b.id.clone(), b)))
                .collect::<rusqlite::Result<_>>()?
        };
        for id in stored.keys().filter(|id| !seen.contains(id.as_str())) {
            tx.execute("DELETE FROM blocks WHERE id = ?1", [id])?;
        }
        for (index, block) in blocks.iter().enumerate() {
            let position = index as i64;
            match stored.get(&block.id) {
                // Unchanged rows are skipped so FTS is only touched when needed.
                Some(old)
                    if old.kind == block.kind
                        && old.content == block.content
                        && old.checked == block.checked
                        && old.indent == block.indent
                        && old.position == position => {}
                Some(_) => {
                    tx.execute(
                        "UPDATE blocks SET kind = ?2, content = ?3, checked = ?4, position = ?5,
                             indent = ?6, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE id = ?1",
                        params![
                            block.id,
                            block.kind.as_str(),
                            block.content,
                            block.checked,
                            position,
                            block.indent
                        ],
                    )?;
                }
                // An id that exists in another page fails on the primary key.
                None => {
                    tx.execute(
                        "INSERT INTO blocks (id, page_id, kind, content, checked, position, indent)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            block.id,
                            page_id,
                            block.kind.as_str(),
                            block.content,
                            block.checked,
                            position,
                            block.indent
                        ],
                    )
                    .with_context(|| format!("No se pudo guardar el bloque {}", block.id))?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Sends a page and all its live subpages to the trash as one branch.
    /// The branch shares a `deleted_at` that is unique to this operation, so
    /// restoring brings back exactly what was trashed together. Returns the
    /// number of pages trashed (0 if the page was not live).
    pub fn trash_page(&self, id: &str) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        // Strictly later than any existing deleted_at (millisecond resolution),
        // so two trash operations never merge into one branch.
        let stamp: String = tx.query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ', MAX(
                 julianday('now'),
                 COALESCE((SELECT julianday(MAX(deleted_at)) FROM pages), 0) + 1.5 / 86400000.0
             ))",
            [],
            |r| r.get(0),
        )?;
        let trashed = tx.execute(
            "WITH RECURSIVE branch(id) AS (
                 SELECT id FROM pages WHERE id = ?1 AND deleted_at IS NULL
                 UNION
                 SELECT p.id FROM pages p JOIN branch b ON p.parent_id = b.id
                 WHERE p.deleted_at IS NULL
             )
             UPDATE pages SET deleted_at = ?2 WHERE id IN (SELECT id FROM branch)",
            params![id, stamp],
        )?;
        tx.commit()?;
        Ok(trashed)
    }

    /// Trash entries, newest first: each page the user trashed, with the
    /// number of subpages that went with it. Pages trashed as part of a
    /// branch are not listed separately.
    pub fn list_trash(&self) -> Result<Vec<TrashItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title, kind, deleted_at FROM pages
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, title",
        )?;
        let trashed: Vec<(Page, String)> = stmt
            .query_map([], |r| Ok((read_page(r, 0)?, r.get(4)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let stamp_of: HashMap<&str, &str> = trashed
            .iter()
            .map(|(p, at)| (p.id.as_str(), at.as_str()))
            .collect();
        // A page heads its branch unless its parent was trashed in the same operation.
        let heads_branch = |page: &Page, at: &str| {
            page.parent_id
                .as_deref()
                .and_then(|parent| stamp_of.get(parent))
                .is_none_or(|parent_at| *parent_at != at)
        };
        let mut items: Vec<TrashItem> = trashed
            .iter()
            .filter(|(page, at)| heads_branch(page, at))
            .map(|(page, at)| TrashItem {
                page: page.clone(),
                deleted_at: at.clone(),
                subpages: 0,
            })
            .collect();
        for item in &mut items {
            let branch: Vec<Page> = trashed
                .iter()
                .filter(|(_, at)| *at == item.deleted_at)
                .map(|(p, _)| p.clone())
                .collect();
            item.subpages = crate::model::descendants(&branch, &item.page.id).len();
        }
        Ok(items)
    }

    /// Restores a trashed page together with the subpages trashed with it.
    /// If its parent is no longer live, the page comes back at the root.
    pub fn restore_page(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (parent, stamp): (Option<String>, String) = tx
            .query_row(
                "SELECT parent_id, deleted_at FROM pages WHERE id = ?1 AND deleted_at IS NOT NULL",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("La página no está en la papelera")?;
        tx.execute(
            "WITH RECURSIVE branch(id) AS (
                 SELECT ?1
                 UNION
                 SELECT p.id FROM pages p JOIN branch b ON p.parent_id = b.id
                 WHERE p.deleted_at = ?2
             )
             UPDATE pages SET deleted_at = NULL WHERE id IN (SELECT id FROM branch)",
            params![id, stamp],
        )?;
        let parent_is_live = match &parent {
            Some(parent) => tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM pages WHERE id = ?1 AND deleted_at IS NULL)",
                [parent],
                |r| r.get(0),
            )?,
            None => true,
        };
        if !parent_is_live {
            tx.execute(
                "UPDATE pages SET parent_id = NULL,
                     sort_order = (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pages
                                   WHERE parent_id IS NULL AND id <> ?1)
                 WHERE id = ?1",
                [id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn trash_count(&self) -> Result<usize> {
        Ok(self.list_trash()?.len())
    }

    /// Live pages whose title, or one of whose blocks, contains every word of
    /// `term` (whole words, A–Z by title). An empty term lists every page.
    /// Arbitrary input is safe: see [`search::fts_query`].
    pub fn search_pages(&self, term: &str) -> Result<Vec<Page>> {
        if term.trim().is_empty() {
            return self.list_pages();
        }
        let Some(phrase) = search::fts_query(term, false) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT p.id, p.parent_id, p.title, p.kind FROM pages p
             WHERE p.deleted_at IS NULL AND (
                 p.rowid IN (SELECT rowid FROM page_search WHERE page_search MATCH ?1)
                 OR p.id IN (
                     SELECT b.page_id FROM blocks b
                     WHERE b.rowid IN (SELECT rowid FROM block_search WHERE block_search MATCH ?1)
                 )
             ) ORDER BY p.title",
        )?;
        let rows = stmt.query_map([phrase], |r| read_page(r, 0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Global search (H5.7): live pages whose title or one of whose blocks
    /// contains every word, also as the start of a longer word, ignoring
    /// case and accents. Ranked by [`search::rank`]: title matches first,
    /// then by FTS5 `bm25`. At most [`search::MAX_RESULTS`]; each result
    /// with a content match carries a snippet of its best block.
    pub fn search(&self, input: &str) -> Result<Vec<SearchHit>> {
        let Some(query) = search::fts_query(input, true) else {
            return Ok(Vec::new());
        };
        let start = search::MATCH_START.to_string();
        let end = search::MATCH_END.to_string();
        let mut found: HashMap<String, Candidate> = HashMap::new();
        // One read transaction: the three queries see the same data, even if
        // another connection writes in between. Dropping it ends it.
        let tx = self.conn.unchecked_transaction()?;

        let mut titles = tx.prepare(
            "SELECT p.id, p.parent_id, p.title, p.kind, bm25(page_search), highlight(page_search, 0, ?2, ?3)
             FROM page_search JOIN pages p ON p.rowid = page_search.rowid
             WHERE page_search MATCH ?1 AND p.deleted_at IS NULL",
        )?;
        let rows = titles.query_map(params![query, start, end], |r| {
            Ok(Candidate {
                page: read_page(r, 0)?,
                title: Some((r.get(4)?, r.get(5)?)),
                block: None,
            })
        })?;
        for candidate in rows {
            let candidate = candidate?;
            found.insert(candidate.page.id.clone(), candidate);
        }

        // Best block first; on a tie, the one higher up in the page.
        let mut blocks = tx.prepare(
            "SELECT p.id, p.parent_id, p.title, p.kind, b.id, b.rowid, bm25(block_search) AS score
             FROM block_search JOIN blocks b ON b.rowid = block_search.rowid
             JOIN pages p ON p.id = b.page_id
             WHERE block_search MATCH ?1 AND p.deleted_at IS NULL
             ORDER BY score, b.position, b.rowid",
        )?;
        let rows = blocks.query_map([&query], |r| {
            let page = read_page(r, 0)?;
            let block: (String, i64, f64) = (r.get(4)?, r.get(5)?, r.get(6)?);
            Ok((page, block))
        })?;
        for row in rows {
            let (page, block) = row?;
            let candidate = found.entry(page.id.clone()).or_insert(Candidate {
                page,
                title: None,
                block: None,
            });
            if candidate.block.is_none() {
                candidate.block = Some(block);
            }
        }

        let mut ranked: Vec<Candidate> = found.into_values().collect();
        search::rank(&mut ranked);
        ranked.truncate(search::MAX_RESULTS);

        // Snippets only for the results that are returned.
        let mut snippet = tx.prepare(
            "SELECT snippet(block_search, 0, ?2, ?3, '…', ?4), highlight(block_search, 0, ?2, ?3)
             FROM block_search WHERE block_search MATCH ?1 AND rowid = ?5",
        )?;
        let mut hits = Vec::with_capacity(ranked.len());
        for candidate in ranked {
            let title = match &candidate.title {
                Some((_, marked)) => search::parse_marked(marked),
                None => search::parse_marked(&candidate.page.title),
            };
            let block = match candidate.block {
                Some((block_id, rowid, _)) => snippet
                    .query_row(
                        params![query, start, end, search::SNIPPET_TOKENS, rowid],
                        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                    )
                    .optional()?
                    .map(|(cut, full)| BlockMatch {
                        block_id,
                        snippet: search::parse_marked(&cut),
                        char_offset: search::first_match_offset(&full),
                    }),
                None => None,
            };
            hits.push(SearchHit {
                page: candidate.page,
                title,
                block,
            });
        }
        Ok(hits)
    }
}

/// Kind of a live page; `None` if it is missing or in the trash.
fn page_kind(conn: &Connection, id: &str) -> Result<Option<PageKind>> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM pages WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    match kind {
        None => Ok(None),
        Some(kind) => PageKind::parse(&kind)
            .map(Some)
            .with_context(|| format!("tipo de página desconocido: {kind}")),
    }
}

/// Maps the page columns `id, parent_id, title, kind` starting at column
/// `at`. An unknown kind is a conversion error, not a panic.
fn read_page(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Page> {
    let kind: String = r.get(at + 3)?;
    let kind = PageKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            at + 3,
            rusqlite::types::Type::Text,
            format!("tipo de página desconocido: {kind}").into(),
        )
    })?;
    Ok(Page {
        id: r.get(at)?,
        parent_id: r.get(at + 1)?,
        title: r.get(at + 2)?,
        kind,
    })
}

/// Maps a `blocks` row (id, page_id, kind, content, checked, position, indent).
/// An unknown kind is a conversion error, not a panic.
fn read_block(r: &rusqlite::Row<'_>) -> rusqlite::Result<Block> {
    let kind: String = r.get(2)?;
    let kind = BlockKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("tipo de bloque desconocido: {kind}").into(),
        )
    })?;
    Ok(Block {
        id: r.get(0)?,
        page_id: r.get(1)?,
        kind,
        content: r.get(3)?,
        checked: r.get(4)?,
        position: r.get(5)?,
        indent: r.get(6)?,
    })
}
