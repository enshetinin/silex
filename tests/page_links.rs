//! Links between pages (ADR 0008): a `page_link` block points at its page by
//! id, and its text follows the page's title.
use silex_desktop::db::Database;
use silex_desktop::model::{Block, BlockKind, PageKind};

fn block(page: &str, kind: BlockKind, content: &str, link: Option<&str>) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: link.map(str::to_owned),
        marks: Vec::new(),
    }
}

/// «Diario» links to «Recetas»; returns (diario, recetas, link block).
fn linked(db: &mut Database) -> anyhow::Result<(String, String, Block)> {
    let recetas = db.create_page("Recetas", None)?;
    let diario = db.create_page("Diario", None)?;
    // The text it is saved with does not matter: it becomes the title.
    let link = block(
        &diario,
        BlockKind::PageLink,
        "cualquier cosa",
        Some(&recetas),
    );
    db.save_document(
        &diario,
        "Diario",
        &[
            block(&diario, BlockKind::Paragraph, "Hoy cociné", None),
            link.clone(),
        ],
    )?;
    Ok((diario, recetas, link))
}

#[test]
fn a_link_points_at_its_page_and_reads_its_title() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, recetas, link) = linked(&mut db)?;
    let stored = db.list_blocks(&diario)?;
    assert_eq!(stored[1].kind, BlockKind::PageLink);
    assert_eq!(stored[1].link.as_deref(), Some(recetas.as_str()));
    assert_eq!(stored[1].content, "Recetas");
    assert_eq!(stored[1].id, link.id);
    // The page that links is found by the title it links to.
    let hits = db.search("recetas")?;
    assert!(hits.iter().any(|h| h.page.id == diario), "{hits:?}");
    Ok(())
}

#[test]
fn renaming_a_page_renames_the_links_to_it() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, recetas, link) = linked(&mut db)?;
    db.rename_page(&recetas, "Cocina")?;
    assert_eq!(db.list_blocks(&diario)?[1].content, "Cocina");
    assert!(db.search("cocina")?.iter().any(|h| h.page.id == diario));
    // Also when the title is edited in the page itself.
    db.save_document(&recetas, "Cocina vasca", &[])?;
    assert_eq!(db.list_blocks(&diario)?[1].content, "Cocina vasca");
    // A stale title in the open document is corrected when it is saved.
    let stale = Block {
        content: "Recetas".into(),
        ..link
    };
    db.save_document(&diario, "Diario", &[stale])?;
    assert_eq!(db.list_blocks(&diario)?[0].content, "Cocina vasca");
    Ok(())
}

#[test]
fn links_know_when_their_page_is_in_the_trash() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, recetas, _) = linked(&mut db)?;
    let base = db.create_database("Lecturas", None)?;
    let mut blocks = db.list_blocks(&diario)?;
    blocks.push(block(&diario, BlockKind::PageLink, "", Some(&base)));
    db.save_document(&diario, "Diario", &blocks)?;
    db.trash_page(&recetas)?;
    let mut targets = db.page_link_targets(&diario)?;
    targets.sort_by(|a, b| a.title.cmp(&b.title));
    let shown: Vec<(&str, PageKind, bool)> = targets
        .iter()
        .map(|t| (t.title.as_str(), t.kind, t.trashed))
        .collect();
    assert_eq!(
        shown,
        [
            ("Lecturas", PageKind::Database, false),
            ("Recetas", PageKind::Page, true)
        ]
    );
    // The link never broke: back from the trash, it works as before.
    db.restore_page(&recetas)?;
    assert!(db.page_link_targets(&diario)?.iter().all(|t| !t.trashed));
    Ok(())
}

#[test]
fn impossible_links_are_refused() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, _, _) = linked(&mut db)?;
    let before = db.list_blocks(&diario)?;
    // A text block with a link.
    let blocks = [block(&diario, BlockKind::Paragraph, "x", Some(&diario))];
    assert!(db.save_document(&diario, "Diario", &blocks).is_err());
    assert_eq!(db.list_blocks(&diario)?, before, "nothing changed");
    assert!(db.add_block(&diario, BlockKind::PageLink).is_err());
    Ok(())
}

