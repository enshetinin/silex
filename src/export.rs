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

use crate::{markdown::page_to_markdown, model::Block, safe_path};

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
    fs::create_dir_all(dir)
        .with_context(|| format!("No se puede crear la carpeta {}", dir.display()))?;
    let mut taken: HashSet<String> = fs::read_dir(dir)
        .with_context(|| format!("No se puede leer la carpeta {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect();
    let name = safe_path::unique_file_name(title, "md", &mut taken);
    let path = safe_path::join_inside(dir, &[&name])?;
    write_atomic(&path, page_to_markdown(title, blocks).as_bytes())?;
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
