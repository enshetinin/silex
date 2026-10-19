use super::*;

use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    db::Database,
    model::{Block, BlockKind},
    property::{CellValue, Date, OptionColor, PropertyKind, SelectOption},
};

fn block(id: &str) -> Block {
    Block {
        id: id.into(),
        page_id: "p".into(),
        kind: BlockKind::Paragraph,
        content: String::new(),
        checked: false,
        position: 0,
        indent: 0,
    }
}

/// Headless egui: runs one frame with the given events at `time` seconds.
fn frame(
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
    blocks: &mut [Block],
    focus: Option<egui::Id>,
) -> bool {
    let input = egui::RawInput {
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut changed = false;
    let mut output = ctx.run_ui(input, |ui| {
        if let Some(id) = focus {
            ui.memory_mut(|m| m.request_focus(id));
        }
        for block in blocks.iter_mut() {
            changed |= block_field(
                ui,
                0,
                block,
                &BlockView {
                    number: None,
                    scroll_into_view: false,
                    selected: false,
                    show_handle: false,
                    focused: false,
                },
            )
            .text_changed;
        }
    });
    // Headless: nothing renders the font texture, so drop it explicitly.
    output.textures_delta.clear();
    changed
}

fn type_text(text: &str) -> Vec<egui::Event> {
    vec![egui::Event::Text(text.into())]
}

fn undo() -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }]
}

/// Collects all text painted in a frame (egui paints ID-clash warnings as text).
fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn main_layout_has_no_id_clashes() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let page = app.selected.clone().expect("first page is open");
    for kind in BlockKind::ALL {
        app.db.add_block(&page, kind)?;
    }
    app.blocks = app.db.list_blocks(&page)?;
    app.reset_history();
    // A nested, expanded tree: page ─ child ─ grandchild, plus a second child.
    let child = app.db.create_page("Hija", Some(&page))?;
    app.db.create_page("Nieta", Some(&child))?;
    app.db.create_page("Hija 2", Some(&page))?;
    app.db.set_favorite(&child, true)?;
    app.expanded.extend([page.clone(), child]);
    app.reload_pages()?;
    assert_eq!(app.favorites.len(), 1);
    let depths: Vec<usize> = app.tree.iter().map(|r| r.depth).collect();
    assert_eq!(depths, vec![0, 1, 2, 1]);

    let ctx = egui::Context::default();
    let mut warnings = Vec::new();
    // Two frames: egui reports clashes against the previous frame's widgets too.
    for time in [0.0, 0.1] {
        let input = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
        output.textures_delta.clear();
        warnings = painted_text(&output.shapes)
            .into_iter()
            .filter(|t| t.contains(" use of ") && t.contains(" ID "))
            .collect();
    }
    assert!(warnings.is_empty(), "egui ID clashes: {warnings:?}");
    Ok(())
}

#[test]
fn trash_and_restore_from_the_app() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let first = app.selected.clone().expect("first page is open");
    app.create_subpage(&first)?;
    let child = app.selected.clone().expect("subpage is open");
    app.db.create_page("Otra", None)?;
    app.reload_pages()?;
    app.open_page(first.clone())?;

    // Unsaved edits are saved before trashing.
    app.title = "Primera editada".into();
    app.mark_dirty();
    app.trash_page(&first)?;
    assert_eq!(app.trash_count, 1);
    assert!(app.pages.iter().all(|p| p.id != first && p.id != child));
    assert_ne!(app.selected.as_deref(), Some(first.as_str()));
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("1 subpágina"))
    );

    app.restore_from_trash(&first)?;
    assert_eq!(app.trash_count, 0);
    assert_eq!(app.selected.as_deref(), Some(first.as_str()));
    assert_eq!(app.title, "Primera editada");
    assert!(app.pages.iter().any(|p| p.id == child));
    Ok(())
}

#[test]
fn trashing_the_last_page_shows_the_empty_state() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let only = app.selected.clone().expect("first page is open");
    app.trash_page(&only)?;
    assert_eq!(app.selected, None);
    assert!(app.tree.is_empty());
    Ok(())
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Runs one full app frame (shortcuts, layout, dialogs) with `events`.
fn app_frame(app: &mut SilexApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
    let input = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
    painted_text(&output.shapes)
}

#[test]
fn keyboard_shortcuts_create_and_navigate() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    let first = app.selected.clone().expect("first page");

    app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND)]);
    let second = app.selected.clone().expect("new page is open");
    assert_ne!(second, first);
    assert_eq!(app.pages.len(), 2);

    // Cmd+Shift+N makes a subpage only (not also a root page).
    app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND | M::SHIFT)]);
    let child = app.selected.clone().expect("subpage is open");
    assert_eq!(app.pages.len(), 3);
    let parent = app
        .pages
        .iter()
        .find(|p| p.id == child)
        .and_then(|p| p.parent_id.clone());
    assert_eq!(parent, Some(second.clone()));

    // Visible tree: first, second, child. Alt+Up walks back up.
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
    assert_eq!(app.selected.as_deref(), Some(second.as_str()));
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
    assert_eq!(app.selected.as_deref(), Some(first.as_str()));
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT)]);
    assert_eq!(
        app.selected.as_deref(),
        Some(first.as_str()),
        "stays at the top"
    );
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::ALT)]);
    assert_eq!(app.selected.as_deref(), Some(second.as_str()));

    // Cmd+K opens the search window with its field focused; Escape closes it.
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.search.is_some());
    assert!(ctx.memory(|m| m.has_focus(search_field_id())));
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.search.is_none());
    // F2 starts renaming the open page.
    app_frame(&mut app, &ctx, vec![key(Key::F2, M::NONE)]);
    assert_eq!(
        app.rename.as_ref().map(|r| r.id.as_str()),
        Some(second.as_str())
    );
    Ok(())
}

#[test]
fn rename_from_sidebar_keeps_editor_and_database_in_sync() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let open = app.selected.clone().expect("first page");
    let other = app.db.create_page("Otra", None)?;
    app.reload_pages()?;

    // Renaming another page goes straight to the database.
    app.start_rename(&other);
    if let Some(r) = app.rename.as_mut() {
        r.text = "  Renombrada  ".into();
    }
    app.commit_rename()?;
    assert_eq!(
        app.db.get_page(&other)?.map(|p| p.title).as_deref(),
        Some("Renombrada")
    );
    assert_eq!(app.selected.as_deref(), Some(open.as_str()));

    // The open page starts from the editor's unsaved title and saves through it.
    app.title = "Sin guardar".into();
    app.mark_dirty();
    app.start_rename(&open);
    assert_eq!(
        app.rename.as_ref().map(|r| r.text.as_str()),
        Some("Sin guardar")
    );
    if let Some(r) = app.rename.as_mut() {
        r.text = "Definitivo".into();
    }
    app.commit_rename()?;
    assert_eq!(app.title, "Definitivo");
    assert!(app.save.can_close(), "saved");
    assert_eq!(
        app.db.get_page(&open)?.map(|p| p.title).as_deref(),
        Some("Definitivo")
    );

    // Empty becomes "Sin título"; cancelling changes nothing.
    app.start_rename(&other);
    if let Some(r) = app.rename.as_mut() {
        r.text = "   ".into();
    }
    app.commit_rename()?;
    assert_eq!(
        app.db.get_page(&other)?.map(|p| p.title).as_deref(),
        Some("Sin título")
    );
    app.start_rename(&other);
    app.apply(RowAction::CancelRename)?;
    assert!(app.rename.is_none());
    assert_eq!(
        app.db.get_page(&other)?.map(|p| p.title).as_deref(),
        Some("Sin título")
    );
    Ok(())
}

#[test]
fn trashing_another_page_keeps_the_open_one() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let open = app.selected.clone().expect("first page");
    let other = app.db.create_page("Para borrar", None)?;
    app.reload_pages()?;
    app.apply(RowAction::Trash(other))?;
    assert_eq!(app.selected.as_deref(), Some(open.as_str()));
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("Para borrar"))
    );
    Ok(())
}

/// Pages for the search tests: "Recetas" ─ "Pan de centeno", and "Notas"
/// whose second block mentions bread. Returns (pan de centeno, notas).
fn search_pages(app: &mut SilexApp) -> anyhow::Result<(String, String)> {
    let recetas = app.db.create_page("Recetas", None)?;
    let pan = app.db.create_page("Pan de centeno", Some(&recetas))?;
    let blocks = [
        ("Lista de la compra", BlockKind::Heading2),
        ("leche, huevos y pan integral", BlockKind::Bulleted),
    ];
    let new: Vec<_> = blocks
        .iter()
        .map(|(text, kind)| silex_desktop::model::NewBlock {
            kind: *kind,
            indent: 0,
            checked: false,
            content: (*text).to_owned(),
        })
        .collect();
    let notas = app.db.create_page_with_blocks("Notas", None, &new)?;
    app.reload_pages()?;
    Ok((pan, notas))
}

#[test]
fn search_window_ranks_results_and_opens_the_match() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let (pan, notas) = search_pages(&mut app)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("pan".into())]);
    let window = app.search.as_ref().expect("window open");
    let found: Vec<&str> = window.results.iter().map(|h| h.page.id.as_str()).collect();
    // The title match first, then the content match.
    assert_eq!(found, [pan.as_str(), notas.as_str()]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Recetas"), "path shown");

    // ↓ then Enter opens "Notas" with the cursor on the match.
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    assert_eq!(app.search.as_ref().map(|w| w.selected), Some(1));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.search.is_none(), "closed");
    assert_eq!(app.selected.as_deref(), Some(notas.as_str()));
    let block = app.blocks[1].id.clone();
    let offset = "leche, huevos y ".chars().count();
    assert_eq!(app.pending_focus, Some((block, offset)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    assert_eq!(caret_char(&app), Some(offset));
    Ok(())
}

#[test]
fn enter_never_opens_a_result_of_an_older_query() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let (pan, notas) = search_pages(&mut app)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("centeno".into())]);
    let shown: Vec<String> = app
        .search
        .as_ref()
        .map(|w| w.results.iter().map(|h| h.page.id.clone()).collect())
        .unwrap_or_default();
    assert_eq!(shown, [pan]);
    // New text and Enter arrive in the same frame: Enter is read before the
    // field takes the text, while the old results are still listed.
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("leche".into()), key(Key::Enter, M::NONE)],
    );
    assert!(app.search.is_none());
    assert_eq!(app.selected.as_deref(), Some(notas.as_str()));
    Ok(())
}

