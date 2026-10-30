//! Page icons in the database (H12.8).
use silex_desktop::db::Database;

#[test]
fn an_icon_is_one_emoji_and_can_be_removed() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Recetas", None)?;
    db.set_page_icon(&page, Some("🍞"))?;
    assert_eq!(db.page_icons()?.get(&page).map(String::as_str), Some("🍞"));
    for bad in ["", "pan", "🍞🍞", "🍞 "] {
        assert!(db.set_page_icon(&page, Some(bad)).is_err(), "{bad:?}");
    }
    assert_eq!(db.page_icons()?.get(&page).map(String::as_str), Some("🍞"));
    db.set_page_icon(&page, None)?;
    assert!(db.page_icons()?.is_empty());
    assert!(db.set_page_icon("no-existe", Some("🍞")).is_err());
    Ok(())
}

#[test]
fn duplicates_and_templates_keep_the_icon() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Diario", None)?;
    db.set_page_icon(&page, Some("📒"))?;
    let copy = db.duplicate_page(&page, false)?;
    let template = db.save_as_template(&page)?;
    let from_template = db.create_from_template(&template, None, "2026-10-30")?;
    let icons = db.page_icons()?;
    for id in [&copy, &template, &from_template] {
        assert_eq!(icons.get(id).map(String::as_str), Some("📒"), "{id}");
    }
    Ok(())
}
