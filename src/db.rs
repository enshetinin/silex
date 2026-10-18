//! All SQL lives here. Callers never construct SQL from user input.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use directories::ProjectDirs;
use rusqlite::{Connection, MAIN_DB, OptionalExtension, params};
use uuid::Uuid;

use crate::model::{Block, Page, TrashItem};

pub mod backup;

/// Published migrations, in order. Never edit one that has shipped: add a new file.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("db/migrations/0001_init.sql")),
    (2, include_str!("db/migrations/0002_favorites.sql")),
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
    /// Opens the user's database. `SILEX_DB_PATH` overrides the location
    /// (development and manual testing, so real data stays untouched).
    pub fn open_default() -> Result<Self> {
        if let Some(path) = std::env::var_os("SILEX_DB_PATH") {
            return Self::open(path);
        }
        let dirs = ProjectDirs::from("dev", "silex", "Silex")
            .context("No se encuentra el directorio de datos del sistema")?;
        std::fs::create_dir_all(dirs.data_local_dir())
            .context("No se puede crear el directorio de datos")?;
        Self::open(dirs.data_local_dir().join("silex.sqlite3"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).context("No se puede abrir SQLite")?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate(PreMigrationBackup::Yes)?;
        Ok(db)
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
            "SELECT id, parent_id, title FROM pages
             WHERE favorite_at IS NOT NULL AND deleted_at IS NULL
             ORDER BY favorite_at, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Page {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_pages(&self) -> Result<Vec<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, title FROM pages WHERE deleted_at IS NULL ORDER BY sort_order, created_at, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Page {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
            })
        })?;
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

    pub fn get_page(&self, id: &str) -> Result<Option<Page>> {
        self.conn
            .query_row(
                "SELECT id, parent_id, title FROM pages WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                |r| {
                    Ok(Page {
                        id: r.get(0)?,
                        parent_id: r.get(1)?,
                        title: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_blocks(&self, page_id: &str) -> Result<Vec<Block>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, page_id, kind, content, checked, position
             FROM blocks WHERE page_id = ?1 ORDER BY position, rowid",
        )?;
        let rows = stmt.query_map([page_id], |r| {
            Ok(Block {
                id: r.get(0)?,
                page_id: r.get(1)?,
                kind: r.get(2)?,
                content: r.get(3)?,
                checked: r.get(4)?,
                position: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_block(&self, page_id: &str, kind: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO blocks (id, page_id, kind, position)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), -1) + 1 FROM blocks WHERE page_id = ?2))",
            params![id, page_id, kind],
        )?;
        Ok(id)
    }

    /// Persist title and current blocks together; IDs always come from the loaded document.
    pub fn save_document(&mut self, page_id: &str, title: &str, blocks: &[Block]) -> Result<()> {
        let tx = self.conn.transaction()?;
        let affected = tx.execute(
            "UPDATE pages SET title = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            params![page_id, title],
        )?;
        if affected != 1 {
            bail!("La página ya no existe o está en la papelera");
        }
        for block in blocks {
            let affected = tx.execute(
                "UPDATE blocks SET content = ?3, checked = ?4,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1 AND page_id = ?2",
                params![block.id, page_id, block.content, block.checked],
            )?;
            if affected != 1 {
                bail!("Bloque ausente: {}", block.id);
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
            "SELECT id, parent_id, title, deleted_at FROM pages
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, title",
        )?;
        let trashed: Vec<(Page, String)> = stmt
            .query_map([], |r| {
                Ok((
                    Page {
                        id: r.get(0)?,
                        parent_id: r.get(1)?,
                        title: r.get(2)?,
                    },
                    r.get(3)?,
                ))
            })?
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

    /// Escape arbitrary input into quoted alphanumeric tokens for safe FTS queries.
    pub fn search_pages(&self, term: &str) -> Result<Vec<Page>> {
        if term.trim().is_empty() {
            return self.list_pages();
        }
        let normalized: String = term
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect();
        let terms: Vec<String> = normalized
            .split_whitespace()
            .map(|part| format!("\"{part}\""))
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let phrase = terms.join(" AND ");
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT p.id, p.parent_id, p.title FROM pages p
             WHERE p.deleted_at IS NULL AND (
                 p.rowid IN (SELECT rowid FROM page_search WHERE page_search MATCH ?1)
                 OR p.id IN (
                     SELECT b.page_id FROM blocks b
                     WHERE b.rowid IN (SELECT rowid FROM block_search WHERE block_search MATCH ?1)
                 )
             ) ORDER BY p.title",
        )?;
        let rows = stmt.query_map([phrase], |r| {
            Ok(Page {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                title: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