#[test]
fn empty_palette_lists_commands_and_enter_runs_the_first() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    use silex_desktop::palette::Command;
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    let pages = app.pages.len();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Acciones"));
    assert!(painted.iter().any(|t| t == "Nueva página"));
    assert!(
        painted.iter().any(|t| t == "Cmd+N" || t == "Ctrl+N"),
        "shortcut shown"
    );
    let commands = app
        .search
        .as_ref()
        .map(|w| w.commands.clone())
        .unwrap_or_default();
    assert_eq!(commands.first(), Some(&Command::NewPage));
    // In-memory database: no backups or workspace export offered.
    assert!(!commands.contains(&Command::BackupNow));
    assert!(!commands.contains(&Command::ExportWorkspace));

    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.search.is_none());
    assert_eq!(app.pages.len(), pages + 1, "a new page was created");
    assert!(app.focus_title, "with its title ready to type");
    Ok(())
}

#[test]
fn commands_come_before_pages_and_run_from_the_keyboard() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    use silex_desktop::palette::Command;
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let viaje = app.db.create_page("Favoritos del viaje", None)?;
    app.reload_pages()?;
    let ctx = egui::Context::default();
    let search = |app: &mut SilexApp, text: &str| {
        app_frame(app, &ctx, vec![key(Key::K, M::COMMAND)]);
        app_frame(app, &ctx, vec![egui::Event::Text(text.into())]);
    };

    // "favorit": the command first, then the page; ↓ Enter opens the page.
    search(&mut app, "favorit");
    let window = app.search.as_ref().expect("open");
    assert_eq!(window.commands, [Command::AddFavorite]);
    assert_eq!(window.results.len(), 1);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.selected.as_deref(), Some(viaje.as_str()));

    // Enter on the command marks the open page; then it offers the opposite.
    search(&mut app, "favorit");
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.favorites.iter().any(|p| p.id == viaje));
    search(&mut app, "favorit");
    assert_eq!(
        app.search.as_ref().map(|w| w.commands.clone()),
        Some(vec![Command::RemoveFavorite])
    );
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);

    // Settings commands apply and are stored.
    search(&mut app, "oscuro");
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        ctx.options(|o| o.theme_preference),
        egui::ThemePreference::Dark
    );
    assert_eq!(app.db.load_settings()?.theme, ThemeMode::Dark);
    Ok(())
}

#[test]
fn palette_commands_reuse_the_app_actions() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    use silex_desktop::palette::Command;
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    let first = app.selected.clone().expect("first page");

    app.run_command(&ctx, Command::ZoomIn)?;
    assert_eq!(app.settings.zoom_percent, 110);
    app.run_command(&ctx, Command::ZoomOut)?;
    app.run_command(&ctx, Command::ZoomOut)?;
    assert_eq!(app.db.load_settings()?.zoom_percent, 90);
    app.run_command(&ctx, Command::ZoomReset)?;
    assert_eq!(app.settings.zoom_percent, 100);

    app.run_command(&ctx, Command::RenamePage)?;
    assert!(app.focus_title);
    app.run_command(&ctx, Command::NewSubpage)?;
    let child = app.selected.clone().expect("subpage open");
    let parent = app
        .pages
        .iter()
        .find(|p| p.id == child)
        .and_then(|p| p.parent_id.clone());
    assert_eq!(parent.as_deref(), Some(first.as_str()));

    app.run_command(&ctx, Command::TrashPage)?;
    assert!(app.pages.iter().all(|p| p.id != child), "in the trash");
    app.run_command(&ctx, Command::OpenTrash)?;
    assert!(app.trash_dialog.is_some());
    app.trash_dialog = None;

    app.run_command(&ctx, Command::ShowShortcuts)?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Atajos de teclado"));
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(!app.shortcuts_dialog, "Escape closes it");
    Ok(())
}

#[test]
fn search_window_has_the_keyboard_until_it_closes() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    search_pages(&mut app)?;
    let ctx = egui::Context::default();
    let pages = app.pages.len();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    // Other shortcuts wait while the window is open.
    app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::ALT)]);
    assert_eq!(app.pages.len(), pages, "Cmd+N did nothing");
    // Enter with no results does nothing; Cmd+K closes.
    app_frame(&mut app, &ctx, vec![egui::Event::Text("zzz".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t.contains("Nada coincide")));
    assert!(app.search.is_some());
    assert!(
        ctx.memory(|m| m.has_focus(search_field_id())),
        "the field keeps the focus"
    );
    // The focus comes back to the field if it is lost.
    ctx.memory_mut(|m| m.surrender_focus(search_field_id()));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
    assert_eq!(app.search.as_ref().map(|w| w.query.as_str()), Some("zzz!"));
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.search.is_none());
    // Reopened, it starts empty.
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.search.as_ref().is_some_and(|w| w.query.is_empty()));
    Ok(())
}

#[test]
fn opening_search_saves_first_and_drops_the_block_selection() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno"), (Par, 0, "dos")], 0, 3)?;
    type_into(&mut app, &ctx, " tilo");
    app.selection = Some(BlockSelection {
        anchor: app.blocks[1].id.clone(),
        head: app.blocks[1].id.clone(),
    });
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.save.can_close(), "saved before searching");
    assert!(app.selection.is_none());
    // Keys go to the window, never to the old selection.
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(texts(&app), ["uno tilo", "dos"]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("tilo".into())]);
    assert_eq!(app.search.as_ref().map(|w| w.results.len()), Some(1));
    Ok(())
}

#[test]
fn enter_confirming_an_ime_composition_does_not_open_a_result() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    search_pages(&mut app)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("pan ".into())]);
    let opened = app.selected.clone();
    let preedit = egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "に".into(),
        active_range_chars: None,
    });
    app_frame(&mut app, &ctx, vec![preedit]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("に".into()))],
    );
    assert!(app.search.is_some(), "still open");
    assert_eq!(app.selected, opened);
    Ok(())
}

#[test]
fn search_runs_on_a_worker_with_a_file_database() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    // Declared before the app: dropped after it (Windows cannot delete open files).
    let tmp = TempFolder::new("silex-app-search")?;
    let mut app = SilexApp::new(Database::open(tmp.0.join("silex.sqlite3"))?)?;
    app.startup_backup_pending = false;
    let (pan, notas) = search_pages(&mut app)?;
    let ctx = egui::Context::default();
    let wait_for = |app: &mut SilexApp, done: &dyn Fn(&SilexApp) -> bool| {
        let start = std::time::Instant::now();
        while !done(app) && start.elapsed() < std::time::Duration::from_secs(30) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_frame(app, &ctx, vec![]);
        }
    };
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("centeno".into())]);
    wait_for(&mut app, &|app| {
        app.search.as_ref().is_some_and(|w| !w.results.is_empty())
    });
    let window = app.search.as_ref().expect("window open");
    assert_eq!(window.error, None);
    let found: Vec<&str> = window.results.iter().map(|h| h.page.id.as_str()).collect();
    assert_eq!(found, [pan.as_str()]);

    // Enter right after typing opens a result of the new query, never one
    // of the previous query that is still on screen.
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("leche".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    wait_for(&mut app, &|app| app.search.is_none());
    assert_eq!(app.selected.as_deref(), Some(notas.as_str()));

    // Enter on a command runs it at once, even while pages are searched.
    let pages = app.pages.len();
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("nueva".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.search.is_none());
    assert_eq!(app.pages.len(), pages + 1);
    drop(app);
    Ok(())
}

#[test]
fn sidebar_states_have_no_id_clashes() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let open = app.selected.clone().expect("first page");
    // The same page in favorites and in the tree, plus an inline rename.
    app.db.set_favorite(&open, true)?;
    let child = app.db.create_page("Hija", Some(&open))?;
    app.expanded.insert(open.clone());
    app.reload_pages()?;
    app.start_rename(&child);
    let ctx = egui::Context::default();
    for state in ["at rest", "search window"] {
        if state == "search window" {
            app.open_search(&ctx);
            if let Some(window) = app.search.as_mut() {
                window.query = "hija".into();
            }
        }
        for _ in 0..2 {
            let warnings: Vec<String> = app_frame(&mut app, &ctx, vec![])
                .into_iter()
                .filter(|t| t.contains(" use of ") && t.contains(" ID "))
                .collect();
            assert!(warnings.is_empty(), "{state}: {warnings:?}");
        }
    }
    Ok(())
}

/// App with one page whose blocks are `(kind, indent, text)`, the given
/// block focused at `char`, after a frame so the caret is known.
fn editor_app(
    specs: &[(BlockKind, u8, &str)],
    focus: usize,
    char: usize,
) -> anyhow::Result<(SilexApp, egui::Context)> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let page = app.selected.clone().expect("page");
    for (kind, _, _) in specs {
        app.db.add_block(&page, *kind)?;
    }
    let mut blocks = app.db.list_blocks(&page)?;
    for (block, (_, indent, text)) in blocks.iter_mut().zip(specs) {
        block.indent = *indent;
        block.content = (*text).to_owned();
    }
    let title = app.title.clone();
    app.db.save_document(&page, &title, &blocks)?;
    app.blocks = blocks;
    // As open_page does: the loaded document is the starting point for undo.
    app.reset_history();
    let ctx = egui::Context::default();
    app.pending_focus = Some((app.blocks[focus].id.clone(), char));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx))
}

fn contents(app: &SilexApp) -> Vec<(BlockKind, u8, String)> {
    app.blocks
        .iter()
        .map(|b| (b.kind, b.indent, b.content.clone()))
        .collect()
}

fn focused_block(app: &SilexApp, ctx: &egui::Context) -> Option<usize> {
    app.blocks
        .iter()
        .position(|b| ctx.memory(|m| m.has_focus(app.block_field_id(&b.id))))
}

fn caret_char(app: &SilexApp) -> Option<usize> {
    app.caret.as_ref().map(|c| c.char)
}

use BlockKind::{Bulleted as Bul, Code as Cod, Paragraph as Par};
use egui::{Key, Modifiers as M};

#[test]
fn enter_splits_and_focuses_the_new_block() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "hola🙂mundo")], 0, 5)?;
    assert_eq!(caret_char(&app), Some(5));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        contents(&app),
        [(Par, 0, "hola🙂".into()), (Par, 0, "mundo".into())]
    );
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    assert_eq!(caret_char(&app), Some(0));

    // The split is persisted by the normal save path.
    assert!(!app.save.can_close());
    app.save_now()?;
    let page = app.selected.clone().expect("page");
    assert_eq!(app.db.list_blocks(&page)?.len(), 2);
    Ok(())
}

#[test]
fn shift_enter_is_a_newline_inside_the_block() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 1)?;
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::SHIFT)]);
    assert_eq!(contents(&app), [(Par, 0, "a\nb".into())]);
    Ok(())
}

#[test]
fn enter_in_code_block_is_a_newline() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Cod, 0, "fn x()")], 0, 6)?;
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks.len(), 1);
    assert!(app.blocks[0].content.contains('\n'));
    Ok(())
}

#[test]
fn text_typed_before_enter_in_the_same_frame_is_kept_in_order() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("X".into()), key(Key::Enter, M::NONE)],
    );
    assert_eq!(
        contents(&app),
        [(Par, 0, "abX".into()), (Par, 0, "".into())]
    );
    Ok(())
}

