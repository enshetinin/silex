//! Local data kept private (H7.12, ADR 0012): files only their owner can
//! read on Unix, and spotting a database inside a folder that a cloud
//! service syncs (an SQLite database in use can be damaged by it).
//!
//! On Windows, permissions are ACLs and the user's own folders are already
//! private: nothing is changed there.
use std::path::{Component, Path, PathBuf};

use anyhow::Result;

/// The cloud service that syncs the folder holding `path`, if it is one
/// Silex knows: iCloud Drive, Dropbox, OneDrive or Google Drive. Looks at
/// the folder names only (no file system access), without case.
pub fn synced_folder(path: &Path) -> Option<&'static str> {
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    for (index, name) in names.iter().enumerate() {
        let next = names.get(index + 1).map(String::as_str);
        let service = match name.as_str() {
            // macOS: ~/Library/Mobile Documents (iCloud Drive); Windows:
            // iCloudDrive.
            "mobile documents" | "iclouddrive" | "icloud drive" => Some("iCloud Drive"),
            // macOS 12.3+: ~/Library/CloudStorage/<Service>-<account>.
            "cloudstorage" => next.and_then(cloud_storage_service),
            "google drive" | "my drive" | "mi unidad" => Some("Google Drive"),
            n if n == "dropbox" || n.starts_with("dropbox (") => Some("Dropbox"),
            n if n == "onedrive" || n.starts_with("onedrive -") || n.starts_with("onedrive-") => {
                Some("OneDrive")
            }
            n if n.starts_with("googledrive") => Some("Google Drive"),
            _ => None,
        };
        if service.is_some() {
            return service;
        }
    }
    None
}

/// The service of a folder in macOS's `~/Library/CloudStorage`.
fn cloud_storage_service(name: &str) -> Option<&'static str> {
    if name.starts_with("dropbox") {
        Some("Dropbox")
    } else if name.starts_with("onedrive") {
        Some("OneDrive")
    } else if name.starts_with("googledrive") {
        Some("Google Drive")
    } else if name.starts_with("icloud") {
        Some("iCloud Drive")
    } else {
        None
    }
}

/// Leaves `path` (a folder when `dir`) readable only by its owner: `0700`
/// for folders, `0600` for files. Returns whether it had to change it. A
/// missing path is not an error (nothing to protect). No-op on Windows.
pub fn restrict(path: &Path, dir: bool) -> Result<bool> {
    #[cfg(unix)]
    {
        use anyhow::Context;
        use std::os::unix::fs::PermissionsExt;
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err).with_context(|| format!("No se puede leer {}", path.display()));
            }
        };
        let mode = metadata.permissions().mode() & 0o777;
        let wanted = if dir { 0o700 } else { 0o600 };
        // Only take away: never add a permission the owner did not have.
        let restricted = mode & wanted;
        if mode == restricted {
            return Ok(false);
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(restricted))
            .with_context(|| format!("No se pueden ajustar los permisos de {}", path.display()))?;
        Ok(true)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, dir);
        Ok(false)
    }
}

