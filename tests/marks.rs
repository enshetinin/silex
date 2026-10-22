//! Inline formatting (ADR 0015): marks are saved with their blocks, read
//! back clean, follow copies, templates and versions, and damaged ones
//! never cost the text.
use std::{fs, path::PathBuf};

use rusqlite::Connection;
use silex_desktop::db::Database;
use silex_desktop::marks::{Mark, MarkKind};
use silex_desktop::model::{Block, BlockKind};

fn block(page: &str, kind: BlockKind, content: &str, marks: Vec<Mark>) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks,
    }
}

fn bold(start: usize, end: usize) -> Mark {
    Mark::new(start, end, MarkKind::Bold)
}

fn link(start: usize, end: usize, url: &str) -> Mark {
    Mark::new(start, end, MarkKind::Link(url.into()))
}

/// The characters a mark covers.
fn covered(block: &Block, mark: &Mark) -> String {
    block
        .content
        .chars()
        .skip(mark.start)
        .take(mark.end - mark.start)
        .collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-marks-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn formatting_is_saved_and_read_back() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Notas", None)?;
    let marks = vec![
        bold(0, 4),
        Mark::new(2, 9, MarkKind::Italic),
        link(10, 15, "https://ejemplo.com"),
    ];
    let saved = block(
        &page,
        BlockKind::Paragraph,
        "café 🌾 y pan rico",
        marks.clone(),
    );
    let plain = block(&page, BlockKind::Quote, "sin formato", Vec::new());
    db.save_document(&page, "Notas", &[saved.clone(), plain.clone()])?;
    let read = db.list_blocks(&page)?;
    assert_eq!(read[0].marks, marks);
    assert_eq!(covered(&read[0], &read[0].marks[0]), "café");
    assert!(read[1].marks.is_empty());

    // Only the formatting changes: it is saved, and search is unaffected.
    let changed = Block {
        marks: vec![bold(5, 6)],
        ..read[0].clone()
    };
    db.save_document(&page, "Notas", &[changed, read[1].clone()])?;
    assert_eq!(db.list_blocks(&page)?[0].marks, vec![bold(5, 6)]);
    assert_eq!(db.search_pages("rico")?.len(), 1);

    // Removing it stores NULL again.
    let cleared = Block {
        marks: Vec::new(),
        ..db.list_blocks(&page)?[0].clone()
    };
    db.save_document(&page, "Notas", &[cleared])?;
    assert!(db.list_blocks(&page)?[0].marks.is_empty());
    Ok(())
}

#[test]
fn formatting_is_stored_clean() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Notas", None)?;
    let messy = block(
        &page,
        BlockKind::Paragraph,
        "hola",
        vec![
            bold(2, 3),
            bold(0, 2),
            bold(3, 99),
            Mark::new(1, 1, MarkKind::Italic),
            link(0, 2, "javascript:alert(1)"),
        ],
    );
    let code = block(&page, BlockKind::Code, "let x = 1;", vec![bold(0, 3)]);
    db.save_document(&page, "Notas", &[messy, code])?;
    let read = db.list_blocks(&page)?;
    assert_eq!(read[0].marks, vec![bold(0, 4)]);
    assert!(read[1].marks.is_empty(), "code blocks take no formatting");
    Ok(())
}

#[test]
fn damaged_formatting_keeps_the_text() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let path = tmp.0.join("silex.sqlite3");
    let mut db = Database::open(&path)?;
    let page = db.create_page("Notas", None)?;
    let blocks = [
        block(&page, BlockKind::Paragraph, "uno", vec![bold(0, 3)]),
        block(&page, BlockKind::Paragraph, "dos", vec![bold(0, 3)]),
        block(&page, BlockKind::Paragraph, "tres", vec![bold(0, 4)]),
    ];
    db.save_document(&page, "Notas", &blocks)?;
    drop(db);

    let conn = Connection::open(&path)?;
    // Valid JSON that does not fit the text, and an unknown type.
    conn.execute(
        "UPDATE blocks SET marks = ?2 WHERE id = ?1",
        [
            blocks[0].id.as_str(),
            r#"[{"start":0,"end":50,"type":"bold"}]"#,
        ],
    )?;
    conn.execute(
        "UPDATE blocks SET marks = ?2 WHERE id = ?1",
        [
            blocks[1].id.as_str(),
            r#"[{"start":0,"end":1,"type":"color"}]"#,
        ],
    )?;
    // Not JSON at all is refused by the CHECK.
    assert!(
        conn.execute(
            "UPDATE blocks SET marks = 'no' WHERE id = ?1",
            [&blocks[2].id],
        )
        .is_err()
    );
    drop(conn);

    let mut db = Database::open(&path)?;
    let read = db.list_blocks(&page)?;
    assert_eq!(
        read.iter().map(|b| b.content.as_str()).collect::<Vec<_>>(),
        ["uno", "dos", "tres"]
    );
    assert!(read[0].marks.is_empty());
    assert!(read[1].marks.is_empty());
    assert_eq!(read[2].marks, vec![bold(0, 4)]);
    // The page still saves.
    db.save_document(&page, "Notas", &read)?;
    Ok(())
}