#[test]
fn backspace_at_start_merges_and_mid_text_deletes() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "Café"), (Par, 0, " ☕")], 1, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(contents(&app), [(Par, 0, "Café ☕".into())]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0));
    assert_eq!(caret_char(&app), Some(4), "cursor at the seam");

    // Not at the start: a normal character delete by the text field.
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(contents(&app), [(Par, 0, "Caf ☕".into())]);
    Ok(())
}

#[test]
fn tab_and_shift_tab_change_indent_and_keep_focus() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Bul, 0, "uno"), (Bul, 0, "dos")], 1, 1)?;
    app_frame(&mut app, &ctx, vec![key(Key::Tab, M::NONE)]);
    assert_eq!(app.blocks[1].indent, 1);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    app_frame(&mut app, &ctx, vec![key(Key::Tab, M::SHIFT)]);
    assert_eq!(app.blocks[1].indent, 0);
    Ok(())
}

#[test]
fn arrows_move_between_blocks_at_the_edges() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "arriba"), (Par, 0, "abajo")], 1, 2)?;
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0));
    assert_eq!(caret_char(&app), Some(6), "end of the block above");
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    assert_eq!(caret_char(&app), Some(0));
    assert_eq!(app.blocks.len(), 2, "navigation changes nothing");
    assert!(app.save.can_close());
    Ok(())
}

#[test]
fn keys_are_left_alone_during_ime_composition() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
    let preedit = |text: &str| {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: None,
        })
    };
    app_frame(&mut app, &ctx, vec![preedit("に")]);
    assert!(app.ime_composing);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        app.blocks.len(),
        1,
        "Enter confirms the composition, no split"
    );
    // Composition ends (empty preedit): Enter splits again.
    app_frame(&mut app, &ctx, vec![preedit("")]);
    assert!(!app.ime_composing);
    Ok(())
}

/// egui selects the preedit text, so the selection rule alone already
/// blocks Enter above. This isolates the IME guard itself: a composition
/// in progress with no selection must still not be intercepted.
#[test]
fn ime_guard_blocks_editor_keys_even_without_selection() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab"), (Par, 0, "cd")], 1, 0)?;
    assert!(app.caret.as_ref().is_some_and(|c| !c.selection));
    app.ime_composing = true;
    // Keys reach the text field as usual; only block structure must not change.
    let ids = |app: &SilexApp| app.blocks.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
    let before = ids(&app);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(ids(&app), before, "no split while composing");
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(ids(&app), before, "no merge while composing");
    let commit = egui::Event::Ime(egui::ImeEvent::Commit("に".into()));
    app_frame(&mut app, &ctx, vec![commit]);
    assert!(!app.ime_composing, "a commit ends the composition");
    Ok(())
}

/// Like `app_frame`, but returns the platform commands (e.g. copy).
fn app_frame_commands(
    app: &mut SilexApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> Vec<egui::OutputCommand> {
    let input = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
    output.platform_output.commands
}

fn type_into(app: &mut SilexApp, ctx: &egui::Context, text: &str) {
    for c in text.chars() {
        app_frame(app, ctx, vec![egui::Event::Text(c.to_string())]);
    }
}

#[test]
fn slash_command_changes_the_block_type() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
    type_into(&mut app, &ctx, "/h2");
    let menu = app.slash.clone().expect("menu open");
    assert_eq!(menu.matches, [BlockKind::Heading2]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(contents(&app), [(BlockKind::Heading2, 0, String::new())]);
    assert!(app.slash.is_none());
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0), "keeps editing");
    Ok(())
}

#[test]
fn slash_menu_arrows_pick_and_escape_closes() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
    type_into(&mut app, &ctx, "/titulo");
    assert_eq!(app.slash.as_ref().map(|m| m.matches.len()), Some(3));
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    assert_eq!(app.slash.as_ref().map(|m| m.selected), Some(1));
    assert_eq!(app.blocks.len(), 1, "arrows stay in the menu");
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks[0].kind, BlockKind::Heading2);

    // Escape closes the menu and leaves the text (no block selection).
    let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
    type_into(&mut app, &ctx, "/h1");
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.slash.is_none());
    assert!(app.selection.is_none());
    assert_eq!(app.blocks[0].content, "/h1");
    assert_eq!(app.blocks[0].kind, Par);
    // Coming back to the block does not reopen it until the text changes.
    app.pending_focus = Some((app.blocks[0].id.clone(), 3));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0));
    assert!(app.slash.is_none(), "stays dismissed");
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks[0].content, "/h");
    assert!(app.slash.is_some(), "text changed: menu is back");
    Ok(())
}

#[test]
fn enter_without_slash_matches_splits_as_usual() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
    type_into(&mut app, &ctx, "/zzz");
    assert!(app.slash.is_none());
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks.len(), 2);
    Ok(())
}

#[test]
fn block_selection_extend_copy_delete_and_undo() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(
        &[
            (Par, 0, "uno"),
            (Bul, 0, "dos"),
            (Bul, 1, "tres"),
            (Par, 0, "cuatro"),
        ],
        1,
        0,
    )?;
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.selection_range(), Some((1, 1)));
    assert_eq!(focused_block(&app, &ctx), None, "text field left");

    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
    assert_eq!(app.selection_range(), Some((1, 2)));
    let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Copy]);
    assert!(
        commands.contains(&egui::OutputCommand::CopyText("• dos\n  • tres".into())),
        "{commands:?}"
    );

    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    let texts: Vec<String> = app.blocks.iter().map(|b| b.content.clone()).collect();
    assert_eq!(texts, ["uno", "cuatro"]);
    assert_eq!(app.selection_range(), Some((1, 1)), "selection moves on");
    assert!(!app.save.can_close());

    app.undo();
    let texts: Vec<String> = app.blocks.iter().map(|b| b.content.clone()).collect();
    assert_eq!(texts, ["uno", "dos", "tres", "cuatro"]);
    Ok(())
}

#[test]
fn selection_moves_with_arrows_and_enter_goes_back_to_editing() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "bb"), (Par, 0, "c")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    assert_eq!(app.selection_range(), Some((1, 1)));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.selection.is_none());
    assert_eq!(focused_block(&app, &ctx), Some(1));
    assert_eq!(caret_char(&app), Some(2), "cursor at the end");
    Ok(())
}

#[test]
fn cut_copies_and_deletes_and_page_switch_drops_undo() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Cut]);
    assert!(commands.contains(&egui::OutputCommand::CopyText("a".into())));
    assert_eq!(app.blocks.len(), 1);
    assert!(app.history.can_undo());
    let other = app.db.create_page("Otra", None)?;
    app.reload_pages()?;
    app.open_page(other)?;
    assert!(!app.history.can_undo() && app.selection.is_none());
    Ok(())
}

fn texts(app: &SilexApp) -> Vec<String> {
    app.blocks.iter().map(|b| b.content.clone()).collect()
}

#[test]
fn undo_and_redo_walk_typing_and_structure() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
    type_into(&mut app, &ctx, "cd"); // quick typing: one undo step
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(texts(&app), ["abcd", ""]);

    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(texts(&app), ["abcd"], "undo the split");
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(texts(&app), ["ab"], "undo the typing as one step");
    assert!(!app.history.can_undo());

    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND | M::SHIFT)]);
    assert_eq!(texts(&app), ["abcd"]);
    app_frame(&mut app, &ctx, vec![key(Key::Y, M::COMMAND)]);
    assert_eq!(texts(&app), ["abcd", ""], "Cmd+Y also redoes");

    // Undo results are saved like any edit.
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    app.save_now()?;
    let page = app.selected.clone().expect("page");
    assert_eq!(app.db.list_blocks(&page)?.len(), 1);
    Ok(())
}

#[test]
fn undo_focuses_the_changed_block() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno"), (Par, 0, "dos")], 1, 3)?;
    type_into(&mut app, &ctx, "!");
    app.pending_focus = Some((app.blocks[0].id.clone(), 0));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(texts(&app), ["uno", "dos"]);
    assert_eq!(
        focused_block(&app, &ctx),
        Some(1),
        "cursor goes to the undone block"
    );
    Ok(())
}

#[test]
fn cmd_z_in_the_search_field_leaves_the_document_alone() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
    type_into(&mut app, &ctx, "c");
    assert!(app.history.can_undo());
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(texts(&app), ["abc"]);
    assert!(app.history.can_undo(), "document history untouched");
    Ok(())
}

#[test]
fn alt_shift_arrows_move_the_block_and_cmd_d_duplicates() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b"), (Par, 0, "c")], 0, 1)?;
    let page = app.selected.clone();
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::ALT | M::SHIFT)]);
    assert_eq!(texts(&app), ["b", "a", "c"]);
    assert_eq!(app.selected, page, "not the Alt+↓ page navigation");
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        focused_block(&app, &ctx),
        Some(1),
        "focus follows the block"
    );

    app_frame(&mut app, &ctx, vec![key(Key::D, M::COMMAND)]);
    assert_eq!(texts(&app), ["b", "a", "a", "c"]);
    assert_ne!(app.blocks[1].id, app.blocks[2].id);
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(texts(&app), ["a", "b", "c"]);
    Ok(())
}

#[test]
fn selection_moves_and_duplicates_as_a_group() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(
        &[(Par, 0, "a"), (Par, 0, "b"), (Par, 0, "c"), (Par, 0, "d")],
        2,
        0,
    )?;
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::ALT | M::SHIFT)]);
    assert_eq!(texts(&app), ["a", "c", "d", "b"]);
    assert_eq!(app.selection_range(), Some((1, 2)), "selection follows");
    app_frame(&mut app, &ctx, vec![key(Key::D, M::COMMAND)]);
    assert_eq!(texts(&app), ["a", "c", "d", "c", "d", "b"]);
    assert_eq!(
        app.selection_range(),
        Some((3, 4)),
        "the copies are selected"
    );
    Ok(())
}

#[test]
fn block_menu_actions_and_add_buttons_are_undoable() -> anyhow::Result<()> {
    let (mut app, _ctx) = editor_app(&[(Par, 0, "a"), (Par, 0, "b")], 0, 0)?;
    let first = app.blocks[0].id.clone();
    app.block_action(BlockAction::MoveDown, &first);
    assert_eq!(texts(&app), ["b", "a"]);
    app.block_action(BlockAction::Duplicate, &first);
    assert_eq!(texts(&app), ["b", "a", "a"]);
    app.block_action(BlockAction::Delete, &first);
    assert_eq!(texts(&app), ["b", "a"]);
    for _ in 0..3 {
        app.undo();
    }
    assert_eq!(texts(&app), ["a", "b"]);
    Ok(())
}

fn paste(text: &str) -> egui::Event {
    egui::Event::Paste(text.into())
}

