//! Attachments (ADR 0007): files stored in SQLite, used by `file` blocks.
use std::{fs, path::PathBuf};

use rusqlite::Connection;
use silex_desktop::attachment::{MAX_SIZE, NewAttachment};
use silex_desktop::db::{Database, backup};
use silex_desktop::model::{Block, BlockKind};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-adj-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn db(&self) -> PathBuf {
        self.0.join("silex.sqlite3")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pdf(name: &str) -> anyhow::Result<NewAttachment> {
    NewAttachment::from_bytes(name, b"%PDF-1.7\ncontenido".to_vec())
}

fn block(page: &str, kind: BlockKind, content: &str, attachment: Option<String>) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment,
    }
}

/// A page with a paragraph and a file block; returns (page, file block, attachment).
fn page_with_file(db: &mut Database) -> anyhow::Result<(String, Block, String)> {
    let page = db.create_page("Facturas", None)?;
    let attachment = db.add_attachment(&pdf("factura marzo.pdf")?)?;
    let file = block(
        &page,
        BlockKind::File,
        &attachment.name,
        Some(attachment.id.clone()),
    );
    let blocks = vec![
        block(&page, BlockKind::Paragraph, "Pagada", None),
        file.clone(),
    ];
    db.save_document(&page, "Facturas", &blocks)?;
    Ok((page, file, attachment.id))
}

#[test]
fn a_file_block_keeps_its_attachment_and_is_found_by_name() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (page, file, attachment) = page_with_file(&mut db)?;
    let blocks = db.list_blocks(&page)?;
    assert_eq!(
        blocks[1],
        Block {
            position: 1,
            ..file
        }
    );
    let info = db.attachment(&attachment)?.expect("stored");
    assert_eq!(info.name, "factura marzo.pdf");
    assert_eq!(info.mime, "application/pdf");
    assert_eq!(info.size, 18);
    assert_eq!(db.attachment_data(&attachment)?, b"%PDF-1.7\ncontenido");
    assert_eq!(db.page_attachments(&page)?, [info]);
    // Search finds it by its name, like any block.
    let hits = db.search("marzo")?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page.id, page);
    assert!(db.attachment_data("no-existe").is_err());
    assert!(db.attachment("no-existe")?.is_none());
    Ok(())
}

#[test]
fn blocks_that_do_not_match_their_attachment_are_refused() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (page, file, attachment) = page_with_file(&mut db)?;
    let before = db.list_blocks(&page)?;
    let attempts = [
        // A file block without its file.
        vec![block(&page, BlockKind::File, "x.pdf", None)],
        // A text block with a file.
        vec![block(
            &page,
            BlockKind::Paragraph,
            "x",
            Some(attachment.clone()),
        )],
        // A file that does not exist (foreign key).
        vec![block(
            &page,
            BlockKind::File,
            "x.pdf",
            Some("no-existe".into()),
        )],
        // Turning the file block into text.
        vec![Block {
            kind: BlockKind::Paragraph,
            ..file.clone()
        }],
    ];
    for blocks in attempts {
        assert!(
            db.save_document(&page, "Facturas", &blocks).is_err(),
            "{blocks:?}"
        );
        assert_eq!(db.list_blocks(&page)?, before, "nothing changed");
    }
    assert!(db.add_block(&page, BlockKind::File).is_err());
    let new = silex_desktop::model::NewBlock {
        kind: BlockKind::File,
        indent: 0,
        checked: false,
        content: "x".into(),
    };
    assert!(
        db.create_page_with_blocks("Importada", None, &[new])
            .is_err()
    );
    Ok(())
}

#[test]
fn an_attachment_outlives_its_block_and_cannot_be_deleted_while_used() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    let (page, file, attachment) = page_with_file(&mut db)?;

    // In use: the database refuses to delete it (ON DELETE RESTRICT).
    let conn = Connection::open(tmp.db())?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    assert!(
        conn.execute("DELETE FROM attachments WHERE id = ?1", [&attachment])
            .is_err()
    );
    // The size must match the data, and stay under the limit.
    assert!(
        conn.execute(
            "INSERT INTO attachments (id, name, mime, size, data) VALUES ('x', 'x', 'x', 5, x'00')",
            [],
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO attachments (id, name, mime, size, data) VALUES ('y', 'y', 'y', ?1, zeroblob(?1))",
            [MAX_SIZE as i64 + 1],
        )
        .is_err()
    );
    drop(conn);

    // Its page in the trash keeps it; deleting the block keeps it too (undo
    // can bring the block back).
    db.trash_page(&page)?;
    assert!(db.attachment(&attachment)?.is_some());
    db.restore_page(&page)?;
    db.save_document(&page, "Facturas", &[])?;
    assert!(db.attachment(&attachment)?.is_some());
    db.save_document(&page, "Facturas", &[file])?;
    assert_eq!(db.page_attachments(&page)?.len(), 1);
    Ok(())
}

