//! The last open view of each database, kept between sessions (H11.6).
use std::{fs, path::PathBuf};

use silex_desktop::db::Database;
use silex_desktop::model::ViewKind;
use silex_desktop::settings::Settings;
use silex_desktop::view::ViewConfig;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("silex-last-view-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A database with two views; returns its id and the second view's.
fn with_two_views(db: &mut Database, title: &str) -> anyhow::Result<(String, String)> {
    let base = db.create_database(title, None)?;
    let board = db.add_view(&base, "Tablero", ViewKind::Board, &ViewConfig::default())?;
    Ok((base, board))
}

#[test]
fn the_last_view_survives_reopening_the_file() -> anyhow::Result<()> {
    let tmp = TempDir::new()?;
    let path = tmp.0.join("silex.sqlite3");
    let (base, board) = {
        let mut db = Database::open(&path)?;
        let (base, board) = with_two_views(&mut db, "Lecturas")?;
        assert_eq!(db.last_view(&base)?, None, "nothing stored yet");
        db.set_last_view(&base, &board)?;
        (base, board)
    };
    let db = Database::open(&path)?;
    assert_eq!(db.last_view(&base)?.as_deref(), Some(board.as_str()));
    // Not a preference: the settings ignore it.
    assert_eq!(db.load_settings()?, Settings::default());
    Ok(())
}

#[test]
fn each_database_has_its_own_and_another_ones_view_is_refused() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (lecturas, tablero) = with_two_views(&mut db, "Lecturas")?;
    let (recetas, tablero_recetas) = with_two_views(&mut db, "Recetas")?;
    db.set_last_view(&lecturas, &tablero)?;
    assert!(
        db.set_last_view(&recetas, &tablero).is_err(),
        "not its view"
    );
    assert!(db.set_last_view(&recetas, "no-existe").is_err());
    assert_eq!(db.last_view(&recetas)?, None);
    db.set_last_view(&recetas, &tablero_recetas)?;
    let first = db.database_views(&lecturas)?[0].id.clone();
    db.set_last_view(&lecturas, &first)?;
    assert_eq!(db.last_view(&lecturas)?, Some(first));
    assert_eq!(db.last_view(&recetas)?, Some(tablero_recetas));
    Ok(())
}

#[test]
fn a_deleted_view_stays_stored_and_the_caller_falls_back() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (base, board) = with_two_views(&mut db, "Lecturas")?;
    db.set_last_view(&base, &board)?;
    db.delete_view(&board)?;
    let stored = db.last_view(&base)?;
    assert_eq!(stored.as_deref(), Some(board.as_str()));
    assert!(
        !db.database_views(&base)?
            .iter()
            .any(|v| Some(&v.id) == stored.as_ref()),
        "no longer a view: the app opens the first"
    );
    Ok(())
}

#[test]
fn deleting_for_good_forgets_the_view() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let (one, one_board) = with_two_views(&mut db, "Una")?;
    let (two, two_board) = with_two_views(&mut db, "Dos")?;
    let (three, three_board) = with_two_views(&mut db, "Tres")?;
    for (base, view) in [
        (&one, &one_board),
        (&two, &two_board),
        (&three, &three_board),
    ] {
        db.set_last_view(base, view)?;
    }
    // In the trash it is kept: restoring brings the view back too.
    db.trash_page(&one)?;
    assert!(db.last_view(&one)?.is_some());
    db.purge_trash_item(&one)?;
    assert_eq!(db.last_view(&one)?, None);

    db.trash_page(&two)?;
    db.empty_trash()?;
    assert_eq!(db.last_view(&two)?, None);
    assert_eq!(db.last_view(&three)?, Some(three_board), "a live one stays");
    Ok(())
}
