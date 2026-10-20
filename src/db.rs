//! All SQL lives here. Callers never construct SQL from user input.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use directories::ProjectDirs;
use rusqlite::{Connection, MAIN_DB, OptionalExtension, params, types::Value};
use uuid::Uuid;

use crate::attachment::{Attachment, MAX_SIZE, NewAttachment};
use crate::model::{
    Block, BlockKind, DatabaseView, MAX_INDENT, NewBlock, Page, PageKind, TrashItem, ViewKind,
};
use crate::property::{CellValue, OptionColor, Property, PropertyKind, SelectOption, TableValues};
use crate::search::{self, BlockMatch, Candidate, SearchHit};
use crate::settings::Settings;
use crate::versions::{self, VersionBlocks, VersionInfo};
use crate::view::ViewConfig;

pub mod backup;

/// Published migrations, in order. Never edit one that has shipped: add a new file.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("db/migrations/0001_init.sql")),
    (2, include_str!("db/migrations/0002_favorites.sql")),
    (3, include_str!("db/migrations/0003_block_kinds.sql")),
    (4, include_str!("db/migrations/0004_settings.sql")),
    (5, include_str!("db/migrations/0005_databases.sql")),
    (6, include_str!("db/migrations/0006_attachments.sql")),
    (7, include_str!("db/migrations/0007_page_links.sql")),
    (8, include_str!("db/migrations/0008_page_versions.sql")),
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
    /// A fixed time for page versions (tests only); `None` is SQLite's
    /// clock. See [`Database::set_clock`].
    clock: Option<String>,
}

