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
    markdown::{FileLink, database_to_markdown, page_to_markdown, page_to_markdown_with},
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
    let folder = files_folder(&stem, blocks, &mut taken);
    let mut written = None;
    let result = (|| -> Result<PathBuf> {
        let files = match &folder {
            Some(name) => write_attachments(db, dir, name, blocks, &mut written)?,
            None => HashMap::new(),
        };
        let path = safe_path::join_inside(dir, &[&format!("{stem}.md")])?;
        // A page alone: the pages it links to are not exported (ADR 0008).
        let markdown = page_to_markdown_with(title, blocks, &files, &HashMap::new());
        write_atomic(&path, markdown.as_bytes())?;
        Ok(path)
    })();
    if result.is_err()
        && let Some(folder) = written
    {
        let _ = fs::remove_dir_all(folder);
    }
    result
}

/// The name of the folder for the attachments of `blocks`, `«stem»
/// (adjuntos)`, made unique and reserved in `taken`; `None` when no block
/// is a file.
fn files_folder(stem: &str, blocks: &[Block], taken: &mut HashSet<String>) -> Option<String> {
    if !blocks.iter().any(|b| b.attachment.is_some()) {
        return None;
    }
    let base = format!("{stem} (adjuntos)");
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name.to_lowercase()) {
        name = format!("{base} {n}");
        n += 1;
    }
    taken.insert(name.to_lowercase());
    Some(name)
}

