use std::collections::HashSet;

use silex_desktop::model::{Page, ancestors, build_tree, descendants};

fn page(id: &str, parent: Option<&str>) -> Page {
    Page {
        id: id.into(),
        parent_id: parent.map(Into::into),
        title: id.to_uppercase(),
    }
}

fn expanded(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_owned()).collect()
}

fn rows(pages: &[Page], open: &[&str]) -> Vec<(String, usize, bool)> {
    build_tree(pages, &expanded(open))
        .into_iter()
        .map(|r| (r.page.id, r.depth, r.has_children))
        .collect()
}

fn r(id: &str, depth: usize, has_children: bool) -> (String, usize, bool) {
    (id.into(), depth, has_children)
}

/// a ─ a1 ─ a1x
///   └ a2
/// b
fn sample() -> Vec<Page> {
    vec![
        page("a", None),
        page("a1", Some("a")),
        page("b", None),
        page("a1x", Some("a1")),
        page("a2", Some("a")),
    ]
}

#[test]
fn collapsed_tree_shows_only_roots() {
    assert_eq!(
        rows(&sample(), &[]),
        vec![r("a", 0, true), r("b", 0, false)]
    );
}

#[test]
fn expanded_tree_nests_in_sibling_order() {
    assert_eq!(
        rows(&sample(), &["a", "a1"]),
        vec![
            r("a", 0, true),
            r("a1", 1, true),
            r("a1x", 2, false),
            r("a2", 1, false),
            r("b", 0, false),
        ]
    );
}

#[test]
fn collapsing_a_parent_hides_all_its_descendants() {
    // a1 is "expanded" but a is not: nothing under a is visible.
    assert_eq!(
        rows(&sample(), &["a1"]),
        vec![r("a", 0, true), r("b", 0, false)]
    );
}

#[test]
fn page_with_missing_parent_is_shown_at_root() {
    let pages = vec![page("a", None), page("huerfana", Some("en-la-papelera"))];
    assert_eq!(
        rows(&pages, &[]),
        vec![r("a", 0, false), r("huerfana", 0, false)]
    );
}

#[test]
fn stored_cycle_does_not_loop_or_lose_pages() {
    let pages = vec![page("x", Some("y")), page("y", Some("x")), page("z", None)];
    let shown = rows(&pages, &["x", "y"]);
    let ids: HashSet<String> = shown.iter().map(|(id, _, _)| id.clone()).collect();
    assert_eq!(ids, expanded(&["x", "y", "z"]));
    assert_eq!(shown.len(), 3, "each page exactly once");
}

#[test]
fn descendants_and_ancestors() {
    let pages = sample();
    assert_eq!(descendants(&pages, "a"), expanded(&["a1", "a1x", "a2"]));
    assert!(descendants(&pages, "b").is_empty());
    assert_eq!(ancestors(&pages, "a1x"), vec!["a1", "a"]);
    assert!(ancestors(&pages, "a").is_empty());

    let cyclic = vec![page("x", Some("y")), page("y", Some("x"))];
    assert_eq!(descendants(&cyclic, "x"), expanded(&["y"]));
    assert_eq!(ancestors(&cyclic, "x"), vec!["y"]);
}

mod moves {
    use silex_desktop::db::Database;
    use silex_desktop::model::Page;

    /// Notas ─ Viaje ─ Islandia ; Trabajo
    fn tree(db: &Database) -> anyhow::Result<[String; 4]> {
        let notas = db.create_page("Notas", None)?;
        let viaje = db.create_page("Viaje", Some(&notas))?;
        let islandia = db.create_page("Islandia", Some(&viaje))?;
        let trabajo = db.create_page("Trabajo", None)?;
        Ok([notas, viaje, islandia, trabajo])
    }

    fn parent_of(db: &Database, id: &str) -> anyhow::Result<Option<String>> {
        Ok(db.get_page(id)?.and_then(|p: Page| p.parent_id))
    }

    #[test]
    fn move_to_other_parent_and_to_root() -> anyhow::Result<()> {
        let mut db = Database::open_in_memory()?;
        let [notas, viaje, islandia, trabajo] = tree(&db)?;

        db.move_page(&viaje, Some(&trabajo))?;
        assert_eq!(parent_of(&db, &viaje)?, Some(trabajo.clone()));
        assert_eq!(
            parent_of(&db, &islandia)?,
            Some(viaje.clone()),
            "subtree moves along"
        );

        db.move_page(&islandia, None)?;
        assert_eq!(parent_of(&db, &islandia)?, None);
        assert!(db.get_page(&notas)?.is_some());
        Ok(())
    }

    #[test]
    fn moved_page_becomes_last_sibling() -> anyhow::Result<()> {
        let mut db = Database::open_in_memory()?;
        let [notas, viaje, _, trabajo] = tree(&db)?;
        db.move_page(&notas, Some(&trabajo))?;
        db.move_page(&viaje, Some(&trabajo))?;
        let children: Vec<String> = db
            .list_pages()?
            .into_iter()
            .filter(|p| p.parent_id.as_deref() == Some(trabajo.as_str()))
            .map(|p| p.title)
            .collect();
        assert_eq!(children, vec!["Notas", "Viaje"]);
        Ok(())
    }

    #[test]
    fn cycles_are_rejected_and_nothing_changes() -> anyhow::Result<()> {
        let mut db = Database::open_in_memory()?;
        let [notas, viaje, islandia, _] = tree(&db)?;
        let before = db.list_pages()?;

        assert!(db.move_page(&notas, Some(&notas)).is_err(), "into itself");
        assert!(
            db.move_page(&notas, Some(&viaje)).is_err(),
            "into its child"
        );
        assert!(
            db.move_page(&notas, Some(&islandia)).is_err(),
            "into its grandchild"
        );
        assert_eq!(db.list_pages()?, before);
        Ok(())
    }

    #[test]
    fn missing_or_trashed_pages_are_rejected() -> anyhow::Result<()> {
        let mut db = Database::open_in_memory()?;
        let [notas, viaje, _, trabajo] = tree(&db)?;
        assert!(db.move_page("no-existe", None).is_err());
        assert!(db.move_page(&viaje, Some("no-existe")).is_err());

        db.trash_page(&trabajo)?;
        assert!(
            db.move_page(&viaje, Some(&trabajo)).is_err(),
            "trashed destination"
        );
        assert!(
            db.move_page(&trabajo, Some(&notas)).is_err(),
            "trashed page"
        );
        assert_eq!(parent_of(&db, &viaje)?, Some(notas));
        Ok(())
    }

    #[test]
    fn move_persists_after_reopen() -> anyhow::Result<()> {
        let path =
            std::env::temp_dir().join(format!("silex-tree-{}.sqlite3", uuid::Uuid::new_v4()));
        let result = (|| -> anyhow::Result<()> {
            let (viaje, trabajo) = {
                let mut db = Database::open(&path)?;
                let [_, viaje, _, trabajo] = tree(&db)?;
                db.move_page(&viaje, Some(&trabajo))?;
                (viaje, trabajo)
            };
            let db = Database::open(&path)?;
            assert_eq!(parent_of(&db, &viaje)?, Some(trabajo));
            Ok(())
        })();
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        result
    }
}