#[test]
fn copies_and_templates_keep_the_formatting_on_the_same_words() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Reunión", None)?;
    // «{{fecha}}» becomes a longer date: «hoy» must stay bold.
    let text = "Acta {{fecha}}: hoy";
    let marks = vec![bold(16, 19), link(0, 4, "https://acta.es")];
    db.save_document(
        &page,
        "Reunión",
        &[block(&page, BlockKind::Heading2, text, marks.clone())],
    )?;

    let copy = db.duplicate_page(&page, false)?;
    assert_eq!(
        db.list_blocks(&copy)?[0].marks,
        db.list_blocks(&page)?[0].marks
    );

    let template = db.save_as_template(&page)?;
    assert_eq!(db.list_blocks(&template)?[0].content, text);
    let new = db.create_from_template(&template, None, "2026-10-22")?;
    let filled = &db.list_blocks(&new)?[0];
    assert_eq!(filled.content, "Acta 22 oct 2026: hoy");
    let words: Vec<String> = filled.marks.iter().map(|m| covered(filled, m)).collect();
    assert_eq!(words, ["Acta", "hoy"]);
    Ok(())
}

#[test]
fn versions_keep_and_restore_the_formatting() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Diario", None)?;
    let first = block(&page, BlockKind::Paragraph, "uno dos", vec![bold(0, 3)]);
    db.set_clock(Some("2026-10-22T10:00:00.000Z"));
    db.save_document(&page, "Diario", std::slice::from_ref(&first))?;
    db.set_clock(Some("2026-10-22T10:30:00.000Z"));
    let edited = Block {
        content: "uno dos tres".into(),
        marks: Vec::new(),
        ..first.clone()
    };
    db.save_document(&page, "Diario", &[edited])?;

    let versions = db.page_versions(&page)?;
    assert_eq!(versions.len(), 1);
    let old = db
        .version_blocks(versions[0].id)?
        .expect("exists")
        .expect("readable");
    assert_eq!(old[0].marks, vec![bold(0, 3)]);
    let restored = db.restorable_blocks(&page, old)?;
    db.save_document(&page, "Diario", &restored)?;
    assert_eq!(db.list_blocks(&page)?[0].marks, vec![bold(0, 3)]);
    Ok(())
}

#[test]
fn a_version_without_formatting_stays_format_1() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let path = tmp.0.join("silex.sqlite3");
    let mut db = Database::open(&path)?;
    let page = db.create_page("Diario", None)?;
    let plain = block(&page, BlockKind::Paragraph, "uno", Vec::new());
    db.set_clock(Some("2026-10-22T10:00:00.000Z"));
    db.save_document(&page, "Diario", std::slice::from_ref(&plain))?;
    db.set_clock(Some("2026-10-22T10:30:00.000Z"));
    let marked = Block {
        marks: vec![bold(0, 3)],
        ..plain.clone()
    };
    db.save_document(&page, "Diario", &[marked])?;
    db.set_clock(Some("2026-10-22T11:00:00.000Z"));
    db.save_document(&page, "Diario", &[plain])?;
    drop(db);
    let conn = Connection::open(&path)?;
    let formats: Vec<i64> = conn
        .prepare("SELECT format FROM page_versions ORDER BY created_at")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    assert_eq!(formats, [1, 2]);
    Ok(())
}