#[test]
fn multi_line_paste_becomes_blocks_and_undoes_in_one_step() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "Hola mundo")], 0, 5)?;
    app_frame(&mut app, &ctx, vec![paste("uno\r\ndos\ntres ")]);
    assert_eq!(texts(&app), ["Hola uno", "dos", "tres mundo"]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(2));
    assert_eq!(
        caret_char(&app),
        Some(5),
        "cursor at the end of the pasted text"
    );
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(texts(&app), ["Hola mundo"]);
    Ok(())
}

#[test]
fn single_line_paste_and_code_blocks_are_left_to_the_text_field() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 1)?;
    app_frame(&mut app, &ctx, vec![paste("XY")]);
    assert_eq!(texts(&app), ["aXYb"]);

    let (mut app, ctx) = editor_app(&[(Cod, 0, "")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![paste("fn a() {\n}\n")]);
    assert_eq!(app.blocks.len(), 1);
    assert_eq!(app.blocks[0].content, "fn a() {\n}\n");
    Ok(())
}

#[test]
fn copied_blocks_paste_back_with_their_structure() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Bul, 0, "Pan"), (Bul, 1, "Integral"), (Par, 0, "")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
    let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Copy]);
    let Some(egui::OutputCommand::CopyText(copied)) = commands.into_iter().next() else {
        anyhow::bail!("nothing copied");
    };
    // Back to editing the empty last block, then paste.
    app.selection = None;
    app.pending_focus = Some((app.blocks[2].id.clone(), 0));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![paste(&copied)]);
    assert_eq!(
        contents(&app)[2..],
        [(Bul, 0, "Pan".into()), (Bul, 1, "Integral".into())]
    );
    Ok(())
}

#[test]
fn ime_composition_commits_text_without_splitting() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "Texto: ")], 0, 7)?;
    let preedit = |t: &str| {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: t.into(),
            active_range_chars: None,
        })
    };
    for step in ["n", "に", "にほ", "にほn"] {
        app_frame(&mut app, &ctx, vec![preedit(step)]);
    }
    // The user confirms with Enter: the IME turns it into a commit.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".into()))],
    );
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks.len(), 1, "no split during the composition");
    assert_eq!(
        app.blocks[0].content, "Texto: 日本",
        "no stray newline, no preedit left"
    );
    assert!(!app.ime_composing);
    // After the commit, Enter splits again as usual.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks.len(), 2);
    Ok(())
}

#[test]
fn a_stale_ime_flag_is_cleared_when_leaving_the_block() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ab")], 0, 2)?;
    app.ime_composing = true; // e.g. the IME never sent its end event
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(!app.ime_composing, "no block focused: flag reset");
    app.selection = None;
    app.pending_focus = Some((app.blocks[0].id.clone(), 2));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks.len(), 2, "Enter works again");
    Ok(())
}

#[test]
fn arrow_up_moves_inside_a_multi_line_block_first() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "arriba"), (Par, 0, "uno\ndos")], 1, 6)?;
    assert!(
        app.caret.as_ref().is_some_and(|c| !c.first_row),
        "on the second line"
    );
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        focused_block(&app, &ctx),
        Some(1),
        "still in the same block"
    );
    assert!(app.caret.as_ref().is_some_and(|c| c.first_row));
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0), "now it jumps");
    Ok(())
}

#[test]
fn tricky_unicode_typed_in_the_app_is_saved_exactly() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "")], 0, 0)?;
    let text = "e\u{301} 👨‍👩‍👧 🇪🇸 مرحبا שלום 日本語";
    app_frame(&mut app, &ctx, vec![egui::Event::Text(text.into())]);
    app.save_now()?;
    let page = app.selected.clone().expect("page");
    assert_eq!(app.db.list_blocks(&page)?[0].content, text);
    assert_eq!(app.db.search_pages("日本語")?.len(), 1);
    Ok(())
}

#[test]
fn exporting_writes_what_is_on_screen_and_offers_the_folder() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Bul, 0, "uno"), (Bul, 1, "dos")], 0, 3)?;
    type_into(&mut app, &ctx, "!"); // unsaved edit
    // Removed on drop; the in-memory app keeps no files open inside it.
    let tmp = TempFolder::new("silex-app-export")?;
    let dir = tmp.0.clone();
    app.export_current_page_to(&dir)?;
    let path = app.last_export.clone().expect("export offered");
    let text = std::fs::read_to_string(&path)?;
    assert!(text.contains("- uno\\!\n  - dos\n"), "{text}"); // "!" is escaped
    assert!(app.save.can_close(), "saved before exporting");
    Ok(())
}

/// Temporary folder removed on drop (best effort). Declare it before
/// anything that keeps files inside it open, so it is dropped last.
struct TempFolder(std::path::PathBuf);

impl TempFolder {
    fn new(prefix: &str) -> anyhow::Result<Self> {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A file dropped on the window (test double for egui's handle).
#[derive(Debug)]
struct TestDrop(std::path::PathBuf);

impl egui::DroppedFile for TestDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_frame(app: &mut SilexApp, ctx: &egui::Context, files: &[std::path::PathBuf]) {
    let input = egui::RawInput {
        dropped_files: files
            .iter()
            .map(|f| std::sync::Arc::new(TestDrop(f.clone())) as egui::DroppedFileHandle)
            .collect(),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
}

#[test]
fn dropping_markdown_files_imports_them_as_pages() -> anyhow::Result<()> {
    // Declared before the app: dropped after it, once its files are closed
    // (Windows cannot delete open files).
    let tmp = TempFolder::new("silex-drop")?;
    let dir = tmp.0.clone();
    let good = dir.join("receta.md");
    std::fs::write(
        &good,
        "# Pan casero\n\n- harina\n  - integral\n- [x] amasar\n",
    )?;
    let latin1 = dir.join("vieja.md");
    std::fs::write(&latin1, b"caf\xe9")?;
    let other = dir.join("foto.png");
    std::fs::write(&other, b"png")?;

    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    drop_frame(&mut app, &ctx, &[good, latin1, other.clone()]);
    let start = std::time::Instant::now();
    while app.import_job.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_frame(&mut app, &ctx, vec![]);
    }
    assert!(app.import_job.is_none(), "import finished");
    assert_eq!(app.title, "Pan casero", "the imported page is open");
    assert_eq!(
        contents(&app),
        [
            (Bul, 0, "harina".into()),
            (Bul, 1, "integral".into()),
            (BlockKind::Todo, 0, "amasar".into())
        ]
    );
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.contains("Importada(s) 1"), "{notice}");
    assert!(
        notice.contains("vieja.md") && notice.contains("UTF-8"),
        "{notice}"
    );
    assert!(notice.contains("1 ignorado"), "{notice}");

    // Only non-Markdown files: nothing to import, just the message.
    drop_frame(&mut app, &ctx, &[other]);
    assert!(app.import_job.is_none());
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.contains("1 ignorado"), "{notice}");
    Ok(())
}

#[test]
fn workspace_export_runs_in_the_background_and_saves_first() -> anyhow::Result<()> {
    // Declared before the app: dropped after it, once its files are closed
    // (Windows cannot delete open files).
    let tmp = TempFolder::new("silex-app-ws")?;
    let dir = tmp.0.clone();
    let mut app = SilexApp::new(Database::open(dir.join("silex.sqlite3"))?)?;
    // A file database would start the automatic backup on the first frame,
    // still writing into `dir` while the test cleans up: not under test here.
    app.startup_backup_pending = false;
    app.title = "Título sin guardar".into();
    app.mark_dirty();
    let ctx = egui::Context::default();
    app.start_workspace_export(&ctx, dir.join("exportaciones"))?;
    assert!(app.save.can_close(), "saved before exporting");
    let start = std::time::Instant::now();
    while app.export_job.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_frame(&mut app, &ctx, vec![]);
    }
    let folder = app.last_export.clone().expect("export finished");
    assert!(folder.join("Título sin guardar.md").exists());
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("1 página"))
    );

    // In-memory databases cannot be exported from another thread.
    let mut memory = SilexApp::new(Database::open_in_memory()?)?;
    assert!(memory.start_workspace_export(&ctx, dir.join("x")).is_err());
    Ok(())
}

#[test]
fn a_new_page_opens_with_its_title_ready_to_type() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    app.create_root_page("Sin título")?;
    app_frame(&mut app, &ctx, vec![]);
    let page = app.selected.clone().expect("new page open");
    let title = field_id(app.doc_generation, "title", &page);
    assert!(ctx.memory(|m| m.has_focus(title)), "title focused");
    app_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
    assert_eq!(app.title, "Sin título!");
    Ok(())
}

#[test]
fn field_ids_follow_block_and_generation() {
    assert_eq!(field_id(0, "block", "a"), field_id(0, "block", "a"));
    assert_ne!(field_id(0, "block", "a"), field_id(0, "block", "b"));
    assert_ne!(field_id(0, "block", "a"), field_id(1, "block", "a"));
    assert_ne!(field_id(0, "block", "a"), field_id(0, "title", "a"));
}

#[test]
fn undo_stays_with_its_block_when_blocks_move() {
    let ctx = egui::Context::default();
    let a = field_id(0, "block", "a");
    let mut blocks = vec![block("a"), block("b")];

    frame(&ctx, 0.0, vec![], &mut blocks, Some(a));
    frame(&ctx, 0.1, vec![], &mut blocks, None);
    assert!(frame(&ctx, 0.2, type_text("hola"), &mut blocks, None));
    // Let egui's undoer record the new state.
    frame(&ctx, 2.0, vec![], &mut blocks, None);
    frame(&ctx, 4.0, vec![], &mut blocks, None);
    assert_eq!(blocks[0].content, "hola");

    // A new block appears before "a": positions shift by one.
    blocks.insert(0, block("nuevo"));
    frame(&ctx, 4.1, vec![], &mut blocks, Some(a));
    frame(&ctx, 4.2, vec![], &mut blocks, None);
    let changed = frame(&ctx, 4.3, undo(), &mut blocks, None);

    assert!(changed, "undo must mark the document dirty");
    assert_eq!(blocks[1].id, "a");
    assert_eq!(blocks[1].content, "", "undo reverts block a");
    assert_eq!(blocks[0].content, "", "the new block is untouched");
    assert_eq!(blocks[2].content, "", "block b is untouched");
}

