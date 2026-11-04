//! Page templates (ADR 0010): a template is a page marked with
//! `template_at`, left out of the tree, favorites, search and backlinks,
//! and copied to make new pages.
use silex_desktop::attachment::NewAttachment;
use silex_desktop::db::{Database, Shift};
use silex_desktop::model::{Block, BlockKind, NewBlock};

fn block(page: &str, kind: BlockKind, content: &str) -> Block {
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
        marks: Vec::new(),
    }
}

/// «Acta {{fecha}}» with a heading, a checked to-do, a file and a link to
/// «Equipo», saved as a template. Returns (db, page, template, equipo,
/// attachment id).
fn acta() -> anyhow::Result<(Database, String, String, String, String)> {
    let mut db = Database::open_in_memory()?;
    let equipo = db.create_page("Equipo", None)?;
    let page = db.create_page("Acta {{fecha}}", None)?;
    let file = db.add_attachment(&NewAttachment::from_bytes(
        "orden del día.pdf",
        b"%PDF-1.7 orden".to_vec(),
    )?)?;
    let blocks = vec![
        block(&page, BlockKind::Heading1, "Reunión del {{fecha}}"),
        Block {
            checked: true,
            indent: 1,
            ..block(&page, BlockKind::Todo, "Repasar {{fecha}}")
        },
        Block {
            attachment: Some(file.id.clone()),
            ..block(&page, BlockKind::File, "orden del día.pdf")
        },
        Block {
            link: Some(equipo.clone()),
            ..block(&page, BlockKind::PageLink, "")
        },
    ];
    db.save_document(&page, "Acta {{fecha}}", &blocks)?;
    let template = db.save_as_template(&page)?;
    Ok((db, page, template, equipo, file.id))
}

#[test]
fn saving_as_a_template_copies_the_page_and_leaves_it_as_it_was() -> anyhow::Result<()> {
    let (db, page, template, _, _) = acta()?;
    assert_ne!(page, template);
    assert!(db.is_template(&template)?);
    assert!(!db.is_template(&page)?);
    let templates = db.list_templates()?;
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].title, "Acta {{fecha}}");
    assert_eq!(templates[0].parent_id, None);
    // The same blocks with new ids; the page keeps its own.
    let original = db.list_blocks(&page)?;
    let copied = db.list_blocks(&template)?;
    let shape = |b: &Block| {
        (
            b.kind,
            b.content.clone(),
            b.checked,
            b.indent,
            b.attachment.clone(),
            b.link.clone(),
        )
    };
    assert_eq!(
        copied.iter().map(shape).collect::<Vec<_>>(),
        original.iter().map(shape).collect::<Vec<_>>()
    );
    assert!(copied.iter().all(|b| original.iter().all(|o| o.id != b.id)));
    // The template's title stays «{{fecha}}»: it is filled when used.
    assert_eq!(copied[0].content, "Reunión del {{fecha}}");
    Ok(())
}

#[test]
fn a_page_from_a_template_fills_the_date_and_shares_files_and_links() -> anyhow::Result<()> {
    let (mut db, _, template, equipo, file) = acta()?;
    let parent = db.create_page("Reuniones", None)?;
    let new = db.create_from_template(&template, Some(&parent), "2026-10-20")?;
    let page = db.get_page(&new)?.expect("a live page");
    assert_eq!(page.title, "Acta 20 oct 2026");
    assert_eq!(page.parent_id.as_deref(), Some(parent.as_str()));
    assert!(!db.is_template(&new)?);
    let blocks = db.list_blocks(&new)?;
    let texts: Vec<&str> = blocks.iter().map(|b| b.content.as_str()).collect();
    // A link's text is its page's title, never a date.
    assert_eq!(
        texts,
        [
            "Reunión del 20 oct 2026",
            "Repasar 20 oct 2026",
            "orden del día.pdf",
            "Equipo"
        ]
    );
    assert!(blocks[1].checked && blocks[1].indent == 1);
    assert_eq!(blocks[2].attachment.as_deref(), Some(file.as_str()));
    assert_eq!(blocks[3].link.as_deref(), Some(equipo.as_str()));
    // The template is unchanged, and found again for the next page.
    assert_eq!(
        db.list_blocks(&template)?[0].content,
        "Reunión del {{fecha}}"
    );
    let root = db.create_from_template(&template, None, "2026-10-21")?;
    assert_eq!(
        db.get_page(&root)?.map(|p| p.title).as_deref(),
        Some("Acta 21 oct 2026")
    );
    // The page is in the tree, last among its siblings.
    let pages = db.list_pages()?;
    let roots: Vec<&str> = pages
        .iter()
        .filter(|p| p.parent_id.is_none())
        .map(|p| p.title.as_str())
        .collect();
    assert_eq!(roots.last(), Some(&"Acta 21 oct 2026"));
    Ok(())
}

