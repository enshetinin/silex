//! Writing exports to disk (milestone 4): where they go and how files are
//! written. Markdown generation itself is in `markdown`.
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::{ProjectDirs, UserDirs};

use crate::{
    db::Database,
    markdown::{database_to_markdown, page_to_markdown},
    model::{Block, PageKind, build_tree},
    safe_path,
};

/// Folder for exports: `SILEX_EXPORT_DIR` if set (testing), otherwise
/// `Documents/Silex/Exportaciones`, or the app data folder if the system
/// has no Documents folder.
pub fn export_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("SILEX_EXPORT_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(documents) = UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf)) {
        return Ok(documents.join("Silex").join("Exportaciones"));
    }
    let dirs = ProjectDirs::from("dev", "silex", "Silex")
        .context("No se encuentra una carpeta para las exportaciones")?;
    Ok(dirs.data_local_dir().join("exports"))
}

/// Writes one page as Markdown into `dir` (created if needed), named after
/// its title and never overwriting an existing file. Returns the file path.
pub fn export_page(dir: &Path, title: &str, blocks: &[Block]) -> Result<PathBuf> {
    export_markdown(dir, title, &page_to_markdown(title, blocks))
}

/// Writes ready Markdown (a page or a database) like [`export_page`].
pub fn export_markdown(dir: &Path, title: &str, markdown: &str) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("No se puede crear la carpeta {}", dir.display()))?;
    let mut taken: HashSet<String> = fs::read_dir(dir)
        .with_context(|| format!("No se puede leer la carpeta {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let name = safe_path::unique_file_name(title, "md", &mut taken);
    let path = safe_path::join_inside(dir, &[&name])?;
    write_atomic(&path, markdown.as_bytes())?;
    Ok(path)
}

/// Writes to a temporary file in the same folder, flushes it to disk and
/// renames it into place, so a crash never leaves a half-written file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let dir = path.parent().context("Ruta de archivo sin carpeta")?;
    let tmp = dir.join(format!(".silex-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.with_context(|| format!("No se pudo escribir {}", path.display()))
}

/// Opens a folder in the system file manager.
pub fn open_in_file_manager(dir: &Path) -> Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .with_context(|| format!("No se pudo abrir {}", dir.display()))?;
    Ok(())
}

/// Result of exporting the whole workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceExport {
    /// The new folder (`Silex <UTC stamp>`) inside the export root.
    pub dir: PathBuf,
    pub pages: usize,
}

/// Exports every live page (not the trash) as Markdown into a new folder
/// `root/Silex <stamp>`: each page is `Title.md`, and a page with subpages
/// also gets a `Title/` folder next to it holding them. Names are safe on all
/// systems and unique per folder.
///
/// Everything is written into a hidden temporary folder that is renamed into
/// place only at the end: an error leaves no partial export behind.
pub fn export_workspace(db: &Database, root: &Path) -> Result<WorkspaceExport> {
    fs::create_dir_all(root)
        .with_context(|| format!("No se puede crear la carpeta {}", root.display()))?;
    let stamp = db.utc_stamp()?;
    let pages = db.list_pages()?;
    let everything: HashSet<String> = pages.iter().map(|p| p.id.clone()).collect();
    // Depth-first rows with depth; never loops, orphans come at the root.
    let rows = build_tree(&pages, &everything);

    let tmp = root.join(format!(".silex-export-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&tmp).with_context(|| format!("No se puede escribir en {}", root.display()))?;
    let written = (|| -> Result<usize> {
        // One (folder, names taken) per depth.
        let mut stack: Vec<(PathBuf, HashSet<String>)> = vec![(tmp.clone(), HashSet::new())];
        for row in &rows {
            stack.truncate(row.depth + 1);
            let Some((folder, taken)) = stack.last_mut() else {
                anyhow::bail!("Árbol de páginas incoherente");
            };
            let stem = safe_path::unique_stem(&row.page.title, "md", taken);
            let file = safe_path::join_inside(folder, &[&format!("{stem}.md")])?;
            let markdown = match row.page.kind {
                // Its rows follow as subpages, in the folder below.
                PageKind::Database => {
                    let rows = db.database_rows(&row.page.id)?;
                    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
                    database_to_markdown(&row.page.title, &titles)
                }
                PageKind::Page => page_to_markdown(&row.page.title, &db.list_blocks(&row.page.id)?),
            };
            write_atomic(&file, markdown.as_bytes())?;
            if row.has_children {
                let sub = safe_path::join_inside(folder, &[&stem])?;
                fs::create_dir(&sub)
                    .with_context(|| format!("No se puede crear {}", sub.display()))?;
                stack.push((sub, HashSet::new()));
            }
        }
        Ok(rows.len())
    })();
    let pages = match written {
        Ok(pages) => pages,
        Err(err) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(err);
        }
    };

    let mut taken: HashSet<String> = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let name = safe_path::unique_stem(&format!("Silex {stamp}"), "md", &mut taken);
    let dir = safe_path::join_inside(root, &[&name])?;
    if let Err(err) = fs::rename(&tmp, &dir) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(err).with_context(|| format!("No se pudo finalizar {}", dir.display()));
    }
    Ok(WorkspaceExport { dir, pages })
}

/// Workspace export on a worker thread with its own database connection.
pub struct WorkspaceExportJob {
    rx: std::sync::mpsc::Receiver<Result<WorkspaceExport>>,
}

impl WorkspaceExportJob {
    pub fn spawn(
        db_path: PathBuf,
        root: PathBuf,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("silex-export".into())
            .spawn(move || {
                let result = Database::open(&db_path).and_then(|db| export_workspace(&db, &root));
                let _ = tx.send(result);
                on_done();
            })
            .context("No se puede iniciar el hilo de exportación")?;
        Ok(Self { rx })
    }

    /// `None` while running; the result once, when finished.
    pub fn poll(&self) -> Option<Result<WorkspaceExport>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "El hilo de exportación terminó inesperadamente"
            ))),
        }
    }
}