#[test]
fn sidebar_width_is_stable_across_frames() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    // Nested pages: tree rows with expand arrows (icon buttons).
    let parent = app
        .db
        .create_page("Una página con un título bastante largo", None)?;
    app.db.create_page("Hija", Some(&parent))?;
    app.expanded.insert(parent);
    app.reload_pages()?;
    let ctx = egui::Context::default();
    let mut widths = Vec::new();
    for frame in 0..30 {
        // 0–9 at rest, 10–19 pointer over a sidebar row, 20–29 a block menu open.
        let events = if (10..20).contains(&frame) {
            // Over the first tree row (shows its "+" and "…" buttons).
            vec![egui::Event::PointerMoved(egui::pos2(120.0, 222.0))]
        } else {
            vec![]
        };
        if frame == 20 {
            app.blocks.push(Block {
                id: "b".into(),
                page_id: app.selected.clone().unwrap_or_default(),
                kind: BlockKind::Paragraph,
                content: "texto".into(),
                checked: false,
                position: 0,
                indent: 0,
            });
            egui::Popup::open_id(&ctx, super::blocks::handle_menu_id("b"));
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            events,
            ..Default::default()
        };
        let mut id = egui::Id::NULL;
        let mut output = ctx.run_ui(input, |ui| {
            // Same id the panel derives from its parent Ui.
            id = ui.make_persistent_id("silex_sidebar");
            app.update_ui(ui);
        });
        output.textures_delta.clear();
        if let Some(state) = egui::containers::panel::PanelState::load(&ctx, id) {
            widths.push(state.size().x.round());
        }
    }
    assert!(!widths.is_empty(), "panel state found");
    let first = widths[0];
    assert!(
        widths.iter().all(|w| *w == first),
        "sidebar width drifts: {widths:?}"
    );
    Ok(())
}

#[test]
fn the_window_icon_is_embedded_and_readable() {
    let icon = super::window_icon().expect("embedded PNG decodes");
    assert_eq!((icon.width, icon.height), (512, 512));
    assert_eq!(icon.rgba.len(), 512 * 512 * 4);
}

// ---- Databases (H6.1, ADR 0006) ----

/// App with a new, open database and the egui context it was drawn with.
fn database_app() -> anyhow::Result<(SilexApp, egui::Context, String)> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    app.create_database(None)?;
    let id = app.selected.clone().expect("database open");
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, id))
}

fn row_titles(app: &SilexApp) -> Vec<String> {
    app.database
        .as_ref()
        .map(|d| d.rows.iter().map(|r| r.title.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn a_new_database_opens_with_an_empty_table() -> anyhow::Result<()> {
    let (mut app, ctx, id) = database_app()?;
    assert_eq!(
        app.pages.iter().find(|p| p.id == id).map(|p| p.kind),
        Some(PageKind::Database)
    );
    let painted = app_frame(&mut app, &ctx, vec![]);
    let title = field_id(app.doc_generation, "title", &id);
    assert!(ctx.memory(|m| m.has_focus(title)), "title ready to type");
    assert!(app.title.is_empty());
    for text in [
        "Tabla",
        "Nombre",
        "0 filas",
        "Todavía no hay filas.",
        "Nueva fila",
    ] {
        assert!(painted.iter().any(|t| t == text), "{text:?} in {painted:?}");
    }
    // The database is in the sidebar, with no rows below it.
    assert!(app.tree.iter().any(|r| r.page.id == id && !r.has_children));
    Ok(())
}

#[test]
fn a_new_row_takes_its_name_and_stays_out_of_the_sidebar() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, id) = database_app()?;
    app.add_row(&id)?;
    let row = app
        .database
        .as_ref()
        .map(|d| d.rows[0].id.clone())
        .expect("a row");
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.memory(|m| m.has_focus(row_name_id(&row))),
        "name ready to type"
    );

    app_frame(&mut app, &ctx, vec![egui::Event::Text("Rayuela".into())]);
    assert_eq!(row_titles(&app), ["Rayuela"]);
    assert_eq!(
        app.db.get_page(&row)?.map(|p| p.title).as_deref(),
        Some(""),
        "not yet"
    );
    // Enter leaves the cell: the name is saved.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.db.get_page(&row)?.map(|p| p.title).as_deref(),
        Some("Rayuela")
    );
    assert!(app.database.as_ref().is_some_and(|d| d.dirty.is_empty()));
    assert!(
        app.tree.iter().all(|r| r.page.id != row),
        "rows stay out of the tree"
    );
    assert!(app.pages.iter().any(|p| p.id == row), "but they are pages");
    assert_eq!(app.selected.as_deref(), Some(id.as_str()));
    Ok(())
}

#[test]
fn an_unsaved_row_name_is_saved_when_leaving_the_database() -> anyhow::Result<()> {
    let (mut app, ctx, id) = database_app()?;
    app.add_row(&id)?;
    let row = app
        .database
        .as_ref()
        .map(|d| d.rows[0].id.clone())
        .expect("a row");
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("Pedro Páramo".into())],
    );
    // Straight to another page, with the cell still focused.
    let other = app.db.create_page("Notas", None)?;
    app.reload_pages()?;
    app.open_page(other)?;
    assert!(app.database.is_none());
    assert_eq!(
        app.db.get_page(&row)?.map(|p| p.title).as_deref(),
        Some("Pedro Páramo")
    );
    Ok(())
}

#[test]
fn rows_come_from_shortcuts_and_leave_through_the_trash() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, id) = database_app()?;
    // Cmd+Shift+N in a database adds a row (not a subpage page).
    app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND | M::SHIFT)]);
    app_frame(&mut app, &ctx, vec![key(Key::N, M::COMMAND | M::SHIFT)]);
    assert_eq!(row_titles(&app).len(), 2);
    assert_eq!(
        app.selected.as_deref(),
        Some(id.as_str()),
        "still in the table"
    );
    // The palette's «Nueva fila» does the same.
    app.run_command(&ctx, silex_desktop::palette::Command::NewRow)?;
    assert_eq!(row_titles(&app).len(), 3);

    let row = app
        .database
        .as_ref()
        .map(|d| d.rows[0].id.clone())
        .expect("a row");
    app.apply(RowAction::Trash(row.clone()))?;
    assert_eq!(row_titles(&app).len(), 2, "gone from the table");
    assert_eq!(
        app.selected.as_deref(),
        Some(id.as_str()),
        "the database stays open"
    );
    app.restore_from_trash(&row)?;
    assert_eq!(
        app.selected.as_deref(),
        Some(row.as_str()),
        "restored row opens as a page"
    );
    let parent = app
        .pages
        .iter()
        .find(|p| p.id == row)
        .and_then(|p| p.parent_id.clone());
    assert_eq!(parent.as_deref(), Some(id.as_str()), "back in its database");
    Ok(())
}

#[test]
fn exporting_a_database_writes_its_table() -> anyhow::Result<()> {
    let (mut app, ctx, id) = database_app()?;
    app.title = "Lecturas".into();
    app.mark_dirty();
    app.db.create_row(&id, "Rayuela")?;
    app.reload_pages()?;
    app_frame(&mut app, &ctx, vec![]);
    let tmp = TempFolder::new("silex-app-db-export")?;
    app.export_current_page_to(&tmp.0)?;
    let path = app.last_export.clone().expect("exported");
    assert_eq!(
        std::fs::read_to_string(path)?,
        "# Lecturas\n\n| Nombre |\n| --- |\n| Rayuela |\n"
    );
    Ok(())
}

/// Manual: `cargo test --release --bin silex-desktop database_scale -- --ignored --nocapture`.
/// Prints timings (ADR 0006 targets: < 50 ms to open, < 16 ms per frame)
/// with 10 properties, two of each type, and a value in every cell.
#[test]
#[ignore = "scale smoke test; run manually"]
fn database_scale_smoke() -> anyhow::Result<()> {
    for count in [1_000, 10_000] {
        let mut db = Database::open_in_memory()?;
        let base = db.create_database("Grande", None)?;
        let mut properties = Vec::new();
        for round in 0..2 {
            for kind in PropertyKind::ALL {
                let id = db.add_property(&base, &format!("{} {round}", kind.label()), kind)?;
                let option = match kind {
                    PropertyKind::Select => {
                        Some(db.add_select_option(&id, "Hecho", OptionColor::Green)?)
                    }
                    _ => None,
                };
                properties.push((id, kind, option));
            }
        }
        let date = Date::parse("2026-10-19").map(CellValue::Date);
        for i in 0..count {
            let row = db.create_row(&base, &format!("Fila {i}"))?;
            for (property, kind, option) in &properties {
                let value = match kind {
                    PropertyKind::Text => Some(CellValue::Text(format!("Texto {i}"))),
                    PropertyKind::Number => Some(CellValue::Number(i as f64)),
                    PropertyKind::Checkbox => Some(CellValue::Checkbox(i % 2 == 0)),
                    PropertyKind::Date => date.clone(),
                    PropertyKind::Select => option.clone().map(CellValue::Select),
                };
                db.set_value(&row, property, value.as_ref())?;
            }
        }
        let t = std::time::Instant::now();
        let values = db.database_values(&base)?;
        println!(
            "{count} filas: leer {} celdas {:?}",
            values.values().map(|cells| cells.len()).sum::<usize>(),
            t.elapsed()
        );
        let mut app = SilexApp::new(db)?;
        let t = std::time::Instant::now();
        app.open_page(base.clone())?;
        println!("{count} filas: abrir la base {:?}", t.elapsed());
        let ctx = egui::Context::default();
        app_frame(&mut app, &ctx, vec![]);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            app_frame(&mut app, &ctx, vec![]);
        }
        println!("{count} filas: un frame de la tabla {:?}", t.elapsed() / 10);
        // A board (H6.5) with every card in one column: the worst case.
        app.add_view(silex_desktop::model::ViewKind::Board)?;
        sized_frame(&mut app, &ctx, vec![]);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            sized_frame(&mut app, &ctx, vec![]);
        }
        println!("{count} filas: un frame del tablero {:?}", t.elapsed() / 10);
        // A text filter and a number sort, as applied on each key typed.
        let (text, number) = (properties[0].0.clone(), properties[1].0.clone());
        let t = std::time::Instant::now();
        change_view(&mut app, |c| {
            c.filters.push(silex_desktop::view::Filter {
                property: text,
                op: silex_desktop::view::FilterOp::Contains,
                value: "9".into(),
            });
            c.sorts.push(silex_desktop::view::Sort {
                property: number,
                direction: silex_desktop::view::Direction::Descending,
            });
        });
        let shown = app.database.as_ref().map_or(0, |d| d.order.len());
        println!(
            "{count} filas: filtrar y ordenar ({shown} filas) {:?}",
            t.elapsed()
        );
    }
    Ok(())
}

// ---- Typed properties (H6.2, ADR 0006) ----

/// A database with one row, «Rayuela», and the given properties.
fn table_app(
    properties: &[(PropertyKind, &str)],
) -> anyhow::Result<(SilexApp, egui::Context, String, Vec<String>)> {
    let (mut app, ctx, base) = database_app()?;
    let row = app.db.create_row(&base, "Rayuela")?;
    app.reload_pages()?;
    let ids = properties
        .iter()
        .map(|(kind, name)| app.add_property(*kind, name))
        .collect::<anyhow::Result<Vec<_>>>()?;
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, row, ids))
}