/// Since pages can be deleted for good (ADR 0013), a link to a page that
/// no longer exists is saved pointing nowhere, not refused: the open
/// document (or its undo) may still hold the old id.
#[test]
fn a_link_to_a_page_that_is_gone_saves_pointing_nowhere() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, _, _) = linked(&mut db)?;
    let gone = block(&diario, BlockKind::PageLink, "Perdida", Some("no-existe"));
    db.save_document(&diario, "Diario", std::slice::from_ref(&gone))?;
    let stored = db.list_blocks(&diario)?;
    assert_eq!(
        (
            stored[0].kind,
            stored[0].link.clone(),
            stored[0].content.as_str()
        ),
        (BlockKind::PageLink, None, "Perdida")
    );
    Ok(())
}

// ---- Backlinks (H7.6) ----

#[test]
fn backlinks_list_the_live_pages_that_link_once_each() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (diario, recetas, _) = linked(&mut db)?;
    assert!(db.backlinks(&diario)?.is_empty(), "nobody links to it");

    // Two links from one page count once; titles in order.
    let agenda = db.create_page("Agenda", None)?;
    db.save_document(
        &agenda,
        "Agenda",
        &[
            block(&agenda, BlockKind::PageLink, "", Some(&recetas)),
            block(&agenda, BlockKind::PageLink, "", Some(&recetas)),
        ],
    )?;
    // A row of a database links too (rows are pages).
    let base = db.create_database("Lecturas", None)?;
    let fila = db.create_row(&base, "Zumos")?;
    db.save_document(
        &fila,
        "Zumos",
        &[block(&fila, BlockKind::PageLink, "", Some(&recetas))],
    )?;
    // A link to itself is not a backlink.
    db.save_document(
        &recetas,
        "Recetas",
        &[block(&recetas, BlockKind::PageLink, "", Some(&recetas))],
    )?;
    assert_eq!(
        db.backlinks(&recetas)?,
        [agenda.clone(), diario.clone(), fila]
    );

    // A page in the trash is not shown, and is again once restored; the
    // trashed database takes its row with it.
    db.trash_page(&diario)?;
    db.trash_page(&base)?;
    assert_eq!(db.backlinks(&recetas)?, std::slice::from_ref(&agenda));
    db.restore_page(&diario)?;
    assert_eq!(db.backlinks(&recetas)?, [agenda.clone(), diario.clone()]);

    // Removing the link removes the backlink.
    db.save_document(&agenda, "Agenda", &[])?;
    assert_eq!(db.backlinks(&recetas)?, [diario]);
    // A page that links nowhere, or does not exist, has none.
    assert!(db.backlinks("no-existe")?.is_empty());
    Ok(())
}

/// Manual: `cargo test --release --test page_links -- --ignored --nocapture`.
/// 1000 pages and 10 000 links; prints timings, never fails on speed.
#[test]
#[ignore = "scale smoke test; run manually"]
fn backlinks_scale_smoke() -> anyhow::Result<()> {
    use std::time::Instant;
    let mut db = Database::open_in_memory()?;
    let mut ids = Vec::new();
    for n in 0..1000 {
        ids.push(db.create_page(&format!("Página {n}"), None)?);
    }
    let start = Instant::now();
    for (n, page) in ids.iter().enumerate() {
        // 10 links per page; page 0 is linked from every page.
        let blocks: Vec<Block> = (0..10)
            .map(|k| {
                let target = if k == 0 { 0 } else { (n * 7 + k * 131) % 1000 };
                block(page, BlockKind::PageLink, "", Some(&ids[target]))
            })
            .collect();
        db.save_document(page, &format!("Página {n}"), &blocks)?;
    }
    println!("guardar 10 000 enlaces: {:?}", start.elapsed());

    let t = Instant::now();
    let popular = db.backlinks(&ids[0])?;
    println!(
        "backlinks de la página más enlazada ({}): {:?}",
        popular.len(),
        t.elapsed()
    );
    assert_eq!(popular.len(), 999);

    let t = Instant::now();
    let mut total = 0;
    for id in &ids[1..101] {
        total += db.backlinks(id)?.len();
    }
    println!(
        "backlinks de 100 páginas ({total} en total): {:?}",
        t.elapsed()
    );

    let t = Instant::now();
    db.rename_page(&ids[0], "Portada")?;
    println!("renombrar la página más enlazada: {:?}", t.elapsed());
    Ok(())
}