#[test]
fn backups_carry_the_attachments() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let mut db = Database::open(tmp.db())?;
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let file = NewAttachment::from_bytes("datos.bin", data.clone())?;
    let attachment = db.add_attachment(&file)?;
    let page = db.create_page("Datos", None)?;
    db.save_document(
        &page,
        "Datos",
        &[block(
            &page,
            BlockKind::File,
            "datos.bin",
            Some(attachment.id.clone()),
        )],
    )?;
    let copy = backup::create_backup(&tmp.db(), &backup::backups_dir(&tmp.db()))?;
    let restored = Database::open(&copy)?;
    assert_eq!(restored.attachment_data(&attachment.id)?, data);
    Ok(())
}

#[test]
fn files_over_the_limit_are_refused_before_storing() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let file = NewAttachment {
        name: "enorme.bin".into(),
        mime: "application/octet-stream".into(),
        width: None,
        height: None,
        data: vec![0; MAX_SIZE as usize + 1],
    };
    let err = db.add_attachment(&file).expect_err("over the limit");
    assert!(format!("{err:#}").contains("20 MB"), "{err:#}");
    Ok(())
}

// ---- Markdown export ----

#[test]
fn file_blocks_export_as_links_next_to_their_page() -> anyhow::Result<()> {
    use silex_desktop::export::{export_page_with_files, export_workspace};
    let tmp = TempDir::new()?;
    let mut db = Database::open_in_memory()?;
    let (page, _, _) = page_with_file(&mut db)?;
    let png = NewAttachment::from_bytes("plano [v2]_*.png", b"\x89PNG\r\n\x1a\nresto".to_vec())?;
    let image = db.add_attachment(&png)?;
    let mut blocks = db.list_blocks(&page)?;
    blocks.push(block(
        &page,
        BlockKind::File,
        &image.name,
        Some(image.id.clone()),
    ));
    db.save_document(&page, "Facturas", &blocks)?;
    let blocks = db.list_blocks(&page)?;

    // A folder already named like the attachments folder is not touched.
    let out = tmp.0.join("Exportaciones");
    fs::create_dir_all(out.join("Facturas (adjuntos)"))?;
    fs::write(out.join("Facturas (adjuntos)").join("mío.txt"), "mío")?;
    let md = export_page_with_files(&db, &out, "Facturas", &blocks)?;
    assert_eq!(md, out.join("Facturas.md"));
    let text = fs::read_to_string(&md)?;
    assert!(
        text.contains("[factura marzo.pdf](<Facturas (adjuntos) 2/factura marzo.pdf>)"),
        "{text}"
    );
    // An image is an image, its name escaped; the path is a safe name.
    assert!(
        text.contains(r"![plano \[v2\]\_\*.png](<Facturas (adjuntos) 2/plano [v2]__.png>)"),
        "{text}"
    );
    let folder = out.join("Facturas (adjuntos) 2");
    assert_eq!(
        fs::read(folder.join("factura marzo.pdf"))?,
        b"%PDF-1.7\ncontenido"
    );
    assert_eq!(
        fs::read_to_string(out.join("Facturas (adjuntos)").join("mío.txt"))?,
        "mío"
    );
    // Exporting again never overwrites.
    let again = export_page_with_files(&db, &out, "Facturas", &blocks)?;
    assert_eq!(again, out.join("Facturas (2).md"));
    assert!(
        out.join("Facturas (2) (adjuntos)")
            .join("factura marzo.pdf")
            .exists()
    );

    // The whole workspace: next to the page, beside its subpages' folder.
    db.create_page("Marzo", Some(&page))?;
    let export = export_workspace(&db, &tmp.0.join("Todo"))?;
    let root = export.dir;
    assert!(root.join("Facturas.md").exists());
    assert!(root.join("Facturas").join("Marzo.md").exists());
    assert_eq!(
        fs::read(root.join("Facturas (adjuntos)").join("factura marzo.pdf"))?,
        b"%PDF-1.7\ncontenido"
    );
    let text = fs::read_to_string(root.join("Facturas.md"))?;
    assert!(
        text.contains("(<Facturas (adjuntos)/factura marzo.pdf>)"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_page_without_files_exports_as_before() -> anyhow::Result<()> {
    use silex_desktop::export::{export_page, export_page_with_files};
    let tmp = TempDir::new()?;
    let db = Database::open_in_memory()?;
    let blocks = vec![block("p", BlockKind::Paragraph, "Hola", None)];
    let with = export_page_with_files(&db, &tmp.0.join("a"), "Nota", &blocks)?;
    let without = export_page(&tmp.0.join("b"), "Nota", &blocks)?;
    assert_eq!(fs::read(with)?, fs::read(without)?);
    assert_eq!(fs::read_dir(tmp.0.join("a"))?.count(), 1, "no folder");
    Ok(())
}