/// The value of a cell as stored in SQLite.
fn stored(app: &SilexApp, row: &str, property: &str) -> anyhow::Result<Option<CellValue>> {
    let base = app
        .database
        .as_ref()
        .map(|d| d.id.clone())
        .unwrap_or_default();
    Ok(app
        .db
        .database_values(&base)?
        .get(row)
        .and_then(|cells| cells.get(property))
        .cloned())
}

/// Puts the keyboard on a cell and "clicks" it with Enter, as a keyboard
/// user would. Two more frames let a text field or menu take the focus.
fn press_cell(app: &mut SilexApp, ctx: &egui::Context, focus: egui::Id) {
    use egui::{Key, Modifiers as M};
    ctx.memory_mut(|m| m.request_focus(focus));
    app_frame(app, ctx, vec![]);
    app_frame(app, ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(app, ctx, vec![]);
    app_frame(app, ctx, vec![]);
}

#[test]
fn numbers_and_dates_are_typed_in_their_cells() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) =
        table_app(&[(PropertyKind::Number, "Páginas"), (PropertyKind::Date, "")])?;
    let (paginas, fecha) = (&ids[0], &ids[1]);
    let names: Vec<String> = app
        .database
        .as_ref()
        .map(|d| d.properties.iter().map(|p| p.name.clone()).collect())
        .unwrap_or_default();
    assert_eq!(
        names,
        ["Páginas", "Fecha"],
        "an empty name takes the type's"
    );

    press_cell(&mut app, &ctx, cell_id(&row, paginas));
    assert!(ctx.memory(|m| m.has_focus(cell_edit_id(&row, paginas))));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("736,5".into())]);
    assert_eq!(stored(&app, &row, paginas)?, None, "not before leaving");
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(stored(&app, &row, paginas)?, Some(CellValue::Number(736.5)));
    app_frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.memory(|m| m.has_focus(cell_id(&row, paginas))),
        "the keyboard stays on the cell"
    );
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "736,5"), "{painted:?}");

    // A date that does not exist is not saved; the calendar says why.
    press_cell(&mut app, &ctx, cell_id(&row, fecha));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("30/02/2026".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert_eq!(stored(&app, &row, fecha)?, None);
    assert!(
        painted
            .iter()
            .any(|t| t == "Escribe una fecha como 2026-10-19 o 19/10/2026."),
        "{painted:?}"
    );
    // Escape closes the calendar without saving.
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(!egui::Popup::is_id_open(&ctx, date_menu_id(&row, fecha)));
    assert_eq!(stored(&app, &row, fecha)?, None);

    // D/M/AAAA is read as a date and stored as AAAA-MM-DD.
    press_cell(&mut app, &ctx, cell_id(&row, fecha));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("19/10/2026".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let date = Date::parse("2026-10-19").map(CellValue::Date);
    assert_eq!(stored(&app, &row, fecha)?, date);

    // Escape drops what was typed.
    press_cell(&mut app, &ctx, cell_id(&row, paginas));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("9".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(stored(&app, &row, paginas)?, Some(CellValue::Number(736.5)));
    assert!(app.notice.is_none(), "{:?}", app.notice);
    Ok(())
}

#[test]
fn a_cell_being_typed_in_is_saved_when_leaving_the_database() -> anyhow::Result<()> {
    let (mut app, ctx, row, ids) = table_app(&[(PropertyKind::Text, "Autor")])?;
    press_cell(&mut app, &ctx, cell_id(&row, &ids[0]));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("Cortázar".into())]);
    // Straight to another page, with the cell still focused.
    let base = app.selected.clone().unwrap_or_default();
    let other = app.db.create_page("Notas", None)?;
    app.reload_pages()?;
    app.open_page(other)?;
    assert!(app.database.is_none());
    app.open_page(base)?;
    assert_eq!(
        stored(&app, &row, &ids[0])?,
        Some(CellValue::Text("Cortázar".into()))
    );
    Ok(())
}

#[test]
fn checkboxes_tick_and_selects_create_and_reuse_options() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) = table_app(&[
        (PropertyKind::Checkbox, "Leído"),
        (PropertyKind::Select, "Estado"),
    ])?;
    let (leido, estado) = (&ids[0], &ids[1]);

    press_cell(&mut app, &ctx, cell_id(&row, leido));
    assert_eq!(stored(&app, &row, leido)?, Some(CellValue::Checkbox(true)));
    press_cell(&mut app, &ctx, cell_id(&row, leido));
    assert_eq!(stored(&app, &row, leido)?, None, "unticked is empty");

    // Enter in the select menu creates the option typed, with a color.
    press_cell(&mut app, &ctx, cell_id(&row, estado));
    assert!(egui::Popup::is_id_open(&ctx, select_menu_id(&row, estado)));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("Leído".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    let options = |app: &SilexApp| -> Vec<SelectOption> {
        app.database
            .as_ref()
            .and_then(|d| d.properties.iter().find(|p| &p.id == estado))
            .map(|p| p.options.clone())
            .unwrap_or_default()
    };
    let created = options(&app);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, "Leído");
    assert_eq!(created[0].color, OptionColor::Blue);
    assert_eq!(
        stored(&app, &row, estado)?,
        Some(CellValue::Select(created[0].id.clone()))
    );
    assert!(!egui::Popup::is_id_open(&ctx, select_menu_id(&row, estado)));

    // Typing part of a name and Enter picks the existing option.
    let other = app
        .db
        .create_row(&app.selected.clone().unwrap_or_default(), "Ficciones")?;
    app.reload_pages()?;
    app_frame(&mut app, &ctx, vec![]);
    press_cell(&mut app, &ctx, cell_id(&other, estado));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("lei".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(options(&app).len(), 1, "no duplicate");
    assert_eq!(
        stored(&app, &other, estado)?,
        Some(CellValue::Select(created[0].id.clone()))
    );
    Ok(())
}

#[test]
fn properties_are_renamed_in_their_menu_deleted_and_restored() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) = table_app(&[
        (PropertyKind::Text, "Autor"),
        (PropertyKind::Number, "Páginas"),
    ])?;
    let autor = &ids[0];
    app.set_cell(&row, autor, Some(CellValue::Text("Cortázar".into())))?;

    // The header opens the menu with the name ready to change.
    press_cell(&mut app, &ctx, property_header_id(autor));
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("Autora".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    let base = app.selected.clone().unwrap_or_default();
    let names = |app: &SilexApp| -> anyhow::Result<Vec<String>> {
        Ok(app
            .db
            .database_properties(&base)?
            .into_iter()
            .map(|p| p.name)
            .collect())
    };
    assert_eq!(names(&app)?, ["Autora", "Páginas"]);

    // Escape leaves the name as it was.
    press_cell(&mut app, &ctx, property_header_id(autor));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("XX".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(names(&app)?, ["Autora", "Páginas"]);

    app.delete_property(autor)?;
    assert_eq!(names(&app)?, ["Páginas"]);
    let notice = app.notice.take().unwrap_or_default();
    assert!(notice.contains("Propiedades borradas"), "{notice}");
    assert!(app.database.as_ref().is_some_and(|d| d.deleted.len() == 1));

    app.restore_property(autor)?;
    assert_eq!(
        names(&app)?,
        ["Páginas", "Autora"],
        "back as the last column"
    );
    assert_eq!(
        stored(&app, &row, autor)?,
        Some(CellValue::Text("Cortázar".into())),
        "with its values"
    );
    Ok(())
}

// ---- H6.2b: a row's page and the arrow keys in the table ----

#[test]
fn a_rows_page_shows_and_edits_its_properties() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) = table_app(&[
        (PropertyKind::Number, "Páginas"),
        (PropertyKind::Select, "Estado"),
    ])?;
    let (paginas, estado) = (&ids[0], &ids[1]);
    app.set_cell(&row, paginas, Some(CellValue::Number(736.0)))?;
    let base = app.selected.clone().unwrap_or_default();

    app.open_page(row.clone())?;
    assert!(app.database.is_none());
    let painted = app_frame(&mut app, &ctx, vec![]);
    for text in ["Páginas", "736", "Estado", "Vacío"] {
        assert!(painted.iter().any(|t| t == text), "{text:?} in {painted:?}");
    }

    // Edited like a cell of the table, and saved at once.
    press_cell(&mut app, &ctx, cell_id(&row, paginas));
    assert!(ctx.memory(|m| m.has_focus(cell_edit_id(&row, paginas))));
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("737".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        app.db.row_values(&row)?.get(paginas),
        Some(&CellValue::Number(737.0))
    );
    press_cell(&mut app, &ctx, cell_id(&row, estado));
    app_frame(&mut app, &ctx, vec![egui::Event::Text("Leído".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(matches!(
        app.db.row_values(&row)?.get(estado),
        Some(CellValue::Select(_))
    ));

    // Typing and leaving the page saves too.
    press_cell(&mut app, &ctx, cell_id(&row, paginas));
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("10".into())]);
    app.open_page(base.clone())?;
    let values = app
        .database
        .as_ref()
        .map(|d| d.values.clone())
        .unwrap_or_default();
    assert_eq!(
        values.get(&row).and_then(|cells| cells.get(paginas)),
        Some(&CellValue::Number(10.0)),
        "the table shows what the page saved"
    );

    // A normal page has no properties.
    let notas = app.db.create_page("Notas", None)?;
    app.reload_pages()?;
    app.open_page(notas)?;
    assert!(app.row_page.is_none());
    Ok(())
}

/// One frame in a window of the default size, so rows can be out of sight.
fn sized_frame(app: &mut SilexApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 760.0),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
}

