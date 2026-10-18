use std::{fs, path::PathBuf, sync::mpsc, time::Duration};

use silex_desktop::db::Database;
use silex_desktop::import::{ImportJob, is_markdown_path, read_markdown_file};
use silex_desktop::model::{BlockKind, NewBlock};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn file(&self, name: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
        let path = self.0.join(name);
        fs::write(&path, bytes)?;
        Ok(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn markdown_extensions_are_recognised_without_case() {
    for ok in ["a.md", "b.MD", "c.markdown", "d.Markdown"] {
        assert!(is_markdown_path(std::path::Path::new(ok)), "{ok}");
    }
    for no in ["a.txt", "md", "a.md.bak", "a"] {
        assert!(!is_markdown_path(std::path::Path::new(no)), "{no}");
    }
}

#[test]
fn reads_a_file_with_title_from_h1_or_file_name() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let with_h1 = tmp.file("nota.md", "# Título real\n\n- uno\n".as_bytes())?;
    let page = read_markdown_file(&with_h1, 1024)?;
    assert_eq!(page.title, "Título real");
    assert_eq!(page.blocks.len(), 1);

    let without = tmp.file("Diario 2026.md", "texto suelto\n".as_bytes())?;
    assert_eq!(read_markdown_file(&without, 1024)?.title, "Diario 2026");
    Ok(())
}

#[test]
fn rejects_too_big_non_utf8_wrong_extension_and_missing_files() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let big = tmp.file("grande.md", &[b'a'; 2048])?;
    let err = read_markdown_file(&big, 1024).expect_err("too big");
    assert!(format!("{err:#}").contains("demasiado grande"));

    let latin1 = tmp.file("latin1.md", b"caf\xe9")?;
    let err = read_markdown_file(&latin1, 1024).expect_err("not UTF-8");
    assert!(format!("{err:#}").contains("UTF-8"));

    let txt = tmp.file("nota.txt", b"hola")?;
    assert!(read_markdown_file(&txt, 1024).is_err());
    assert!(read_markdown_file(&tmp.0.join("no-existe.md"), 1024).is_err());
    Ok(())
}

#[test]
fn background_job_reads_every_file_and_reports_each_result() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let good = tmp.file("bien.md", b"# Bien\n\nhola")?;
    let bad = tmp.file("mal.md", b"\xff\xfe")?;
    let (done_tx, done_rx) = mpsc::channel();
    let job = ImportJob::spawn(vec![good.clone(), bad.clone()], move || {
        let _ = done_tx.send(());
    })?;
    done_rx.recv_timeout(Duration::from_secs(30))?;
    let results = job.poll().expect("finished");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, good);
    assert_eq!(
        results[0].1.as_ref().map(|p| p.title.as_str()).ok(),
        Some("Bien")
    );
    assert!(results[1].1.is_err());
    Ok(())
}

#[test]
fn page_with_blocks_is_created_atomically_and_searchable() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let blocks = vec![
        NewBlock {
            kind: BlockKind::Heading2,
            indent: 0,
            checked: false,
            content: "Sección".into(),
        },
        NewBlock {
            kind: BlockKind::Todo,
            indent: 9, // clamped to the maximum
            checked: true,
            content: "tarea importada".into(),
        },
        NewBlock {
            kind: BlockKind::Paragraph,
            indent: 0,
            checked: true, // only tasks keep checked
            content: "texto".into(),
        },
    ];
    let id = db.create_page_with_blocks("Importada", None, &blocks)?;
    let saved = db.list_blocks(&id)?;
    assert_eq!(saved.len(), 3);
    assert_eq!(saved[1].indent, silex_desktop::model::MAX_INDENT);
    assert!(saved[1].checked && !saved[2].checked);
    assert_eq!(db.search_pages("importada")?.len(), 1);

    // A failing insert (parent does not exist) leaves nothing behind.
    let before = db.list_pages()?.len();
    assert!(
        db.create_page_with_blocks("Huérfana", Some("no-existe"), &blocks)
            .is_err()
    );
    assert_eq!(db.list_pages()?.len(), before);
    assert!(db.search_pages("Huérfana")?.is_empty());
    Ok(())
}
