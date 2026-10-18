use std::{collections::HashSet, fs, path::Path, path::PathBuf};

use silex_desktop::export::{export_page, write_atomic};
use silex_desktop::markdown::page_to_markdown;
use silex_desktop::model::{Block, BlockKind};
use silex_desktop::safe_path::{join_inside, safe_stem, unique_file_name};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-export-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn names(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names: Vec<String> = fs::read_dir(dir)?
        .map(|e| Ok(e?.file_name().to_string_lossy().into_owned()))
        .collect::<anyhow::Result<_>>()?;
    names.sort();
    Ok(names)
}

mod names {
    use super::*;

    #[test]
    fn traversal_and_separators_cannot_escape() {
        for hostile in [
            "../../etc/passwd",
            "..\\..\\windows",
            "/abs/olute",
            "a/b",
            "..",
            ".",
        ] {
            let stem = safe_stem(hostile);
            assert!(!stem.contains(['/', '\\']), "{hostile:?} → {stem:?}");
            assert!(!stem.starts_with('.'), "{hostile:?} → {stem:?}");
            let root = Path::new("/raiz");
            let path = join_inside(root, &[&format!("{stem}.md")]).expect("safe");
            assert_eq!(path.parent(), Some(root), "{hostile:?}");
        }
    }

    #[test]
    fn forbidden_characters_controls_and_trailing_dots_are_replaced() {
        assert_eq!(
            safe_stem("¿Qué? <dos>: \"tres\" | *4*"),
            "¿Qué_ _dos__ _tres_ _ _4_"
        );
        assert_eq!(safe_stem("tab\there\u{0}nul"), "tab_here_nul");
        assert_eq!(safe_stem("final. . "), "final");
        assert_eq!(safe_stem("   "), "Sin título");
        assert_eq!(safe_stem(""), "Sin título");
    }

    #[test]
    fn reserved_windows_names_are_prefixed() {
        assert_eq!(safe_stem("CON"), "_CON");
        assert_eq!(safe_stem("nul"), "_nul");
        assert_eq!(safe_stem("com1.notas"), "_com1.notas");
        assert_eq!(safe_stem("CONSOLA"), "CONSOLA", "only exact device names");
    }

    #[test]
    fn unicode_is_kept_and_long_titles_are_cut_at_a_character_boundary() {
        assert_eq!(safe_stem("Año 日本 🙂"), "Año 日本 🙂");
        let long = "日本".repeat(100);
        let stem = safe_stem(&long);
        assert!(stem.len() <= 120);
        assert!(stem.chars().all(|c| c == '日' || c == '本'));
    }

    #[test]
    fn duplicates_get_numbers_without_regard_to_case() {
        let mut taken = HashSet::new();
        assert_eq!(unique_file_name("Nota", "md", &mut taken), "Nota.md");
        assert_eq!(unique_file_name("nota", "md", &mut taken), "nota (2).md");
        assert_eq!(unique_file_name("NOTA", "md", &mut taken), "NOTA (3).md");
    }

    #[test]
    fn join_inside_rejects_anything_but_plain_names() {
        let root = Path::new("/raiz");
        for bad in ["..", ".", "", "a/b", "/abs", "../x"] {
            assert!(join_inside(root, &[bad]).is_err(), "{bad:?}");
        }
        assert!(join_inside(root, &["carpeta", "nota.md"]).is_ok());
    }
}

mod writing {
    use super::*;

    fn blocks() -> Vec<Block> {
        vec![Block {
            id: "b".into(),
            page_id: "p".into(),
            kind: BlockKind::Bulleted,
            content: "uno".into(),
            checked: false,
            position: 0,
            indent: 0,
        }]
    }

    #[test]
    fn export_writes_markdown_named_after_the_title_and_never_overwrites() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let dir = tmp.0.join("Exportaciones"); // created on demand
        let first = export_page(&dir, "Mi: página", &blocks())?;
        let second = export_page(&dir, "Mi: página", &blocks())?;
        assert_eq!(names(&dir)?, ["Mi_ página (2).md", "Mi_ página.md"]);
        assert_eq!(
            fs::read_to_string(&first)?,
            page_to_markdown("Mi: página", &blocks())
        );
        assert_ne!(first, second);
        Ok(())
    }

    #[test]
    fn atomic_write_replaces_whole_file_and_leaves_no_temporaries() -> anyhow::Result<()> {
        let tmp = TempDir::new()?;
        let path = tmp.0.join("a.md");
        write_atomic(&path, b"primera")?;
        write_atomic(&path, "segunda 日本".as_bytes())?;
        assert_eq!(fs::read_to_string(&path)?, "segunda 日本");
        assert_eq!(names(&tmp.0)?, ["a.md"]);
        // Writing into a missing folder fails cleanly.
        assert!(write_atomic(&tmp.0.join("no-existe").join("x.md"), b"x").is_err());
        assert_eq!(names(&tmp.0)?, ["a.md"]);
        Ok(())
    }
}
