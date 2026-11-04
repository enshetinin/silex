//! Daily notes in the database (H12.9).
use silex_desktop::db::Database;
use silex_desktop::model::BlockKind;

/// The titles of the live children of `parent`, in their order.
fn children(db: &Database, parent: &str) -> anyhow::Result<Vec<String>> {
    Ok(db
        .list_pages()?
        .into_iter()
        .filter(|p| p.parent_id.as_deref() == Some(parent))
        .map(|p| p.title)
        .collect())
}

#[test]
fn a_day_is_found_or_made_by_month_the_latest_on_top() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let first = db.journal_day(None, "2026-10-30", None)?;
    assert!(first.created);
    let root = db.get_page(&first.root)?.expect("root");
    assert_eq!(root.title, "Diario");
    assert_eq!(
        db.page_icons()?.get(&first.root).map(String::as_str),
        Some("📓")
    );
    assert_eq!(children(&db, &first.root)?, ["2026-10"]);
    // Again: the same page.
    let again = db.journal_day(Some(&first.root), "2026-10-30", None)?;
    assert_eq!(
        (again.page.as_str(), again.created),
        (first.page.as_str(), false)
    );
    // The next day and month go first.
    let next = db.journal_day(Some(&first.root), "2026-10-31", None)?;
    let month = db
        .get_page(&next.page)?
        .and_then(|p| p.parent_id)
        .expect("month");
    assert_eq!(children(&db, &month)?, ["2026-10-31", "2026-10-30"]);
    db.journal_day(Some(&first.root), "2026-11-01", None)?;
    assert_eq!(children(&db, &first.root)?, ["2026-11", "2026-10"]);
    Ok(())
}

#[test]
fn a_renamed_journal_is_kept_and_a_trashed_one_is_replaced() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let first = db.journal_day(None, "2026-10-30", None)?;
    db.rename_page(&first.root, "Mi diario")?;
    let same = db.journal_day(Some(&first.root), "2026-10-30", None)?;
    assert_eq!(same.root, first.root);
    assert!(!same.created);
    db.trash_page(&first.root)?;
    let fresh = db.journal_day(Some(&first.root), "2026-10-30", None)?;
    assert_ne!(fresh.root, first.root);
    assert!(fresh.created);
    // A page that is not a page (a database) is not a journal either.
    let table = db.create_database("Tabla", None)?;
    assert_ne!(
        db.journal_day(Some(&table), "2026-10-30", None)?.root,
        table
    );
    assert!(db.journal_day(None, "30/10/2026", None).is_err());
    Ok(())
}

#[test]
fn a_new_day_comes_from_the_journal_template() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let model = db.create_page("Modelo", None)?;
    db.add_block(&model, BlockKind::Heading2)?;
    let mut blocks = db.list_blocks(&model)?;
    blocks[0].content = "Hoy, {{fecha}}".into();
    db.save_document(&model, "Modelo", &blocks)?;
    db.set_page_icon(&model, Some("☀️"))?;
    let template = db.save_as_template(&model)?;
    // A page that is not a template cannot be the journal's template.
    assert!(db.set_journal_template(Some(&model)).is_err());
    db.set_journal_template(Some(&template))?;
    let (root, chosen) = db.journal_settings()?;
    assert_eq!((root, chosen.as_deref()), (None, Some(template.as_str())));
    let day = db.journal_day(None, "2026-10-30", chosen.as_deref())?;
    db.set_journal_page(&day.root)?;
    let made = db.list_blocks(&day.page)?;
    assert_eq!(made[0].kind, BlockKind::Heading2);
    assert!(
        !made[0].content.contains("{{fecha}}"),
        "{}",
        made[0].content
    );
    assert!(made[0].content.contains("2026"), "{}", made[0].content);
    assert_eq!(
        db.get_page(&day.page)?.map(|p| p.title).as_deref(),
        Some("2026-10-30")
    );
    assert_eq!(
        db.page_icons()?.get(&day.page).map(String::as_str),
        Some("☀️")
    );
    assert_eq!(db.journal_settings()?.0.as_deref(), Some(day.root.as_str()));
    db.set_journal_template(None)?;
    assert_eq!(db.journal_settings()?.1, None);
    // A trashed template: the next new day is empty.
    db.set_journal_template(Some(&template))?;
    db.trash_page(&template)?;
    let next = db.journal_day(Some(&day.root), "2026-10-31", Some(&template))?;
    assert!(db.list_blocks(&next.page)?.is_empty());
    Ok(())
}