// ---- Markdown export ----

#[test]
fn relative_links_climb_out_of_folders() {
    use silex_desktop::export::relative_link;
    use std::path::Path;
    assert_eq!(
        relative_link(Path::new(""), Path::new("Diario.md")),
        "Diario.md"
    );
    assert_eq!(
        relative_link(Path::new(""), Path::new("Recetas/Pan.md")),
        "Recetas/Pan.md"
    );
    assert_eq!(
        relative_link(Path::new("Recetas"), Path::new("Diario.md")),
        "../Diario.md"
    );
    assert_eq!(
        relative_link(Path::new("A/B"), Path::new("A/C/D.md")),
        "../C/D.md"
    );
}

#[test]
fn links_export_as_relative_links_in_the_workspace_and_text_alone() -> anyhow::Result<()> {
    use silex_desktop::export::{export_page_with_files, export_workspace};
    let tmp = std::env::temp_dir().join(format!("silex-links-{}", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let mut db = Database::open_in_memory()?;
        let recetas = db.create_page("Recetas", None)?;
        let pan = db.create_page("Pan [de] centeno", Some(&recetas))?;
        let diario = db.create_page("Diario", None)?;
        let borrada = db.create_page("Borrada", None)?;
        db.save_document(
            &diario,
            "Diario",
            &[
                block(&diario, BlockKind::PageLink, "", Some(&pan)),
                block(&diario, BlockKind::PageLink, "", Some(&recetas)),
                block(&diario, BlockKind::PageLink, "", Some(&borrada)),
            ],
        )?;
        db.save_document(
            &pan,
            "Pan [de] centeno",
            &[block(&pan, BlockKind::PageLink, "", Some(&diario))],
        )?;
        db.trash_page(&borrada)?;

        let export = export_workspace(&db, &tmp.join("todo"))?;
        let diario_md = std::fs::read_to_string(export.dir.join("Diario.md"))?;
        assert!(
            diario_md.contains(r"[Pan \[de\] centeno](<Recetas/Pan [de] centeno.md>)"),
            "{diario_md}"
        );
        assert!(diario_md.contains("[Recetas](<Recetas.md>)"), "{diario_md}");
        // Not exported (in the trash): its title, as text.
        assert!(diario_md.contains("\nBorrada\n"), "{diario_md}");
        let pan_md =
            std::fs::read_to_string(export.dir.join("Recetas").join("Pan [de] centeno.md"))?;
        assert!(pan_md.contains("[Diario](<../Diario.md>)"), "{pan_md}");

        // A page alone: its links are just titles.
        let blocks = db.list_blocks(&diario)?;
        let alone = export_page_with_files(&db, &tmp.join("sola"), "Diario", None, &blocks)?;
        let text = std::fs::read_to_string(alone)?;
        assert!(text.contains(r"Pan \[de\] centeno"), "{text}");
        assert!(!text.contains("](<"), "{text}");
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[test]
fn graph_only_contains_live_links() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (source, target, _) = linked(&mut db)?;
    assert_eq!(db.graph_edges()?, vec![(source, target.clone())]);
    db.trash_page(&target)?;
    assert!(db.graph_edges()?.is_empty());
    Ok(())
}
