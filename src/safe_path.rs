//! File names derived from user text (page titles) that are safe on Windows,
//! macOS and Linux, and paths that cannot escape their root folder.
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

use anyhow::{Result, bail};

/// Longest file stem in bytes (filesystems allow ~255; room for " (n)" and
/// the extension).
const MAX_STEM_BYTES: usize = 120;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A file stem safe everywhere: no separators or characters Windows forbids,
/// no control characters, no leading dot (hidden files, `.`/`..`), no
/// trailing dot or space, not a reserved Windows device name, bounded length.
pub fn safe_stem(title: &str) -> String {
    let mut stem: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    stem = stem.trim().to_owned();
    let leading_dots = stem.chars().take_while(|c| *c == '.').count();
    stem = format!("{}{}", "_".repeat(leading_dots), &stem[leading_dots..]);
    stem = truncate_bytes(&stem, MAX_STEM_BYTES);
    stem = stem.trim_end_matches(['.', ' ']).to_owned();
    if stem.is_empty() {
        stem = "Sin título".to_owned();
    }
    let device = stem.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(device)) {
        stem.insert(0, '_');
    }
    stem
}

fn truncate_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// `safe_stem(title).extension`, made unique among `taken` (compared without
/// case, as on macOS and Windows) by appending " (2)", " (3)"…; the chosen
/// name is added to `taken`.
pub fn unique_file_name(title: &str, extension: &str, taken: &mut HashSet<String>) -> String {
    let stem = safe_stem(title);
    let mut candidate = format!("{stem}.{extension}");
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}).{extension}");
        n += 1;
    }
    taken.insert(candidate.to_lowercase());
    candidate
}

/// `root/part1/part2/…`, refusing anything that is not a single plain name
/// (no `..`, no absolute paths, no separators): the result is always inside
/// `root`.
pub fn join_inside(root: &Path, parts: &[&str]) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in parts {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) if name == *part => path.push(name),
            _ => bail!("Nombre de archivo no seguro: {part:?}"),
        }
    }
    Ok(path)
}