/// SQLite's UTC time, as stored in `created_at` columns.
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

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
        let mut db = Self { conn, clock: None };
        db.configure()?;
        db.migrate(PreMigrationBackup::Yes)?;
        db.finish_compaction()?;
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
        let mut db = Self { conn, clock: None };
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
    /// Renames a live page, and the links that point at it (ADR 0008:
    /// their text is its title, for search and export), in one transaction.
    pub fn rename_page(&self, id: &str, title: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let affected = tx.execute(
            "UPDATE pages SET title = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, title],
        )?;
        ensure!(affected == 1, "La página no existe o está en la papelera");
        retitle_links(&tx, id, title)?;
        tx.commit()?;
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
            "SELECT id, name, kind, config FROM db_views WHERE database_id = ?1 ORDER BY position, id",
        )?;
        let rows = stmt.query_map([database_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut views = Vec::new();
        for row in rows {
            let (id, name, kind, config) = row?;
            if let Some(kind) = ViewKind::parse(&kind) {
                // A damaged config reads as the default (ADR 0006).
                let config = ViewConfig::from_json(&config);
                views.push(DatabaseView {
                    id,
                    name,
                    kind,
                    config,
                });
            }
        }
        Ok(views)
    }

    /// Adds a view of `kind` (a table or a board, H6.5) at the end of a
    /// live database's tabs, with `config` (empty for a new view, another
    /// view's for a copy).
    pub fn add_view(
        &mut self,
        database_id: &str,
        name: &str,
        kind: ViewKind,
        config: &ViewConfig,
    ) -> Result<String> {
        let tx = self.conn.transaction()?;
        ensure!(
            page_kind(&tx, database_id)? == Some(PageKind::Database),
            "La base de datos no existe o está en la papelera"
        );
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO db_views (id, database_id, name, kind, position, config)
             VALUES (?1, ?2, ?3, ?4,
                     (SELECT COALESCE(MAX(position), -1) + 1 FROM db_views WHERE database_id = ?2), ?5)",
            params![id, database_id, name, kind.as_str(), config.to_json()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn rename_view(&self, id: &str, name: &str) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_views SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        ensure!(affected == 1, "La vista no existe");
        Ok(())
    }

    /// Moves a view one tab left (`Up`) or right (`Down`). Returns `false`
    /// at the edge. The database's views are renumbered 0..n.
    pub fn move_view(&mut self, id: &str, shift: Shift) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let database: String = tx
            .query_row(
                "SELECT database_id FROM db_views WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La vista no existe")?;
        let mut tabs: Vec<String> = tx
            .prepare("SELECT id FROM db_views WHERE database_id = ?1 ORDER BY position, id")?
            .query_map([&database], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let index = tabs
            .iter()
            .position(|t| t == id)
            .context("La vista no aparece en su base de datos")?;
        let target = match shift {
            Shift::Up => index.checked_sub(1),
            Shift::Down => Some(index + 1).filter(|&i| i < tabs.len()),
        };
        let Some(target) = target else {
            return Ok(false);
        };
        tabs.swap(index, target);
        for (position, tab) in tabs.iter().enumerate() {
            tx.execute(
                "UPDATE db_views SET position = ?2 WHERE id = ?1",
                params![tab, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Deletes a view: only its settings (sort, filters, columns), never
    /// rows or values. A database always keeps at least one view.
    pub fn delete_view(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let database: String = tx
            .query_row(
                "SELECT database_id FROM db_views WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La vista no existe")?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM db_views WHERE database_id = ?1",
            [&database],
            |r| r.get(0),
        )?;
        ensure!(
            count > 1,
            "Es la última vista de la base de datos: no se puede borrar"
        );
        tx.execute("DELETE FROM db_views WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Stores a view's sort, filters and hidden properties.
    pub fn set_view_config(&self, view_id: &str, config: &ViewConfig) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_views SET config = ?2 WHERE id = ?1",
            params![view_id, config.to_json()],
        )?;
        ensure!(affected == 1, "La vista no existe");
        Ok(())
    }

    /// Live properties of a database in column order, each with its select
    /// options. «Nombre» is the row's title, not a property.
    pub fn database_properties(&self, database_id: &str) -> Result<Vec<Property>> {
        read_properties(&self.conn, database_id, Deleted::No)
    }

    /// Deleted properties of a database, most recently deleted first. They
    /// keep their values and options and can be restored.
    pub fn deleted_properties(&self, database_id: &str) -> Result<Vec<Property>> {
        read_properties(&self.conn, database_id, Deleted::Yes)
    }

    /// Adds a property as the last column of a live database.
    pub fn add_property(
        &mut self,
        database_id: &str,
        name: &str,
        kind: PropertyKind,
    ) -> Result<String> {
        let tx = self.conn.transaction()?;
        ensure!(
            page_kind(&tx, database_id)? == Some(PageKind::Database),
            "La base de datos no existe o está en la papelera"
        );
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO db_properties (id, database_id, name, kind, position)
             VALUES (?1, ?2, ?3, ?4,
                     (SELECT COALESCE(MAX(position), -1) + 1 FROM db_properties WHERE database_id = ?2))",
            params![id, database_id, name, kind.as_str()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn rename_property(&self, id: &str, name: &str) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_properties SET name = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, name],
        )?;
        ensure!(affected == 1, "La propiedad no existe o está borrada");
        Ok(())
    }

    /// Moves a live property one column left (`Up`) or right (`Down`).
    /// Returns `false` at the edge. Live properties are renumbered 0..n.
    pub fn move_property(&mut self, id: &str, shift: Shift) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let database: String = tx
            .query_row(
                "SELECT database_id FROM db_properties WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La propiedad no existe o está borrada")?;
        let mut columns: Vec<String> = tx
            .prepare(
                "SELECT id FROM db_properties WHERE database_id = ?1 AND deleted_at IS NULL
                 ORDER BY position, id",
            )?
            .query_map([&database], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let index = columns
            .iter()
            .position(|c| c == id)
            .context("La propiedad no aparece en su base de datos")?;
        let target = match shift {
            Shift::Up => index.checked_sub(1),
            Shift::Down => Some(index + 1).filter(|&i| i < columns.len()),
        };
        let Some(target) = target else {
            return Ok(false);
        };
        columns.swap(index, target);
        for (position, column) in columns.iter().enumerate() {
            tx.execute(
                "UPDATE db_properties SET position = ?2 WHERE id = ?1",
                params![column, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Deletes a property logically (ADR 0006): it leaves the table, and its
    /// values and options stay for [`Database::restore_property`].
    pub fn delete_property(&self, id: &str) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_properties SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            [id],
        )?;
        ensure!(affected == 1, "La propiedad no existe o ya está borrada");
        Ok(())
    }

    /// Brings a deleted property back, with its values, as the last column.
    pub fn restore_property(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let database: String = tx
            .query_row(
                "SELECT database_id FROM db_properties WHERE id = ?1 AND deleted_at IS NOT NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La propiedad no está borrada")?;
        tx.execute(
            "UPDATE db_properties SET deleted_at = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 position = (SELECT COALESCE(MAX(position), -1) + 1 FROM db_properties
                             WHERE database_id = ?2 AND deleted_at IS NULL)
             WHERE id = ?1",
            params![id, database],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Adds an option at the end of a live select property.
    pub fn add_select_option(
        &mut self,
        property_id: &str,
        name: &str,
        color: OptionColor,
    ) -> Result<String> {
        let tx = self.conn.transaction()?;
        let kind: String = tx
            .query_row(
                "SELECT kind FROM db_properties WHERE id = ?1 AND deleted_at IS NULL",
                [property_id],
                |r| r.get(0),
            )
            .optional()?
            .context("La propiedad no existe o está borrada")?;
        ensure!(
            PropertyKind::parse(&kind) == Some(PropertyKind::Select),
            "Solo las propiedades de selección tienen opciones"
        );
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO db_select_options (id, property_id, name, color, position)
             VALUES (?1, ?2, ?3, ?4,
                     (SELECT COALESCE(MAX(position), -1) + 1 FROM db_select_options WHERE property_id = ?2))",
            params![id, property_id, name, color.as_str()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// The values of a database's live rows for its live properties, in one
    /// query. A value that does not fit its property (another column, a
    /// missing option) reads as empty: see [`CellValue::from_columns`].
    pub fn database_values(&self, database_id: &str) -> Result<TableValues> {
        let mut stmt = self.conn.prepare(
            "SELECT v.page_id, v.property_id, p.kind, v.text_value, v.number_value,
                    v.bool_value, v.date_value, o.id
             FROM db_properties p
             JOIN db_values v ON v.property_id = p.id
             JOIN pages r ON r.id = v.page_id
             LEFT JOIN db_select_options o ON o.id = v.option_id AND o.property_id = p.id
             WHERE p.database_id = ?1 AND p.deleted_at IS NULL
               AND r.parent_id = ?1 AND r.deleted_at IS NULL",
        )?;
        let mut rows = stmt.query([database_id])?;
        let mut values = TableValues::new();
        while let Some(r) = rows.next()? {
            if let Some(value) = read_cell(r, 2)? {
                values
                    .entry(r.get(0)?)
                    .or_default()
                    .insert(r.get(1)?, value);
            }
        }
        Ok(values)
    }

    /// The values of one live row (property id → value) for the live
    /// properties of its database: what its page shows above its blocks.
    /// Empty for a page that is not a row.
    pub fn row_values(&self, row_id: &str) -> Result<HashMap<String, CellValue>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.property_id, p.kind, v.text_value, v.number_value,
                    v.bool_value, v.date_value, o.id
             FROM pages r
             JOIN db_properties p ON p.database_id = r.parent_id AND p.deleted_at IS NULL
             JOIN db_values v ON v.page_id = r.id AND v.property_id = p.id
             LEFT JOIN db_select_options o ON o.id = v.option_id AND o.property_id = p.id
             WHERE r.id = ?1 AND r.deleted_at IS NULL",
        )?;
        let mut rows = stmt.query([row_id])?;
        let mut values = HashMap::new();
        while let Some(r) = rows.next()? {
            if let Some(value) = read_cell(r, 1)? {
                values.insert(r.get(0)?, value);
            }
        }
        Ok(values)
    }

    pub fn rename_select_option(&self, id: &str, name: &str) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_select_options SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        ensure!(affected == 1, "La opción no existe");
        Ok(())
    }

    pub fn set_option_color(&self, id: &str, color: OptionColor) -> Result<()> {
        let affected = self.conn.execute(
            "UPDATE db_select_options SET color = ?2 WHERE id = ?1",
            params![id, color.as_str()],
        )?;
        ensure!(affected == 1, "La opción no existe");
        Ok(())
    }

    /// Moves an option one place earlier (`Up`) or later (`Down`) among its
    /// property's options: the order of select menus and of a board's
    /// columns (H6.5). Returns `false` at the edge. The options are
    /// renumbered 0..n.
    pub fn move_select_option(&mut self, id: &str, shift: Shift) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let property: String = tx
            .query_row(
                "SELECT property_id FROM db_select_options WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .context("La opción no existe")?;
        let mut options: Vec<String> = tx
            .prepare(
                "SELECT id FROM db_select_options WHERE property_id = ?1 ORDER BY position, id",
            )?
            .query_map([&property], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let index = options
            .iter()
            .position(|o| o == id)
            .context("La opción no aparece en su propiedad")?;
        let target = match shift {
            Shift::Up => index.checked_sub(1),
            Shift::Down => Some(index + 1).filter(|&i| i < options.len()),
        };
        let Some(target) = target else {
            return Ok(false);
        };
        options.swap(index, target);
        for (position, option) in options.iter().enumerate() {
            tx.execute(
                "UPDATE db_select_options SET position = ?2 WHERE id = ?1",
                params![option, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Deletes a select option. The cells that had it become empty
    /// (`ON DELETE SET NULL`, ADR 0006); the UI asks first.
    pub fn delete_select_option(&self, id: &str) -> Result<()> {
        let affected = self
            .conn
            .execute("DELETE FROM db_select_options WHERE id = ?1", [id])?;
        ensure!(affected == 1, "La opción no existe");
        Ok(())
    }

    /// Sets the value of one cell; `None` or an empty value (empty text, an
    /// unticked box) clears it. The row must be a live row of the property's
    /// database, the property live, and the value of its kind (a finite
    /// number, an option of that property). Rejected values change nothing.
    pub fn set_value(
        &mut self,
        row_id: &str,
        property_id: &str,
        value: Option<&CellValue>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (database, kind): (String, String) = tx
            .query_row(
                "SELECT database_id, kind FROM db_properties WHERE id = ?1 AND deleted_at IS NULL",
                [property_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("La propiedad no existe o está borrada")?;
        let kind = PropertyKind::parse(&kind)
            .with_context(|| format!("tipo de propiedad desconocido: {kind}"))?;
        let is_row: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM pages WHERE id = ?1 AND parent_id = ?2 AND deleted_at IS NULL)",
            params![row_id, database],
            |r| r.get(0),
        )?;
        ensure!(
            is_row,
            "La fila no existe, está en la papelera o es de otra base de datos"
        );
        if let Some(value) = value {
            ensure!(
                value.kind() == kind,
                "Un valor de tipo {} no cabe en una propiedad de tipo {}",
                value.kind().label(),
                kind.label()
            );
        }
        match value.filter(|v| !v.is_empty()) {
            None => {
                tx.execute(
                    "DELETE FROM db_values WHERE page_id = ?1 AND property_id = ?2",
                    params![row_id, property_id],
                )?;
            }
            Some(value) => {
                let (mut text, mut number, mut flag, mut date, mut option) =
                    (None, None, None, None, None);
                match value {
                    CellValue::Text(t) => text = Some(t.as_str()),
                    CellValue::Number(n) => {
                        ensure!(n.is_finite(), "El número no es válido");
                        number = Some(*n);
                    }
                    CellValue::Checkbox(on) => flag = Some(i64::from(*on)),
                    CellValue::Date(d) => date = Some(d.to_string()),
                    CellValue::Select(id) => {
                        let belongs: bool = tx.query_row(
                            "SELECT EXISTS (SELECT 1 FROM db_select_options WHERE id = ?1 AND property_id = ?2)",
                            params![id, property_id],
                            |r| r.get(0),
                        )?;
                        ensure!(belongs, "La opción no es de esta propiedad");
                        option = Some(id.as_str());
                    }
                }
                tx.execute(
                    "INSERT INTO db_values
                         (page_id, property_id, text_value, number_value, bool_value, date_value, option_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (page_id, property_id) DO UPDATE SET
                         text_value = excluded.text_value,
                         number_value = excluded.number_value,
                         bool_value = excluded.bool_value,
                         date_value = excluded.date_value,
                         option_id = excluded.option_id,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                    params![row_id, property_id, text, number, flag, date, option],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Creates a page with its blocks in one transaction (import): either the
    /// whole page exists afterwards or nothing was created.
    pub fn create_page_with_blocks(
        &mut self,
        title: &str,
        parent_id: Option<&str>,
        blocks: &[NewBlock],
    ) -> Result<String> {
        ensure!(
            blocks.iter().all(|b| b.kind.is_text()),
            "Una página nueva no puede crearse con archivos adjuntos"
        );
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
            "SELECT id, page_id, kind, content, checked, position, indent, attachment_id,
                    link_page_id
             FROM blocks WHERE page_id = ?1 ORDER BY position, rowid",
        )?;
        let rows = stmt.query_map([page_id], read_block)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_block(&self, page_id: &str, kind: BlockKind) -> Result<String> {
        ensure!(kind.is_text(), "Un archivo se añade con su adjunto");
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
            ensure!(
                (block.kind == BlockKind::File) == block.attachment.is_some(),
                "El bloque {} no encaja con su adjunto",
                block.id
            );
            // A link may have lost its page (`ON DELETE SET NULL`), but only
            // a link points at one.
            ensure!(
                block.kind == BlockKind::PageLink || block.link.is_none(),
                "El bloque {} no es un enlace",
                block.id
            );
        }

        let tx = self.conn.transaction()?;
        let old_title: Option<String> = tx
            .query_row(
                "SELECT title FROM pages WHERE id = ?1 AND deleted_at IS NULL",
                [page_id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(old_title) = old_title else {
            bail!("La página ya no existe o está en la papelera");
        };
        let mut changed = old_title != title;
        tx.execute(
            "UPDATE pages SET title = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![page_id, title],
        )?;
        retitle_links(&tx, page_id, title)?;
        let stored: HashMap<String, Block> = {
            let mut stmt = tx.prepare(
                "SELECT id, page_id, kind, content, checked, position, indent, attachment_id,
                        link_page_id
                 FROM blocks WHERE page_id = ?1",
            )?;
            let rows = stmt.query_map([page_id], read_block)?;
            rows.map(|b| b.map(|b| (b.id.clone(), b)))
                .collect::<rusqlite::Result<_>>()?
        };
        for id in stored.keys().filter(|id| !seen.contains(id.as_str())) {
            tx.execute("DELETE FROM blocks WHERE id = ?1", [id])?;
            changed = true;
        }
        for (index, block) in blocks.iter().enumerate() {
            let position = index as i64;
            // A link's text is its page's current title (ADR 0008), whatever
            // the open document still holds.
            let linked;
            let block = match &block.link {
                Some(target) => {
                    let title: Option<String> = tx
                        .query_row("SELECT title FROM pages WHERE id = ?1", [target], |r| {
                            r.get(0)
                        })
                        .optional()?;
                    linked = Block {
                        content: title.unwrap_or_else(|| block.content.clone()),
                        ..block.clone()
                    };
                    &linked
                }
                None => block,
            };
            match stored.get(&block.id) {
                // Unchanged rows are skipped so FTS is only touched when needed.
                Some(old)
                    if old.kind == block.kind
                        && old.content == block.content
                        && old.checked == block.checked
                        && old.indent == block.indent
                        && old.attachment == block.attachment
                        && old.link == block.link
                        && old.position == position => {}
                Some(_) => {
                    changed = true;
                    tx.execute(
                        "UPDATE blocks SET kind = ?2, content = ?3, checked = ?4, position = ?5,
                             indent = ?6, attachment_id = ?7, link_page_id = ?8,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                         WHERE id = ?1",
                        params![
                            block.id,
                            block.kind.as_str(),
                            block.content,
                            block.checked,
                            position,
                            block.indent,
                            block.attachment,
                            block.link
                        ],
                    )
                    .with_context(|| format!("No se pudo guardar el bloque {}", block.id))?;
                }
                // An id that exists in another page fails on the primary key.
                None => {
                    changed = true;
                    tx.execute(
                        "INSERT INTO blocks (id, page_id, kind, content, checked, position, indent,
                                             attachment_id, link_page_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            block.id,
                            page_id,
                            block.kind.as_str(),
                            block.content,
                            block.checked,
                            position,
                            block.indent,
                            block.attachment,
                            block.link
                        ],
                    )
                    .with_context(|| format!("No se pudo guardar el bloque {}", block.id))?;
                }
            }
        }
        // The page as it was before this save, if its last version is old
        // enough (ADR 0009).
        if changed {
            let mut before: Vec<Block> = stored.into_values().collect();
            before.sort_by(|a, b| (a.position, &a.id).cmp(&(b.position, &b.id)));
            save_version(
                &tx,
                self.clock.as_deref(),
                page_id,
                &old_title,
                &before,
                false,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Saves the page as it is stored now as a version, however recent the
    /// last one is (before restoring a version, ADR 0009). Returns false if
    /// it is the same as the last version, or the page has no blocks.
    pub fn save_version(&mut self, page_id: &str) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let title: Option<String> = tx
            .query_row("SELECT title FROM pages WHERE id = ?1", [page_id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(title) = title else {
            bail!("La página no existe");
        };
        let blocks = {
            let mut stmt = tx.prepare(
                "SELECT id, page_id, kind, content, checked, position, indent, attachment_id,
                        link_page_id
                 FROM blocks WHERE page_id = ?1 ORDER BY position, id",
            )?;
            let rows = stmt.query_map([page_id], read_block)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let saved = save_version(&tx, self.clock.as_deref(), page_id, &title, &blocks, true)?;
        tx.commit()?;
        Ok(saved)
    }

    /// The versions of a page, newest first, without their blocks.
    pub fn page_versions(&self, page_id: &str) -> Result<Vec<VersionInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, page_id, created_at, title, size,
                    strftime('%Y-%m-%d %H:%M', created_at, 'localtime')
             FROM page_versions
             WHERE page_id = ?1 ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([page_id], |r| {
            Ok(VersionInfo {
                id: r.get(0)?,
                page_id: r.get(1)?,
                created_at: r.get(2)?,
                title: r.get(3)?,
                size: r.get::<_, i64>(4)?.try_into().unwrap_or_default(),
                // Without a time zone (or a bad date), UTC.
                local: r
                    .get::<_, Option<String>>(5)?
                    .unwrap_or_else(|| r.get::<_, String>(2).unwrap_or_default()),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// A version's blocks, or why they cannot be read. `None` if there is
    /// no such version.
    pub fn version_blocks(&self, id: i64) -> Result<Option<VersionBlocks>> {
        let row: Option<(String, i64, String)> = self
            .conn
            .query_row(
                "SELECT page_id, format, blocks FROM page_versions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        Ok(row.map(|(page, format, json)| versions::blocks_from_json(format, &json, &page)))
    }

    /// Today in the system's time zone, `YYYY-MM-DD` (to say «Hoy» and
    /// «Ayer» in the history). SQLite asks the system for the zone.
    pub fn local_today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?)
    }

    /// A version's blocks made safe to save into `page_id` (ADR 0009): a
    /// file that is no longer there becomes a paragraph that says so, a
    /// link to a page that no longer exists loses its page, and a block id
    /// now used by another page gets a new id.
    pub fn restorable_blocks(&self, page_id: &str, blocks: Vec<Block>) -> Result<Vec<Block>> {
        let mut restored = Vec::with_capacity(blocks.len());
        for mut block in blocks {
            block.page_id = page_id.to_owned();
            if let Some(file) = &block.attachment {
                let exists: bool = self.conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM attachments WHERE id = ?1)",
                    [file],
                    |r| r.get(0),
                )?;
                if !exists {
                    block.kind = BlockKind::Paragraph;
                    block.content = format!("[archivo que ya no está] {}", block.content);
                    block.attachment = None;
                }
            }
            if let Some(page) = &block.link {
                let exists: bool = self.conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM pages WHERE id = ?1)",
                    [page],
                    |r| r.get(0),
                )?;
                if !exists {
                    block.link = None;
                }
            }
            let elsewhere: bool = self.conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM blocks WHERE id = ?1 AND page_id <> ?2)",
                params![block.id, page_id],
                |r| r.get(0),
            )?;
            if elsewhere {
                block.id = Uuid::new_v4().to_string();
            }
            restored.push(block);
        }
        Ok(restored)
    }

    /// Fixes the time page versions are saved at (`YYYY-MM-DDTHH:MM:SS.sssZ`,
    /// UTC), or goes back to the real clock with `None`. For tests: the
    /// 10-minute rule and the thinning depend on it.
    #[doc(hidden)]
    pub fn set_clock(&mut self, now: Option<&str>) {
        self.clock = now.map(str::to_owned);
    }

    /// Stores a file (ADR 0007) and returns it as listed. Nothing uses it
    /// until a block points at it (`save_document`). Called from the
    /// attach worker's own connection: 20 MB take ~0,1 s.
    pub fn add_attachment(&mut self, file: &NewAttachment) -> Result<Attachment> {
        let size = file.data.len() as u64;
        ensure!(
            size <= MAX_SIZE,
            "El archivo supera el límite de {}",
            crate::attachment::format_size(MAX_SIZE)
        );
        let id = Uuid::new_v4().to_string();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO attachments (id, name, mime, size, width, height, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                file.name,
                file.mime,
                size as i64,
                file.width,
                file.height,
                file.data
            ],
        )?;
        tx.commit()?;
        Ok(Attachment {
            id,
            name: file.name.clone(),
            mime: file.mime.clone(),
            size,
            width: file.width,
            height: file.height,
        })
    }

    /// What the database and its attachments take (H7.4), without reading
    /// any attachment's data. «Used» means some block points at it, on a
    /// live page or one in the trash.
    pub fn space_usage(&self) -> Result<SpaceUsage> {
        let (used, used_bytes, unused, unused_bytes): (i64, i64, i64, i64) = self.conn.query_row(
            "WITH in_versions AS (
                 SELECT json_extract(j.value, '$.attachment')
                 FROM page_versions v, json_each(v.blocks) j
                 WHERE json_extract(j.value, '$.attachment') IS NOT NULL)
             SELECT
                     COALESCE(SUM(used), 0), COALESCE(SUM(CASE WHEN used THEN size END), 0),
                     COALESCE(SUM(NOT used), 0), COALESCE(SUM(CASE WHEN NOT used THEN size END), 0)
                 FROM (SELECT a.size,
                              EXISTS (SELECT 1 FROM blocks b WHERE b.attachment_id = a.id)
                              OR a.id IN in_versions AS used
                       FROM attachments a)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let page_size: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let pages: i64 = self
            .conn
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free: i64 = self
            .conn
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let bytes = |n: i64| u64::try_from(n).unwrap_or_default();
        Ok(SpaceUsage {
            database: bytes(page_size * pages),
            free: bytes(page_size * free),
            used_attachments: bytes(used) as usize,
            used_bytes: bytes(used_bytes),
            unused_attachments: bytes(unused) as usize,
            unused_bytes: bytes(unused_bytes),
        })
    }

    /// Deletes the attachments no block uses (H7.4), except those in
    /// `keep` (the open page's undo history can still bring their blocks
    /// back). Returns how many went and their size. They stay in older
    /// backups. The space is reused, but the file only shrinks with
    /// [`Database::compact`].
    pub fn delete_unused_attachments(&mut self, keep: &HashSet<String>) -> Result<(usize, u64)> {
        let tx = self.conn.transaction()?;
        let unused: Vec<(String, i64)> = tx
            .prepare(
                "WITH in_versions AS (
                 SELECT json_extract(j.value, '$.attachment')
                 FROM page_versions v, json_each(v.blocks) j
                 WHERE json_extract(j.value, '$.attachment') IS NOT NULL)
                 SELECT id, size FROM attachments a
                 WHERE NOT EXISTS (SELECT 1 FROM blocks b WHERE b.attachment_id = a.id)
                   AND a.id NOT IN in_versions",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut count = 0;
        let mut bytes = 0u64;
        for (id, size) in unused.iter().filter(|(id, _)| !keep.contains(id)) {
            tx.execute("DELETE FROM attachments WHERE id = ?1", [id])?;
            count += 1;
            bytes += u64::try_from(*size).unwrap_or_default();
        }
        tx.commit()?;
        Ok((count, bytes))
    }

    /// Gives free space back to the system (`VACUUM`, H7.4). `VACUUM` may
    /// renumber the implicit `rowid`s the search indexes point at, so both
    /// are rebuilt after it. A mark is stored first: if the app stops in
    /// between, the next open finishes the rebuild
    /// ([`Database::finish_compaction`]).
    pub fn compact(&mut self) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, '1')
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [COMPACTING],
        )?;
        self.conn
            .execute_batch("VACUUM")
            .context("No se pudo compactar la base")?;
        self.finish_compaction()
    }

    /// Rebuilds the search indexes if a compaction was interrupted, in one
    /// transaction with clearing its mark. Cheap when there is no mark.
    pub fn finish_compaction(&mut self) -> Result<()> {
        let pending: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM settings WHERE key = ?1)",
            [COMPACTING],
            |r| r.get(0),
        )?;
        if !pending {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "INSERT INTO page_search(page_search) VALUES ('rebuild');
             INSERT INTO block_search(block_search) VALUES ('rebuild');",
        )?;
        tx.execute("DELETE FROM settings WHERE key = ?1", [COMPACTING])?;
        tx.commit()?;
        Ok(())
    }

    /// The pages the links of `page_id` point at, live or in the trash
    /// (ADR 0008).
    pub fn page_link_targets(&self, page_id: &str) -> Result<Vec<LinkTarget>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT p.id, p.title, p.kind, p.deleted_at IS NOT NULL
             FROM blocks b JOIN pages p ON p.id = b.link_page_id
             WHERE b.page_id = ?1",
        )?;
        let rows = stmt.query_map([page_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?;
        let mut targets = Vec::new();
        for row in rows {
            let (id, title, kind, trashed) = row?;
            targets.push(LinkTarget {
                id,
                title,
                // An unknown kind (a newer Silex) shows as a page.
                kind: PageKind::parse(&kind).unwrap_or(PageKind::Page),
                trashed,
            });
        }
        Ok(targets)
    }

    /// The live pages with a link to `page_id`, without itself (ADR 0008,
    /// «Enlazada desde»). Pages in the trash are left out. Uses the
    /// partial index on `link_page_id`.
    pub fn backlinks(&self, page_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT p.id
             FROM blocks b JOIN pages p ON p.id = b.page_id
             WHERE b.link_page_id = ?1 AND p.id <> ?1 AND p.deleted_at IS NULL
             ORDER BY p.title, p.id",
        )?;
        let ids = stmt
            .query_map([page_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    /// An attachment's details, without its data.
    pub fn attachment(&self, id: &str) -> Result<Option<Attachment>> {
        self.conn
            .query_row(
                "SELECT id, name, mime, size, width, height FROM attachments WHERE id = ?1",
                [id],
                read_attachment,
            )
            .optional()
            .map_err(Into::into)
    }

    /// The attachments used by a page's blocks, without their data.
    pub fn page_attachments(&self, page_id: &str) -> Result<Vec<Attachment>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT a.id, a.name, a.mime, a.size, a.width, a.height
             FROM blocks b JOIN attachments a ON a.id = b.attachment_id
             WHERE b.page_id = ?1",
        )?;
        let rows = stmt.query_map([page_id], read_attachment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The content of an attachment (to open, export or preview it).
    pub fn attachment_data(&self, id: &str) -> Result<Vec<u8>> {
        self.conn
            .query_row("SELECT data FROM attachments WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .context("El archivo adjunto ya no existe")
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

/// Which properties [`read_properties`] lists.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Deleted {
    No,
    Yes,
}

/// Properties of a database with their select options: the live ones in
/// column order, or the deleted ones newest first. A property of an unknown
/// kind (a newer Silex) is skipped, never an error that hides the table.
fn read_properties(
    conn: &Connection,
    database_id: &str,
    deleted: Deleted,
) -> Result<Vec<Property>> {
    let sql = match deleted {
        Deleted::No => {
            "SELECT id, name, kind FROM db_properties
             WHERE database_id = ?1 AND deleted_at IS NULL ORDER BY position, id"
        }
        Deleted::Yes => {
            "SELECT id, name, kind FROM db_properties
             WHERE database_id = ?1 AND deleted_at IS NOT NULL ORDER BY deleted_at DESC, id"
        }
    };
    let mut options: HashMap<String, Vec<SelectOption>> = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT o.property_id, o.id, o.name, o.color FROM db_select_options o
         JOIN db_properties p ON p.id = o.property_id
         WHERE p.database_id = ?1 ORDER BY o.position, o.id",
    )?;
    let mut rows = stmt.query([database_id])?;
    while let Some(r) = rows.next()? {
        let color: String = r.get(3)?;
        options.entry(r.get(0)?).or_default().push(SelectOption {
            id: r.get(1)?,
            name: r.get(2)?,
            // The CHECK allows only known colors; never hide an option over one.
            color: OptionColor::parse(&color).unwrap_or(OptionColor::Gray),
        });
    }
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([database_id])?;
    let mut properties = Vec::new();
    while let Some(r) = rows.next()? {
        let kind: String = r.get(2)?;
        let Some(kind) = PropertyKind::parse(&kind) else {
            continue;
        };
        let id: String = r.get(0)?;
        properties.push(Property {
            options: options.remove(&id).unwrap_or_default(),
            id,
            name: r.get(1)?,
            kind,
        });
    }
    Ok(properties)
}

/// A cell from the columns `kind, text_value, number_value, bool_value,
/// date_value, option id` starting at `at`. `None` if it is empty or does
/// not fit its property (see [`CellValue::from_columns`]); a property kind
/// this version does not know reads as empty too.
fn read_cell(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<Option<CellValue>> {
    let kind: String = r.get(at)?;
    let Some(kind) = PropertyKind::parse(&kind) else {
        return Ok(None);
    };
    let text = match r.get::<_, Value>(at + 1)? {
        Value::Text(text) => Some(text),
        _ => None,
    };
    let number = match r.get::<_, Value>(at + 2)? {
        Value::Real(n) => Some(n),
        Value::Integer(n) => Some(n as f64),
        _ => None,
    };
    let flag = match r.get::<_, Value>(at + 3)? {
        Value::Integer(n) => Some(n),
        _ => None,
    };
    let date = match r.get::<_, Value>(at + 4)? {
        Value::Text(date) => Some(date),
        _ => None,
    };
    let option: Option<String> = r.get(at + 5)?;
    Ok(CellValue::from_columns(
        kind, text, number, flag, date, option,
    ))
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

/// Mark of a compaction in progress, in `settings` (ignored by
/// `Settings::from_pairs` like any unknown key).
const COMPACTING: &str = "compacting";

/// What the database file and its attachments take (H7.4), in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpaceUsage {
    /// The whole file (without its WAL).
    pub database: u64,
    /// Space inside the file that is free for reuse (`VACUUM` returns it).
    pub free: u64,
    pub used_attachments: usize,
    pub used_bytes: u64,
    /// Attachments no block uses.
    pub unused_attachments: usize,
    pub unused_bytes: u64,
}

/// Maps an `attachments` row (id, name, mime, size, width, height).
fn read_attachment(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: r.get(0)?,
        name: r.get(1)?,
        mime: r.get(2)?,
        size: r.get::<_, i64>(3)?.try_into().unwrap_or_default(),
        width: r.get(4)?,
        height: r.get(5)?,
    })
}

/// Keeps the text of the links to `page` equal to its title (ADR 0008).
fn retitle_links(tx: &Connection, page: &str, title: &str) -> Result<()> {
    tx.execute(
        "UPDATE blocks SET content = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE link_page_id = ?1 AND content <> ?2",
        params![page, title],
    )?;
    Ok(())
}

/// A page a link points at, as its card shows it (ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTarget {
    pub id: String,
    pub title: String,
    pub kind: PageKind,
    /// In the trash: the link says so, and opening it offers to restore.
    pub trashed: bool,
}

/// Maps a `blocks` row (id, page_id, kind, content, checked, position,
/// indent, attachment_id, link_page_id).
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
        attachment: r.get(7)?,
        link: r.get(8)?,
    })
}

/// Saves `blocks` and `title` as a version of `page` (ADR 0009) and thins
/// out its old versions, inside the caller's transaction. Unless `force`,
/// nothing is saved if the last version is younger than
/// [`versions::MIN_GAP_MINUTES`]. Never saves a copy of the last version,
/// nor a page without blocks. Returns whether it saved one.
fn save_version(
    tx: &Connection,
    clock: Option<&str>,
    page: &str,
    title: &str,
    blocks: &[Block],
    force: bool,
) -> Result<bool> {
    // An empty page has nothing worth restoring.
    if blocks.is_empty() {
        return Ok(false);
    }
    let now: String = match clock {
        Some(now) => now.to_owned(),
        None => tx.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0))?,
    };
    let json = versions::blocks_to_json(blocks);
    let last: Option<(String, i64, String, bool)> = tx
        .query_row(
            "SELECT title, format, blocks,
                    julianday(?2) - julianday(created_at) < ?3 / 1440.0
             FROM page_versions WHERE page_id = ?1
             ORDER BY created_at DESC, id DESC LIMIT 1",
            params![page, now, versions::MIN_GAP_MINUTES],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    if let Some((last_title, format, last_json, recent)) = last {
        let same = last_title == title && format == versions::FORMAT && last_json == json;
        if same || (recent && !force) {
            return Ok(false);
        }
    }
    tx.execute(
        "INSERT INTO page_versions (page_id, created_at, title, format, blocks, size)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![page, now, title, versions::FORMAT, json, json.len() as i64],
    )?;
    let dated: Vec<(i64, f64)> = tx
        .prepare("SELECT id, julianday(created_at) FROM page_versions WHERE page_id = ?1")?
        .query_map([page], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let now_jd: f64 = tx.query_row("SELECT julianday(?1)", [&now], |r| r.get(0))?;
    for id in versions::to_prune(&dated, now_jd) {
        tx.execute("DELETE FROM page_versions WHERE id = ?1", [id])?;
    }
    Ok(true)
}