/// Writes the attachments of `blocks` into the new folder `name` inside
/// `dir` and returns their links, relative to `dir`. `folder` is set as
/// soon as the folder exists, so the caller can remove it.
fn write_attachments(
    db: &Database,
    dir: &Path,
    name: &str,
    blocks: &[Block],
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
    let path = safe_path::join_inside(dir, &[name])?;
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
    remove_stale_temporaries(dir, STALE_AFTER);
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

/// Temporaries older than this are left by an export that was cut short
/// (the app was killed, H9.1): no export running takes so long.
pub const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(3600);

/// Removes Silex's hidden temporaries in `dir` (`.silex-export-*`
/// folders and `.silex-*.tmp` files) last changed more than `older_than`
/// ago. Best effort: what cannot be removed stays for next time. Returns
/// how many went.
pub fn remove_stale_temporaries(dir: &Path, older_than: std::time::Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = name.starts_with(".silex-export-")
            || (name.starts_with(".silex-") && name.ends_with(".tmp"));
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age >= older_than);
        if !ours || !old {
            continue;
        }
        let path = entry.path();
        let gone = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if gone.is_ok() {
            removed += 1;
        }
    }
    removed
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
    remove_stale_temporaries(root, STALE_AFTER);
    let stamp = db.utc_stamp()?;
    let pages = db.list_pages()?;
    let everything: HashSet<String> = pages.iter().map(|p| p.id.clone()).collect();
    // Depth-first rows with depth; never loops, orphans come at the root.
    let rows = build_tree(&pages, &everything);
    // Templates go to a folder of their own, after the pages (ADR 0010).
    let templates = db.list_templates()?;
    let entries: Vec<(&crate::model::Page, bool)> = rows
        .iter()
        .map(|r| (&r.page, r.has_children))
        .chain(templates.iter().map(|t| (t, false)))
        .collect();

    // First every name, so links can point at pages written later.
    let plan = plan_workspace(db, &rows, &templates)?;
    let targets: HashMap<String, PathBuf> = plan
        .iter()
        .map(|p| (p.id.clone(), p.folder.join(format!("{}.md", p.stem))))
        .collect();

    let tmp = root.join(format!(".silex-export-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&tmp).with_context(|| format!("No se puede escribir en {}", root.display()))?;
    let written = (|| -> Result<usize> {
        for ((page, has_children), planned) in entries.iter().zip(&plan) {
            let folder = join_relative(&tmp, &planned.folder)?;
            // The templates' folder is the only one not made by a parent.
            fs::create_dir_all(&folder)
                .with_context(|| format!("No se puede crear {}", folder.display()))?;
            let file = safe_path::join_inside(&folder, &[&format!("{}.md", planned.stem)])?;
            let markdown = match (&page.kind, &planned.blocks) {
                // Its rows follow as subpages, in the folder below.
                (PageKind::Database, _) => {
                    let id = &page.id;
                    database_to_markdown(
                        &page.title,
                        &db.database_properties(id)?,
                        &db.database_rows(id)?,
                        &db.database_values(id)?,
                    )
                }
                (PageKind::Page, blocks) => {
                    // Inside the temporary folder: removed with it on error.
                    let files = match &planned.files {
                        Some(name) => write_attachments(db, &folder, name, blocks, &mut None)?,
                        None => HashMap::new(),
                    };
                    let links: HashMap<String, String> = blocks
                        .iter()
                        .filter_map(|b| b.link.as_ref())
                        .filter_map(|id| {
                            let to = targets.get(id)?;
                            Some((id.clone(), relative_link(&planned.folder, to)))
                        })
                        .collect();
                    page_to_markdown_with(&page.title, blocks, &files, &links)
                }
            };
            write_atomic(&file, markdown.as_bytes())?;
            if *has_children {
                let sub = safe_path::join_inside(&folder, &[&planned.stem])?;
                fs::create_dir(&sub)
                    .with_context(|| format!("No se puede crear {}", sub.display()))?;
            }
        }
        Ok(entries.len())
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

/// Where one page goes in a workspace export, decided before writing.
struct Planned {
    id: String,
    /// Its folder, relative to the export's root.
    folder: PathBuf,
    stem: String,
    /// Its attachments' folder, inside `folder`.
    files: Option<String>,
    /// Its blocks (empty for a database).
    blocks: Vec<Block>,
}

/// Names every page of `rows` (depth-first) as the export writes them:
/// unique per folder, `Title.md` next to its `Title/` and `Title
/// (adjuntos)/` folders.
fn plan_workspace(
    db: &Database,
    rows: &[crate::model::TreeRow],
    templates: &[crate::model::Page],
) -> Result<Vec<Planned>> {
    // One (folder, names taken) per depth.
    let mut stack: Vec<(PathBuf, HashSet<String>)> = vec![(PathBuf::new(), HashSet::new())];
    let mut plan = Vec::with_capacity(rows.len());
    for row in rows {
        stack.truncate(row.depth + 1);
        let Some((folder, taken)) = stack.last_mut() else {
            anyhow::bail!("Árbol de páginas incoherente");
        };
        let stem = safe_path::unique_stem(&row.page.title, "md", taken);
        let blocks = match row.page.kind {
            PageKind::Page => db.list_blocks(&row.page.id)?,
            PageKind::Database => Vec::new(),
        };
        let files = files_folder(&stem, &blocks, taken);
        let folder = folder.clone();
        if row.has_children {
            stack.push((folder.join(&stem), HashSet::new()));
        }
        plan.push(Planned {
            id: row.page.id.clone(),
            folder,
            stem,
            files,
            blocks,
        });
    }
    // Templates: in «Plantillas/» (or «Plantillas (2)/» if a page took the
    // name), each with its attachments next to it.
    if !templates.is_empty() {
        let Some((_, root_taken)) = stack.first_mut() else {
            anyhow::bail!("Árbol de páginas incoherente");
        };
        let folder = PathBuf::from(safe_path::unique_stem("Plantillas", "md", root_taken));
        let mut taken = HashSet::new();
        for template in templates {
            let stem = safe_path::unique_stem(&template.title, "md", &mut taken);
            let blocks = db.list_blocks(&template.id)?;
            let files = files_folder(&stem, &blocks, &mut taken);
            plan.push(Planned {
                id: template.id.clone(),
                folder: folder.clone(),
                stem,
                files,
                blocks,
            });
        }
    }
    Ok(plan)
}

/// `root` joined with each part of `relative` (names made by `safe_path`),
/// never leaving `root`.
fn join_relative(root: &Path, relative: &Path) -> Result<PathBuf> {
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    safe_path::join_inside(root, &parts)
}

/// The link from a file in folder `from` to the file `to` (both relative
/// to the same root), with `/`: «../Otra/Página.md».
pub fn relative_link(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// An export on a worker thread (`silex-export`) with its own database
/// connection, so writing files never freezes the window.
pub struct ExportJob<T> {
    rx: std::sync::mpsc::Receiver<Result<T>>,
}

/// The whole workspace, exported on a worker thread (H4.3).
pub type WorkspaceExportJob = ExportJob<WorkspaceExport>;

/// One page and its attachments, exported on a worker thread (H7.9):
/// attachments can be up to 20 MB each.
pub type PageExportJob = ExportJob<PathBuf>;

impl<T: Send + 'static> ExportJob<T> {
    /// Opens `db_path` on a new thread and runs `work` with it; `on_done`
    /// runs when the result is ready (to wake the UI).
    fn start(
        db_path: PathBuf,
        work: impl FnOnce(&Database) -> Result<T> + Send + 'static,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("silex-export".into())
            .spawn(move || {
                let result = Database::open(&db_path).and_then(|db| work(&db));
                let _ = tx.send(result);
                on_done();
            })
            .context("No se puede iniciar el hilo de exportación")?;
        Ok(Self { rx })
    }

    /// `None` while running; the result once, when finished.
    pub fn poll(&self) -> Option<Result<T>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(
                "El hilo de exportación terminó inesperadamente"
            ))),
        }
    }
}

impl WorkspaceExportJob {
    pub fn spawn(
        db_path: PathBuf,
        root: PathBuf,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        Self::start(db_path, move |db| export_workspace(db, &root), on_done)
    }
}

impl PageExportJob {
    /// [`export_page_with_files`] of `title` and `blocks` (as the window
    /// shows them; their attachments are read from `db_path`) into `dir`.
    pub fn spawn(
        db_path: PathBuf,
        dir: PathBuf,
        title: String,
        blocks: Vec<Block>,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {
        Self::start(
            db_path,
            move |db| export_page_with_files(db, &dir, &title, &blocks),
            on_done,
        )
    }
}
