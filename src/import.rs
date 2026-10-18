//! Reading Markdown files to import (milestone 4), off the UI thread.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use anyhow::{Context, Result, anyhow, ensure};

use crate::markdown::{ImportedPage, markdown_to_page};

/// Largest file accepted (a guard against dropping something huge by mistake).
pub const MAX_IMPORT_BYTES: u64 = 10 * 1024 * 1024;

/// `.md` or `.markdown`, ignoring case.
pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// Reads and parses one Markdown file. The file name (without extension) is
/// the title unless the file starts with `# Title`. Files that are too big or
/// not valid UTF-8 are rejected rather than imported as garbage.
pub fn read_markdown_file(path: &Path, max_bytes: u64) -> Result<ImportedPage> {
    ensure!(
        is_markdown_path(path),
        "{} no es un archivo .md",
        path.display()
    );
    let size = fs::metadata(path)
        .with_context(|| format!("No se puede leer {}", path.display()))?
        .len();
    ensure!(
        size <= max_bytes,
        "{} es demasiado grande ({} MB; máximo {} MB)",
        path.display(),
        size / (1024 * 1024),
        max_bytes / (1024 * 1024)
    );
    let bytes = fs::read(path).with_context(|| format!("No se puede leer {}", path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow!("{} no está en UTF-8; no se importa", path.display()))?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Importada".to_owned());
    Ok(markdown_to_page(&text, &stem))
}

/// Files read and parsed on a worker thread; the UI polls for the result.
pub struct ImportJob {
    rx: mpsc::Receiver<Vec<(PathBuf, Result<ImportedPage>)>>,
}

impl ImportJob {
    pub fn spawn(paths: Vec<PathBuf>, on_done: impl FnOnce() + Send + 'static) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("silex-import".into())
            .spawn(move || {
                let results = paths
                    .into_iter()
                    .map(|path| {
                        let page = read_markdown_file(&path, MAX_IMPORT_BYTES);
                        (path, page)
                    })
                    .collect();
                let _ = tx.send(results);
                on_done();
            })
            .context("No se puede iniciar el hilo de importación")?;
        Ok(Self { rx })
    }

    /// `None` while running; the results once, when finished.
    pub fn poll(&self) -> Option<Vec<(PathBuf, Result<ImportedPage>)>> {
        match self.rx.try_recv() {
            Ok(results) => Some(results),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
        }
    }
}
