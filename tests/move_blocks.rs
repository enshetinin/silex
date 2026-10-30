//! Moving blocks between pages in the database (H12.6a, ADR 0017).
use silex_desktop::db::Database;
use silex_desktop::model::{Block, BlockKind};

fn block(page: &str, id: &str, content: &str) -> Block {
    Block {
        id: id.into(),
        page_id: page.into(),
        kind: BlockKind::Paragraph,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks: Vec::new(),
    }
}

fn contents(db: &Database, page: &str) -> anyhow::Result<Vec<String>> {
    Ok(db
        .list_blocks(page)?
        .into_iter()
        .map(|b| b.content)
        .collect())
}

#[test]
fn blocks_move_to_the_end_of_the_other_page_keeping_their_ids() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let a = db.create_page("Origen", None)?;
    let b = db.create_page("Destino", None)?;
    db.save_document(
        &a,
        "Origen",
        &[
            block(&a, "1", "uno"),
            block(&a, "2", "dos 🌾"),
            block(&a, "3", "tres"),
        ],
    )?;
    db.save_document(&b, "Destino", &[block(&b, "x", "ya estaba")])?;
    // In the order given, «missing» skipped.
    let from = db.move_blocks(&["3".into(), "missing".into(), "1".into()], &b)?;
    assert_eq!(from, std::slice::from_ref(&a));
    assert_eq!(contents(&db, &a)?, ["dos 🌾"]);
    assert_eq!(contents(&db, &b)?, ["ya estaba", "tres", "uno"]);
    let moved = db.blocks_by_ids(&["1".into(), "nope".into()])?;
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].page_id, b);
    // The search finds them in their new page.
    let hits = db.search_pages("tres")?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, b);
    // Saving the origin without them does not delete them.
    db.save_document(&a, "Origen", &[block(&a, "2", "dos 🌾")])?;
    assert_eq!(contents(&db, &b)?, ["ya estaba", "tres", "uno"]);
    Ok(())
}

#[test]
fn blocks_never_go_to_a_database_or_a_page_that_does_not_exist() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let a = db.create_page("Origen", None)?;
    db.save_document(&a, "Origen", &[block(&a, "1", "uno")])?;
    let table = db.create_database("Tabla", None)?;
    assert!(db.move_blocks(&["1".into()], &table).is_err());
    assert!(db.move_blocks(&["1".into()], "no-existe").is_err());
    assert_eq!(contents(&db, &a)?, ["uno"], "nothing moved");
    Ok(())
}
