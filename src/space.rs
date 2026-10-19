//! «Liberar espacio» (H7.4, ADR 0007): deletes the attachments no block
//! uses and compacts the database so the file really shrinks. `VACUUM`
//! rewrites the whole file, so it runs in a `silex-space` thread with its
//! own connection; [`free_space`] is the same work without the thread.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::{Context, Result, anyhow};

use crate::db::Database;

/// What freeing space did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Freed {
    /// Unused attachments deleted.
    pub attachments: usize,
    /// How much smaller the database file is.
    pub bytes: u64,
}

/// Deletes the unused attachments except `keep` (what undo can bring
/// back), then compacts.
pub fn free_space(db: &mut Database, keep: &HashSet<String>) -> Result<Freed> {
    let before = db.space_usage()?.database;
    let (attachments, _) = db.delete_unused_attachments(keep)?;
    db.compact()?;
    let after = db.space_usage()?.database;
    Ok(Freed {
        attachments,
        bytes: before.saturating_sub(after),
    })
}

/// [`free_space`] on a `silex-space` thread.
pub struct FreeSpaceJob {
    rx: mpsc::Receiver<Result<Freed>>,
}

impl FreeSpaceJob {
    /// `on_done` runs when it finishes (e.g. to repaint the UI).
    pub fn spawn(
        db_path: PathBuf,
        keep: HashSet<String>,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("silex-space".into())
            .spawn(move || {
                let result = Database::open(&db_path)
                    .map_err(|err| anyhow!("No se puede abrir la base: {err:#}"))
                    .and_then(|mut db| free_space(&mut db, &keep));
                let _ = tx.send(result);
                on_done();
            })
            .context("No se puede iniciar el hilo de espacio")?;
        Ok(Self { rx })
    }

    /// The result, once, when it arrives.
    pub fn poll(&self) -> Option<Result<Freed>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow!("El hilo de espacio se detuvo")))
            }
        }
    }
}
