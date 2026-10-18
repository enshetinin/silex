//! All SQL lives here. Callers never construct SQL from user input.
use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::model::{Block, Page};

const LATEST_SCHEMA_VERSION: i64 = 1;
const SCHEMA_V1: &str = include_str!("db/migrations/0001_init.sql");

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
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.configure()?;
        db.migrate()?;
        Ok(db)
    }

    fn configure(&self) -> Result<()> {
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        self.conn.pragma_update(None, "synchronous", "FULL")?;
        self.conn.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > LATEST_SCHEMA_VERSION {
            bail!("La base de datos usa un esquema más reciente ({version})");
        }
        if version == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
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

    pub fn trash_page(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pages SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            [id],
        )?;
        Ok(())
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
