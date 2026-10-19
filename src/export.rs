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

use std::collections::HashMap;

use crate::{
    attachment::FileKind,
    db::Database,
    markdown::{FileLink, database_to_markdown, page_to_markdown, page_to_markdown_with_files},
    model::{Block, BlockKind, PageKind, build_tree},
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

/// [`export_page`] for a page that may have attachments (ADR 0007): they
/// are written to `Title (adjuntos)/` next to `Title.md` and linked from
/// it. The Markdown is written last; if anything fails, the folder of
/// attachments is removed.
pub fn export_page_with_files(
    db: &Database,
    dir: &Path,
    title: &str,
    blocks: &[Block],
) -> Result<PathBuf> {
    if !blocks.iter().any(|b| b.kind == BlockKind::File) {
        return export_page(dir, title, blocks);
    }
    fs::create_dir_all(dir)
        .with_context(|| format!("No se puede crear la carpeta {}", dir.display()))?;
    let mut taken: HashSet<String> = fs::read_dir(dir)
        .with_context(|| format!("No se puede leer la carpeta {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let stem = safe_path::unique_stem(title, "md", &mut taken);
    let mut written = None;
    let result = (|| -> Result<PathBuf> {
        let files = write_attachments(db, dir, &stem, blocks, &mut taken, &mut written)?;
        let path = safe_path::join_inside(dir, &[&format!("{stem}.md")])?;
        write_atomic(
            &path,
            page_to_markdown_with_files(title, blocks, &files).as_bytes(),
        )?;
        Ok(path)
    })();
    if result.is_err()
        && let Some(folder) = written
    {
        let _ = fs::remove_dir_all(folder);
    }
    result
}

/// Writes the attachments of `blocks` into a new folder `«stem» (adjuntos)`
/// inside `dir` (its name reserved in `taken`) and returns their links,
/// relative to `dir`. Nothing is written when no block is a file. `folder`
/// is set as soon as the folder exists, so the caller can remove it.
fn write_attachments(
    db: &Database,
    dir: &Path,
    stem: &str,
    blocks: &[Block],
    taken: &mut HashSet<String>,
    folder: &mut Option<PathBuf>,
) -> Result<HashMap<String, FileLink>> {
    let ids: Vec<&String> = {
        let mut seen = HashSet::new();
        blocks
            .iter()
            .filter_map(|b| b.attachment.as_ref())
            .filter(|id| seen.insert(id.as_str()))
            .collect()
    };
    let mut links = HashMap::new();
    if ids.is_empty() {
        return Ok(links);
    }
    let base = format!("{stem} (adjuntos)");
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name.to_lowercase()) {
        name = format!("{base} {n}");
        n += 1;
    }
    taken.insert(name.to_lowercase());
    let path = safe_path::join_inside(dir, &[&name])?;
    fs::create_dir(&path).with_context(|| format!("No se puede crear {}", path.display()))?;
    *folder = Some(path.clone());
    let mut names = HashSet::new();
    for id in ids {
        let Some(attachment) = db.attachment(id)? else {
            continue;
        };
        let file = safe_path::unique_attachment_name(&attachment.name, &mut names);
        write_atomic(
            &safe_path::join_inside(&path, &[&file])?,
            &db.attachment_data(id)?,
        )?;
        links.insert(
            id.clone(),
            FileLink {
                path: format!("{name}/{file}"),
                image: attachment.kind() == FileKind::Image,
            },
        );
    }
    Ok(links)
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
/// also gets a `Title/` folder next to it holding them, and a page with
/// attachments a `Title (adjuntos)/` folder (ADR 0007). Names are safe on
/// all systems and unique per folder.
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
                    let id = &row.page.id;
                    database_to_markdown(
                        &row.page.title,
                        &db.database_properties(id)?,
                        &db.database_rows(id)?,
                        &db.database_values(id)?,
                    )
                }
                PageKind::Page => {
                    let blocks = db.list_blocks(&row.page.id)?;
                    // Inside the temporary folder: removed with it on error.
                    let files = write_attachments(db, folder, &stem, &blocks, taken, &mut None)?;
                    page_to_markdown_with_files(&row.page.title, &blocks, &files)
                }
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