/// Restricts the database with its `-wal` and `-shm`, the backups folder
/// with every file in it and, when `own_folder` (Silex's own data folder,
/// not one the user chose), the database's folder. Returns the paths
/// changed.
pub fn harden(db_path: &Path, backups: &Path, own_folder: bool) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    let mut check = |path: PathBuf, dir: bool| -> Result<()> {
        if restrict(&path, dir)? {
            changed.push(path);
        }
        Ok(())
    };
    if own_folder && let Some(folder) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        check(folder.to_path_buf(), true)?;
    }
    for suffix in ["", "-wal", "-shm"] {
        check(
            PathBuf::from(format!("{}{suffix}", db_path.display())),
            false,
        )?;
    }
    check(backups.to_path_buf(), true)?;
    if let Ok(entries) = std::fs::read_dir(backups) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                check(entry.path(), false)?;
            }
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn known_synced_folders_are_spotted_by_name() {
        let cases = [
            (
                vec![
                    "Users",
                    "ana",
                    "Library",
                    "Mobile Documents",
                    "com~apple~CloudDocs",
                    "silex.sqlite3",
                ],
                Some("iCloud Drive"),
            ),
            (
                vec![
                    "Users",
                    "ana",
                    "Library",
                    "CloudStorage",
                    "Dropbox",
                    "silex.sqlite3",
                ],
                Some("Dropbox"),
            ),
            (
                vec![
                    "Users",
                    "ana",
                    "Library",
                    "CloudStorage",
                    "OneDrive-Personal",
                    "x",
                ],
                Some("OneDrive"),
            ),
            (
                vec![
                    "Users",
                    "ana",
                    "Library",
                    "CloudStorage",
                    "GoogleDrive-ana@ejemplo.org",
                    "My Drive",
                    "x",
                ],
                Some("Google Drive"),
            ),
            (
                vec!["home", "ana", "Dropbox", "notas", "silex.sqlite3"],
                Some("Dropbox"),
            ),
            (
                vec!["home", "ana", "Dropbox (Empresa)", "silex.sqlite3"],
                Some("Dropbox"),
            ),
            (
                vec!["Users", "ana", "OneDrive - Empresa", "silex.sqlite3"],
                Some("OneDrive"),
            ),
            (
                vec!["Users", "ana", "OneDrive", "Documentos", "silex.sqlite3"],
                Some("OneDrive"),
            ),
            (
                vec!["Users", "ana", "iCloudDrive", "silex.sqlite3"],
                Some("iCloud Drive"),
            ),
            (
                vec!["Volumes", "GoogleDrive", "Mi unidad", "silex.sqlite3"],
                Some("Google Drive"),
            ),
        ];
        for (parts, expected) in cases {
            assert_eq!(synced_folder(&path(&parts)), expected, "{parts:?}");
        }
    }

    #[test]
    fn ordinary_folders_are_not_synced() {
        for parts in [
            vec![
                "Users",
                "ana",
                "Library",
                "Application Support",
                "dev.silex.Silex",
                "silex.sqlite3",
            ],
            vec!["home", "ana", ".local", "share", "silex", "silex.sqlite3"],
            vec![
                "Users",
                "ana",
                "AppData",
                "Local",
                "silex",
                "Silex",
                "data",
                "silex.sqlite3",
            ],
            // Names that only contain a service's name.
            vec!["home", "ana", "dropboxes", "silex.sqlite3"],
            vec!["home", "ana", "notas sobre onedrive", "silex.sqlite3"],
            vec!["Users", "ana", "Library", "CloudStorage", "OtraCosa", "x"],
        ] {
            assert_eq!(synced_folder(&path(&parts)), None, "{parts:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn files_and_folders_end_up_only_for_their_owner() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("silex-privacy-{}", uuid::Uuid::new_v4()));
        let backups = root.join("backups");
        std::fs::create_dir_all(&backups)?;
        let db = root.join("silex.sqlite3");
        for file in [
            db.clone(),
            root.join("silex.sqlite3-wal"),
            backups.join("copia.sqlite3"),
        ] {
            std::fs::write(&file, b"x")?;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
        }
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))?;
        std::fs::set_permissions(&backups, std::fs::Permissions::from_mode(0o775))?;
        let mode =
            |p: &Path| -> Result<u32> { Ok(std::fs::metadata(p)?.permissions().mode() & 0o777) };

        // A folder the user chose (e.g. /tmp) is left alone.
        let changed = harden(&db, &backups, false)?;
        assert_eq!(changed.len(), 4, "{changed:?}");
        assert_eq!(mode(&root)?, 0o755);
        let changed = harden(&db, &backups, true)?;
        assert_eq!(changed, std::slice::from_ref(&root));
        assert_eq!(mode(&root)?, 0o700);
        assert_eq!(mode(&backups)?, 0o700);
        assert_eq!(mode(&db)?, 0o600);
        assert_eq!(mode(&root.join("silex.sqlite3-wal"))?, 0o600);
        assert_eq!(mode(&backups.join("copia.sqlite3"))?, 0o600);
        assert!(harden(&db, &backups, true)?.is_empty(), "already private");
        // Never adds: a read-only file stays read-only.
        std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o444))?;
        assert!(restrict(&db, false)?);
        assert_eq!(mode(&db)?, 0o400);
        // A missing file is nothing to protect.
        assert!(!restrict(&root.join("no-existe"), false)?);
        std::fs::remove_dir_all(&root)?;
        Ok(())
    }
}