#[test]
fn arrow_keys_move_between_cells_and_follow_out_of_sight() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, first, ids) = table_app(&[
        (PropertyKind::Text, "Autor"),
        (PropertyKind::Number, "Páginas"),
    ])?;
    let base = app.selected.clone().unwrap_or_default();
    let mut rows = vec![first];
    for i in 1..60 {
        rows.push(app.db.create_row(&base, &format!("Fila {i}"))?);
    }
    app.reload_pages()?;
    sized_frame(&mut app, &ctx, vec![]);
    let focused = |ctx: &egui::Context, id: egui::Id| ctx.memory(|m| m.has_focus(id));
    let arrow = |app: &mut SilexApp, ctx: &egui::Context, key_: Key| {
        sized_frame(app, ctx, vec![key(key_, M::NONE)]);
        sized_frame(app, ctx, vec![]);
    };

    ctx.memory_mut(|m| m.request_focus(cell_id(&rows[0], &ids[0])));
    sized_frame(&mut app, &ctx, vec![]);
    sized_frame(&mut app, &ctx, vec![]);
    arrow(&mut app, &ctx, Key::ArrowRight);
    assert!(focused(&ctx, cell_id(&rows[0], &ids[1])), "→");
    arrow(&mut app, &ctx, Key::ArrowRight);
    assert!(
        focused(&ctx, cell_id(&rows[0], &ids[1])),
        "stays in the last column"
    );
    arrow(&mut app, &ctx, Key::ArrowDown);
    assert!(focused(&ctx, cell_id(&rows[1], &ids[1])), "↓");
    arrow(&mut app, &ctx, Key::ArrowLeft);
    arrow(&mut app, &ctx, Key::ArrowLeft);
    assert!(focused(&ctx, row_name_id(&rows[1])), "← reaches the name");
    arrow(&mut app, &ctx, Key::ArrowUp);
    assert!(focused(&ctx, row_name_id(&rows[0])), "↑ from a name");
    arrow(&mut app, &ctx, Key::ArrowUp);
    assert!(
        focused(&ctx, row_name_id(&rows[0])),
        "stays in the first row"
    );

    // In a name, → moves in the text; at its end, on to the first property.
    arrow(&mut app, &ctx, Key::Home);
    arrow(&mut app, &ctx, Key::ArrowRight);
    assert!(focused(&ctx, row_name_id(&rows[0])), "still in «Rayuela»");
    arrow(&mut app, &ctx, Key::End);
    arrow(&mut app, &ctx, Key::ArrowRight);
    assert!(
        focused(&ctx, cell_id(&rows[0], &ids[0])),
        "→ at the end of the name"
    );

    // Down past the rows in sight: the table scrolls to the focused row.
    for _ in 0..45 {
        arrow(&mut app, &ctx, Key::ArrowDown);
    }
    assert!(focused(&ctx, cell_id(&rows[45], &ids[0])), "row 45");
    let scrolled = app.database.as_ref().map_or(0.0, |d| d.scroll.y);
    assert!(scrolled > 0.0, "scrolled to it ({scrolled})");
    // Enter still edits the cell the keys reached.
    sized_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    sized_frame(&mut app, &ctx, vec![]);
    assert!(focused(&ctx, cell_edit_id(&rows[45], &ids[0])));
    Ok(())
}

// ---- H6.3a: sort, filters and hidden columns of the view ----

/// Changes the view on show as its menus would.
fn change_view(app: &mut SilexApp, change: impl FnOnce(&mut silex_desktop::view::ViewConfig)) {
    if let Some(state) = app.database.as_mut()
        && let Some(view) = state.views.get_mut(state.view)
    {
        change(&mut view.config);
        state.view_changed();
    }
}

fn shown_titles(app: &SilexApp) -> Vec<String> {
    app.database
        .as_ref()
        .map(|d| d.order.iter().map(|&i| d.rows[i].title.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn the_view_sorts_filters_and_remembers() -> anyhow::Result<()> {
    use silex_desktop::view::{Direction, Filter, FilterOp, NAME, Sort};
    let (mut app, ctx, rayuela, ids) = table_app(&[(PropertyKind::Number, "Páginas")])?;
    let paginas = ids[0].clone();
    let base = app.selected.clone().unwrap_or_default();
    let pedro = app.db.create_row(&base, "Pedro Páramo")?;
    let ficciones = app.db.create_row(&base, "Ficciones")?;
    app.reload_pages()?;
    app.set_cell(&rayuela, &paginas, Some(CellValue::Number(736.0)))?;
    app.set_cell(&pedro, &paginas, Some(CellValue::Number(124.0)))?;
    app.set_cell(&ficciones, &paginas, Some(CellValue::Number(174.0)))?;
    assert_eq!(shown_titles(&app), ["Rayuela", "Pedro Páramo", "Ficciones"]);

    change_view(&mut app, |c| {
        c.sorts.push(Sort {
            property: NAME.into(),
            direction: Direction::Ascending,
        });
    });
    assert_eq!(shown_titles(&app), ["Ficciones", "Pedro Páramo", "Rayuela"]);
    change_view(&mut app, |c| {
        c.filters.push(Filter {
            property: paginas.clone(),
            op: FilterOp::Greater,
            value: "150".into(),
        });
    });
    assert_eq!(shown_titles(&app), ["Ficciones", "Rayuela"]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "2 de 3 filas"), "{painted:?}");

    // Editing a cell does not move or hide its row until the view changes.
    app.set_cell(&ficciones, &paginas, Some(CellValue::Number(10.0)))?;
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(shown_titles(&app), ["Ficciones", "Rayuela"]);

    // A new row stays in sight, though the filter would hide it.
    app.add_row(&base)?;
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(shown_titles(&app), ["Ficciones", "Rayuela", ""]);

    // Saved with the database: opening it again applies it afresh.
    app_frame(&mut app, &ctx, vec![]);
    let notas = app.db.create_page("Notas", None)?;
    app.reload_pages()?;
    app.open_page(notas)?;
    app.open_page(base.clone())?;
    assert_eq!(shown_titles(&app), ["Rayuela"]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "1 de 4 filas"), "{painted:?}");

    // Nothing matches: the table says why it is empty.
    change_view(&mut app, |c| c.filters[0].value = "10000".into());
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t == "Ninguna fila coincide con los filtros."),
        "{painted:?}"
    );
    Ok(())
}

#[test]
fn hidden_columns_leave_the_table_and_the_arrow_keys() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    // (Not «Páginas»: the sidebar has a section with that name.)
    let (mut app, ctx, row, ids) = table_app(&[
        (PropertyKind::Text, "Autor"),
        (PropertyKind::Number, "Extensión"),
        (PropertyKind::Date, "Fecha"),
    ])?;
    change_view(&mut app, |c| c.set_hidden(&ids[1], true));
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Autor"));
    assert!(!painted.iter().any(|t| t == "Extensión"), "hidden");

    ctx.memory_mut(|m| m.request_focus(cell_id(&row, &ids[0])));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowRight, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.memory(|m| m.has_focus(cell_id(&row, &ids[2]))),
        "→ skips the hidden column"
    );
    // A hidden property is still on the row's page.
    app.open_page(row.clone())?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Extensión"));
    Ok(())
}

// ---- H6.3b: several views ----

fn view_names(app: &SilexApp) -> Vec<String> {
    app.database
        .as_ref()
        .map(|d| d.views.iter().map(|v| v.name.clone()).collect())
        .unwrap_or_default()
}

fn open_view(app: &SilexApp) -> String {
    app.database
        .as_ref()
        .and_then(|d| d.views.get(d.view).map(|v| v.name.clone()))
        .unwrap_or_default()
}

#[test]
fn each_view_keeps_its_own_rules() -> anyhow::Result<()> {
    use silex_desktop::view::{Filter, FilterOp, NAME};
    let (mut app, ctx, _, _) = table_app(&[])?;
    let base = app.selected.clone().unwrap_or_default();
    app.db.create_row(&base, "Pedro Páramo")?;
    app.reload_pages()?;

    app.add_view(silex_desktop::model::ViewKind::Table)?;
    assert_eq!(view_names(&app), ["Tabla", "Tabla 2"]);
    assert_eq!(open_view(&app), "Tabla 2", "a new view opens");
    change_view(&mut app, |c| {
        c.filters.push(Filter {
            property: NAME.into(),
            op: FilterOp::Contains,
            value: "pedro".into(),
        });
    });
    assert_eq!(shown_titles(&app), ["Pedro Páramo"]);

    app.switch_view(0)?;
    assert_eq!(
        shown_titles(&app),
        ["Rayuela", "Pedro Páramo"],
        "the first view has no filter"
    );
    // The filter was saved when leaving its view.
    assert_eq!(app.db.database_views(&base)?[1].config.filters.len(), 1);

    // A copy keeps the rules of what it copies.
    app.duplicate_view(1)?;
    assert_eq!(view_names(&app), ["Tabla", "Tabla 2", "Tabla 2 (copia)"]);
    assert_eq!(shown_titles(&app), ["Pedro Páramo"]);
    app.rename_view(
        &app.database
            .as_ref()
            .map(|d| d.views[2].id.clone())
            .unwrap_or_default(),
        "Pedro",
    )?;
    assert_eq!(open_view(&app), "Pedro");

    // The database opens again on the view last open.
    let notas = app.db.create_page("Notas", None)?;
    app.reload_pages()?;
    app.open_page(notas)?;
    app.open_page(base.clone())?;
    assert_eq!(open_view(&app), "Pedro");

    // Deleting the open view shows the one now in its place.
    let tabla2 = app
        .database
        .as_ref()
        .map(|d| d.views[1].id.clone())
        .unwrap_or_default();
    app.switch_view(1)?;
    app.delete_view(&tabla2)?;
    assert_eq!(view_names(&app), ["Tabla", "Pedro"]);
    assert_eq!(open_view(&app), "Pedro");
    assert!(app.notice.take().is_some_and(|n| n.contains("Tabla 2")));
    let pedro = app
        .database
        .as_ref()
        .map(|d| d.views[1].id.clone())
        .unwrap_or_default();
    app.delete_view(&pedro)?;
    let last = app
        .database
        .as_ref()
        .map(|d| d.views[0].id.clone())
        .unwrap_or_default();
    assert!(app.delete_view(&last).is_err(), "the last view stays");
    assert_eq!(view_names(&app), ["Tabla"]);
    app_frame(&mut app, &ctx, vec![]);
    Ok(())
}

// ---- H6.4: undo in the table ----

#[test]
fn the_table_undoes_and_redoes_its_changes() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) = table_app(&[(PropertyKind::Number, "Páginas")])?;
    let paginas = ids[0].clone();
    let base = app.selected.clone().unwrap_or_default();

    // A cell: back to empty, then forward again.
    app.set_cell(&row, &paginas, Some(CellValue::Number(736.0)))?;
    app.set_cell(&row, &paginas, Some(CellValue::Number(737.0)))?;
    app.undo_table();
    assert_eq!(
        stored(&app, &row, &paginas)?,
        Some(CellValue::Number(736.0))
    );
    app.undo_table();
    assert_eq!(stored(&app, &row, &paginas)?, None);
    app.redo_table();
    assert_eq!(
        stored(&app, &row, &paginas)?,
        Some(CellValue::Number(736.0))
    );

    // A name typed in its cell.
    ctx.memory_mut(|m| m.request_focus(row_name_id(&row)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::End, M::NONE)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text(" (1963)".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    let title = |app: &SilexApp| app.db.get_page(&row).ok().flatten().map(|p| p.title);
    assert_eq!(title(&app).as_deref(), Some("Rayuela (1963)"));
    // Cmd+Z with nothing focused is for the table.
    ctx.memory_mut(|m| m.surrender_focus(row_name_id(&row)));
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(title(&app).as_deref(), Some("Rayuela"));
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND | M::SHIFT)]);
    assert_eq!(title(&app).as_deref(), Some("Rayuela (1963)"));

    // A new row goes to the trash and comes back.
    app.add_row(&base)?;
    assert_eq!(row_titles(&app).len(), 2);
    app.undo_table();
    assert_eq!(row_titles(&app).len(), 1);
    assert_eq!(app.trash_count, 1);
    app.redo_table();
    assert_eq!(row_titles(&app).len(), 2);

    // A row sent to the trash from the table comes back with its values.
    app.apply_table_action(&base, TableAction::Row(RowAction::Trash(row.clone())))?;
    assert!(app.db.get_page(&row)?.is_none());
    app.undo_table();
    assert!(app.db.get_page(&row)?.is_some());
    assert_eq!(
        stored(&app, &row, &paginas)?,
        Some(CellValue::Number(736.0))
    );

    // A deleted property comes back.
    app.delete_property(&paginas)?;
    app.undo_table();
    assert_eq!(app.db.database_properties(&base)?.len(), 1);
    assert_eq!(
        stored(&app, &row, &paginas)?,
        Some(CellValue::Number(736.0))
    );
    // A new change clears what could be redone.
    app.set_cell(&row, &paginas, Some(CellValue::Number(1.0)))?;
    assert!(app.database.as_ref().is_some_and(|d| !d.history.can_redo()));
    Ok(())
}