#[test]
fn templates_are_not_in_the_tree_favorites_search_or_backlinks() -> anyhow::Result<()> {
    let (mut db, page, template, equipo, _) = acta()?;
    assert!(db.list_pages()?.iter().all(|p| p.id != template));
    // Search: by title and by text, the page is found, the template is not.
    for query in ["acta", "reunión", "repasar"] {
        let hits = db.search(query)?;
        assert!(hits.iter().any(|h| h.page.id == page), "{query}");
        assert!(hits.iter().all(|h| h.page.id != template), "{query}");
        assert!(
            db.search_pages(query)?.iter().all(|p| p.id != template),
            "{query}"
        );
    }
    assert!(db.search_pages("")?.iter().all(|p| p.id != template));
    // Backlinks: the template's link to «Equipo» does not count.
    assert_eq!(db.backlinks(&equipo)?, [page]);
    // Favorites: a template cannot be one.
    assert!(db.set_favorite(&template, true).is_err());
    assert!(db.list_favorites()?.is_empty());
    // In the trash and back, still a template.
    db.trash_page(&template)?;
    assert!(db.list_templates()?.is_empty());
    assert!(db.list_trash()?.iter().any(|t| t.page.id == template));
    db.restore_page(&template)?;
    assert_eq!(db.list_templates()?.len(), 1);
    assert!(db.list_pages()?.iter().all(|p| p.id != template));
    // A template in the trash cannot be used.
    db.trash_page(&template)?;
    assert!(
        db.create_from_template(&template, None, "2026-10-20")
            .is_err()
    );
    Ok(())
}

#[test]
fn nothing_goes_inside_a_template_and_it_never_moves() -> anyhow::Result<()> {
    let (mut db, page, template, _, _) = acta()?;
    assert!(db.create_page("Hija", Some(&template)).is_err());
    assert!(db.create_database("Base", Some(&template)).is_err());
    let text = [NewBlock {
        kind: BlockKind::Paragraph,
        indent: 0,
        checked: false,
        content: "importada".into(),
        marks: Vec::new(),
    }];
    assert!(
        db.create_page_with_blocks("Importada", Some(&template), &text)
            .is_err()
    );
    assert!(db.move_page(&page, Some(&template)).is_err());
    assert!(db.move_page(&template, Some(&page)).is_err());
    assert!(db.move_page(&template, None).is_err());
    let other = db.save_as_template(&page)?;
    assert!(
        db.create_from_template(&other, Some(&template), "2026-10-20")
            .is_err()
    );
    // Not from a database, a row, or a template; not into a database.
    let base = db.create_database("Lecturas", None)?;
    let row = db.create_row(&base, "Rayuela")?;
    assert!(db.save_as_template(&base).is_err());
    assert!(db.save_as_template(&row).is_err());
    assert!(db.save_as_template(&template).is_err());
    assert!(
        db.create_from_template(&template, Some(&base), "2026-10-20")
            .is_err()
    );
    assert!(
        db.list_pages()?
            .iter()
            .all(|p| p.parent_id.as_deref() != Some(template.as_str()))
    );
    Ok(())
}

#[test]
fn templates_are_ordered_among_templates_and_pages_among_pages() -> anyhow::Result<()> {
    let (mut db, page, _, equipo, _) = acta()?;
    let second = db.save_as_template(&equipo)?;
    let titles = |db: &Database| -> anyhow::Result<Vec<String>> {
        Ok(db.list_templates()?.into_iter().map(|p| p.title).collect())
    };
    assert_eq!(titles(&db)?, ["Acta {{fecha}}", "Equipo"]);
    assert!(db.reorder_page(&second, Shift::Up)?);
    assert_eq!(titles(&db)?, ["Equipo", "Acta {{fecha}}"]);
    assert!(
        !db.reorder_page(&second, Shift::Up)?,
        "first among templates"
    );
    // Pages ignore templates when reordering: «Acta» is the last page.
    assert!(!db.reorder_page(&page, Shift::Down)?);
    assert!(db.reorder_page(&page, Shift::Up)?);
    let roots: Vec<String> = db.list_pages()?.into_iter().map(|p| p.title).collect();
    assert_eq!(roots, ["Acta {{fecha}}", "Equipo"]);
    Ok(())
}

#[test]
fn the_examples_are_created_once_and_never_come_back() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    assert!(db.create_example_templates()?);
    let templates = db.list_templates()?;
    let titles: Vec<&str> = templates.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Reunión {{fecha}}", "Diario {{fecha}}", "Lista de tareas"]
    );
    assert!(
        templates
            .iter()
            .all(|t| !db.list_blocks(&t.id).unwrap_or_default().is_empty())
    );
    assert!(!db.create_example_templates()?, "only once");
    for template in &templates {
        db.trash_page(&template.id)?;
    }
    assert!(
        !db.create_example_templates()?,
        "not even after trashing them"
    );
    assert!(db.list_templates()?.is_empty());
    // The mark does not disturb the settings.
    assert_eq!(
        db.load_settings()?,
        silex_desktop::settings::Settings::default()
    );
    Ok(())
}

#[test]
fn the_workspace_export_puts_templates_in_their_folder() -> anyhow::Result<()> {
    use silex_desktop::export::export_workspace;
    let tmp = std::env::temp_dir().join(format!("silex-templates-{}", uuid::Uuid::new_v4()));
    struct Clean(std::path::PathBuf);
    impl Drop for Clean {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _clean = Clean(tmp.clone());
    let (db, _, _, _, _) = acta()?;
    // A page already called «Plantillas» keeps its name.
    db.create_page("Plantillas", None)?;
    let export = export_workspace(&db, &tmp)?;
    let root = export.dir;
    assert!(root.join("Plantillas.md").exists());
    let folder = root.join("Plantillas (2)");
    let text = std::fs::read_to_string(folder.join("Acta {{fecha}}.md"))?;
    assert!(text.contains("orden del día.pdf"), "{text}");
    assert!(
        folder
            .join("Acta {{fecha}} (adjuntos)")
            .join("orden del día.pdf")
            .exists()
    );
    // Pages (4) and the template (1).
    assert_eq!(export.pages, 4);
    Ok(())
}