#[test]
fn cmd_z_while_typing_in_a_cell_undoes_the_text_not_the_table() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, row, ids) = table_app(&[(PropertyKind::Text, "Autor")])?;
    app.set_cell(&row, &ids[0], Some(CellValue::Text("Cortázar".into())))?;
    press_cell(&mut app, &ctx, cell_id(&row, &ids[0]));
    app_frame(&mut app, &ctx, vec![key(Key::End, M::NONE)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("x".into())]);
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    // The table's change is still there to undo.
    assert!(app.database.as_ref().is_some_and(|d| d.history.can_undo()));
    assert_eq!(
        stored(&app, &row, &ids[0])?,
        Some(CellValue::Text("Cortázar".into()))
    );
    Ok(())
}

// ---- H6.5: board view ----

/// A database with «Estado» (Leyendo, Hecho) and three rows: Rayuela
/// (Leyendo), Ficciones (Hecho) and Aleph (none), open in a new board view.
/// Returns the rows and Estado's (property, Leyendo, Hecho) ids.
fn board_app() -> anyhow::Result<(SilexApp, egui::Context, Vec<String>, [String; 3])> {
    let (mut app, ctx, base) = database_app()?;
    let rows = ["Rayuela", "Ficciones", "Aleph"]
        .into_iter()
        .map(|title| app.db.create_row(&base, title))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let estado = app.add_property(PropertyKind::Select, "Estado")?;
    let leyendo = app
        .db
        .add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
    let hecho = app
        .db
        .add_select_option(&estado, "Hecho", OptionColor::Green)?;
    app.db
        .set_value(&rows[0], &estado, Some(&CellValue::Select(leyendo.clone())))?;
    app.db
        .set_value(&rows[1], &estado, Some(&CellValue::Select(hecho.clone())))?;
    app.reload_pages()?;
    app.open_page(base)?;
    app.add_view(silex_desktop::model::ViewKind::Board)?;
    sized_frame(&mut app, &ctx, vec![]);
    sized_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, rows, [estado, leyendo, hecho]))
}

/// The pointer presses at `from`, moves past the drag threshold, goes to
/// `to` and lets go there.
fn drag(app: &mut SilexApp, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    sized_frame(app, ctx, vec![egui::Event::PointerMoved(from)]);
    sized_frame(app, ctx, vec![button(from, true)]);
    sized_frame(
        app,
        ctx,
        vec![egui::Event::PointerMoved(from + egui::vec2(0.0, 24.0))],
    );
    sized_frame(app, ctx, vec![egui::Event::PointerMoved(to)]);
    sized_frame(app, ctx, vec![egui::Event::PointerMoved(to)]);
    sized_frame(app, ctx, vec![button(to, false)]);
    sized_frame(app, ctx, vec![]);
}

fn card_rect(ctx: &egui::Context, row: &str) -> egui::Rect {
    ctx.read_response(egui::Id::unique(("silex-card", row)))
        .map(|r| r.rect)
        .unwrap_or_else(|| panic!("card {row} drawn"))
}

#[test]
fn a_board_shows_a_column_per_option_and_a_drag_moves_the_card() -> anyhow::Result<()> {
    let (mut app, ctx, rows, [estado, leyendo, hecho]) = board_app()?;
    let state = app.database.as_ref().expect("database open");
    assert!(state.is_board());
    assert_eq!(
        state.config().and_then(|c| c.group_by.clone()),
        Some(estado.clone()),
        "a new board groups by the first select"
    );
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 760.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
    let painted = painted_text(&output.shapes);
    for text in [
        "Tablero",
        "Sin Estado",
        "Leyendo",
        "Hecho",
        "Rayuela",
        "Aleph",
        "Nueva",
    ] {
        assert!(
            painted.iter().any(|t| t.trim() == text),
            "{text:?} in {painted:?}"
        );
    }
    // Columns in order: none, Leyendo, Hecho.
    let (aleph, rayuela, ficciones) = (
        card_rect(&ctx, &rows[2]),
        card_rect(&ctx, &rows[0]),
        card_rect(&ctx, &rows[1]),
    );
    assert!(aleph.left() < rayuela.left() && rayuela.left() < ficciones.left());

    // Rayuela is dropped on the «Hecho» column.
    drag(&mut app, &ctx, rayuela.center(), ficciones.center());
    assert_eq!(
        stored(&app, &rows[0], &estado)?,
        Some(CellValue::Select(hecho.clone()))
    );
    sized_frame(&mut app, &ctx, vec![]);
    assert!(card_rect(&ctx, &rows[0]).left() > rayuela.left() + 100.0);
    // Dropped where it was: nothing changes, nothing to undo.
    let at = card_rect(&ctx, &rows[1]).center();
    drag(&mut app, &ctx, at, at + egui::vec2(0.0, 30.0));
    assert_eq!(
        stored(&app, &rows[1], &estado)?,
        Some(CellValue::Select(hecho.clone()))
    );
    // One undo puts it back.
    app.undo_table();
    assert_eq!(
        stored(&app, &rows[0], &estado)?,
        Some(CellValue::Select(leyendo))
    );
    // A card dropped on the first column loses its value. (Rects are read
    // one frame late.)
    sized_frame(&mut app, &ctx, vec![]);
    sized_frame(&mut app, &ctx, vec![]);
    let (from, to) = (
        card_rect(&ctx, &rows[1]).center(),
        card_rect(&ctx, &rows[2]).center(),
    );
    drag(&mut app, &ctx, from, to);
    assert_eq!(stored(&app, &rows[1], &estado)?, None);
    Ok(())
}

#[test]
fn a_click_on_a_card_opens_its_row() -> anyhow::Result<()> {
    let (mut app, ctx, rows, _) = board_app()?;
    let at = card_rect(&ctx, &rows[0]).center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    sized_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
    sized_frame(&mut app, &ctx, vec![button(true)]);
    sized_frame(&mut app, &ctx, vec![button(false)]);
    assert_eq!(app.selected.as_deref(), Some(rows[0].as_str()));
    Ok(())
}

#[test]
fn a_new_card_takes_its_column_and_its_name() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, _, [estado, _, hecho]) = board_app()?;
    let base = app.selected.clone().unwrap_or_default();
    app.apply_table_action(&base, TableAction::AddCard(Some(hecho.clone())))?;
    let row = app
        .database
        .as_ref()
        .and_then(|d| d.rows.last())
        .map(|r| r.id.clone())
        .expect("a new row");
    assert_eq!(stored(&app, &row, &estado)?, Some(CellValue::Select(hecho)));
    sized_frame(&mut app, &ctx, vec![]);
    sized_frame(&mut app, &ctx, vec![]);
    assert!(
        ctx.memory(|m| m.has_focus(row_name_id(&row))),
        "name ready to type"
    );
    sized_frame(&mut app, &ctx, vec![egui::Event::Text("Bestiario".into())]);
    sized_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    sized_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.db.get_page(&row)?.map(|p| p.title).as_deref(),
        Some("Bestiario")
    );
    // A new card in the first column has no value.
    app.apply_table_action(&base, TableAction::AddCard(None))?;
    let empty = app
        .database
        .as_ref()
        .and_then(|d| d.rows.last())
        .map(|r| r.id.clone())
        .expect("another row");
    assert_eq!(stored(&app, &empty, &estado)?, None);
    // Undo takes the card away in one step.
    app.undo_table();
    assert_eq!(app.trash_count, 1);
    assert!(
        app.database
            .as_ref()
            .is_some_and(|d| d.rows.iter().all(|r| r.id != empty))
    );
    Ok(())
}

#[test]
fn a_new_group_is_a_new_option_and_a_board_without_selects_offers_one() -> anyhow::Result<()> {
    let (mut app, ctx, _, [estado, ..]) = board_app()?;
    let base = app.selected.clone().unwrap_or_default();
    app.apply_table_action(
        &base,
        TableAction::AddOption(estado.clone(), "  Abandonado ".into()),
    )?;
    app.apply_table_action(&base, TableAction::AddOption(estado.clone(), " ".into()))?;
    let names: Vec<String> = app
        .db
        .database_properties(&base)?
        .into_iter()
        .find(|p| p.id == estado)
        .map(|p| p.options.into_iter().map(|o| o.name).collect())
        .unwrap_or_default();
    assert_eq!(names, ["Leyendo", "Hecho", "Abandonado"]);

    // Without a select property the board says how to get one.
    app.delete_property(&estado)?;
    let input = egui::RawInput::default();
    let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
    output.textures_delta.clear();
    let painted = painted_text(&output.shapes);
    assert!(
        painted
            .iter()
            .any(|t| t.contains("Añadir la propiedad «Estado»")),
        "{painted:?}"
    );
    app.apply_table_action(
        &base,
        TableAction::AddProperty(PropertyKind::Select, "Estado".into()),
    )?;
    let state = app.database.as_ref().expect("open");
    let group = state
        .config()
        .and_then(|c| silex_desktop::view::board_property(&state.properties, c));
    assert_eq!(group.map(|p| p.name.as_str()), Some("Estado"));
    Ok(())
}

#[test]
fn a_duplicated_board_is_a_board() -> anyhow::Result<()> {
    use silex_desktop::model::ViewKind;
    let (mut app, _, _, [estado, ..]) = board_app()?;
    let index = app.database.as_ref().map(|d| d.view).unwrap_or_default();
    app.duplicate_view(index)?;
    let state = app.database.as_ref().expect("open");
    let copy = &state.views[state.view];
    assert_eq!(copy.kind, ViewKind::Board);
    assert_eq!(copy.name, "Tablero (copia)");
    assert_eq!(copy.config.group_by.as_deref(), Some(estado.as_str()));
    Ok(())
}
