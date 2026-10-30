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
        attachment: None,
        link: None,
        marks: Vec::new(),
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
                    attachment: None,
                    preview: None,
                    link: None,
                    found: Vec::new(),
                    reveal: false,
                    link_edit: None,
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
            marks: Vec::new(),
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
    assert_eq!(
        menu.matches,
        [silex_desktop::editor::SlashItem::Kind(BlockKind::Heading2)]
    );
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
    app.export_current_page_to(&ctx, &dir)?;
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
    assert!(notice.contains("Se adjuntó 1 archivo."), "{notice}");
    // The other file was attached to the page open when it was dropped.
    let first = app
        .pages
        .iter()
        .find(|p| p.title == "Mi primera página")
        .map(|p| p.id.clone())
        .expect("first page");
    let attached = app.db.list_blocks(&first)?;
    assert!(
        attached
            .iter()
            .any(|b| b.kind == BlockKind::File && b.content == "foto.png"),
        "{attached:?}"
    );

    // Only non-Markdown files: nothing to import, attached to this page.
    drop_frame(&mut app, &ctx, &[other]);
    assert!(app.import_job.is_none());
    assert_eq!(app.notice.as_deref(), Some("Se adjuntó 1 archivo."));
    assert_eq!(app.blocks.last().map(|b| b.kind), Some(BlockKind::File));
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
                attachment: None,
                link: None,
                marks: Vec::new(),
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
    app.export_current_page_to(&ctx, &tmp.0)?;
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

/// H11.6: after restarting, each database opens on the view last open
/// there; if that view was deleted meanwhile, on the first.
#[test]
fn each_database_reopens_on_its_last_view_after_restarting() -> anyhow::Result<()> {
    use silex_desktop::model::ViewKind;
    // Declared before the apps: removed after their files are closed.
    let tmp = TempFolder::new("silex-app-last-view")?;
    let path = tmp.0.join("silex.sqlite3");
    let start = |path: &std::path::Path| -> anyhow::Result<SilexApp> {
        let mut app = SilexApp::new(Database::open(path)?)?;
        app.startup_backup_pending = false;
        Ok(app)
    };
    let ctx = egui::Context::default();
    let (lecturas, recetas) = {
        let mut app = start(&path)?;
        let lecturas = app.db.create_database("Lecturas", None)?;
        let recetas = app.db.create_database("Recetas", None)?;
        app.reload_pages()?;
        app.open_page(recetas.clone())?;
        app.add_view(ViewKind::Board)?;
        app.add_view(ViewKind::Table)?;
        app.switch_view(0)?;
        app.open_page(lecturas.clone())?;
        app.add_view(ViewKind::Board)?;
        assert_eq!(open_view(&app), "Tablero");
        app_frame(&mut app, &ctx, vec![]);
        (lecturas, recetas)
    };

    let mut app = start(&path)?;
    assert_eq!(app.selected.as_deref(), Some(lecturas.as_str()), "its tab");
    assert_eq!(open_view(&app), "Tablero", "the last view, not the first");
    app.open_page(recetas.clone())?;
    assert_eq!(open_view(&app), "Tabla", "went back to the first there");
    app.switch_view(2)?;
    assert_eq!(open_view(&app), "Tabla 2");
    app_frame(&mut app, &ctx, vec![]);
    drop(app);

    // «Tabla 2» goes while the app is closed: the first view instead.
    {
        let mut db = Database::open(&path)?;
        let gone = db.database_views(&recetas)?[2].id.clone();
        db.delete_view(&gone)?;
    }
    let mut app = start(&path)?;
    app.open_page(recetas)?;
    assert_eq!(open_view(&app), "Tabla");
    app.open_page(lecturas)?;
    assert_eq!(open_view(&app), "Tablero");
    assert!(app.notice.is_none(), "{:?}", app.notice);
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

// ---- H7.1: attachments (ADR 0007) ----

thread_local! {
    /// What the app asked to open in this test thread.
    static OPENED: std::cell::RefCell<Vec<std::path::PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn record_open(path: &std::path::Path) -> anyhow::Result<()> {
    OPENED.with(|o| o.borrow_mut().push(path.to_path_buf()));
    Ok(())
}

fn opened() -> Vec<std::path::PathBuf> {
    OPENED.with(|o| o.borrow().clone())
}

/// An app on a page with one paragraph («Antes»), its file folders in
/// `tmp`, and `files` (name, content) dropped on it.
fn file_app(
    tmp: &TempFolder,
    files: &[(&str, &[u8])],
) -> anyhow::Result<(SilexApp, egui::Context)> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    app.files.folders = Some(silex_desktop::attach_job::Folders {
        opened: tmp.0.join("abiertos"),
        saved: tmp.0.join("Exportaciones"),
    });
    app.files.opener = Some(record_open);
    let page = app.selected.clone().expect("a page");
    app.blocks = vec![Block {
        content: "Antes".into(),
        page_id: page,
        ..block("antes")
    }];
    app.mark_dirty();
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    let mut paths = Vec::new();
    for (name, content) in files {
        let path = tmp.0.join(name);
        std::fs::write(&path, content)?;
        paths.push(path);
    }
    drop_frame(&mut app, &ctx, &paths);
    // The new block takes the focus over two frames.
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx))
}

fn file_block(app: &SilexApp) -> Block {
    app.blocks
        .iter()
        .find(|b| b.kind == BlockKind::File)
        .cloned()
        .expect("a file block")
}

#[test]
fn a_dropped_file_is_a_block_that_opens_saves_and_undoes() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[("informe.pdf", b"%PDF-1.7")])?;
    assert_eq!(app.notice.as_deref(), Some("Se adjuntó 1 archivo."));
    assert_eq!(app.files.adding, 0, "nothing left in progress");
    let file = file_block(&app);
    assert_eq!(app.blocks.len(), 2, "after «Antes»");
    assert_eq!(file.content, "informe.pdf");
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t == "informe.pdf  8 B"),
        "{painted:?}"
    );
    // Saved like any block.
    app.save_now()?;
    let page = app.selected.clone().unwrap_or_default();
    assert_eq!(
        app.db.list_blocks(&page)?[1],
        Block {
            position: 1,
            ..file.clone()
        }
    );

    // Intro on the card opens a copy.
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.caret.as_ref().is_some_and(|c| c.block_id == file.id));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let copy = opened().last().cloned().expect("opened");
    assert_eq!(std::fs::read(&copy)?, b"%PDF-1.7");
    assert!(copy.starts_with(tmp.0.join("abiertos")));
    assert_eq!(app.blocks.len(), 2, "Intro did not split anything");

    // «Guardar una copia» writes it to the exports folder.
    app.block_action(BlockAction::SaveFile, &file.id);
    app_frame(&mut app, &ctx, vec![]);
    assert!(tmp.0.join("Exportaciones").join("informe.pdf").exists());
    assert_eq!(
        app.notice.as_deref(),
        Some("Se guardó una copia: «informe.pdf».")
    );

    // Backspace on the card deletes the block; Cmd+Z brings it back.
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(app.blocks.len(), 1);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, undo());
    assert_eq!(
        app.blocks.get(1).map(|b| b.id.clone()),
        Some(file.id.clone())
    );
    // A file block is not converted into text.
    app.block_action(BlockAction::Convert(BlockKind::Heading1), &file.id);
    assert_eq!(file_block(&app).kind, BlockKind::File);
    Ok(())
}

#[test]
fn programs_are_shown_in_their_folder_never_opened() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[("instalar.sh", b"#!/bin/sh\necho hola")])?;
    let file = file_block(&app);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.trim() == "Mostrar en la carpeta"),
        "{painted:?}"
    );
    assert!(!painted.iter().any(|t| t.trim() == "Abrir"), "{painted:?}");
    let before = opened().len();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let shown = opened();
    assert_eq!(shown.len(), before + 1);
    let folder = shown.last().cloned().unwrap_or_default();
    assert!(folder.is_dir(), "a folder was shown, not the script run");
    assert!(folder.join("instalar.sh").exists());
    Ok(())
}

#[test]
fn arrow_keys_reach_a_file_block_like_text() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[("foto.png", b"\x89PNG\r\n\x1a\n")])?;
    let file = file_block(&app);
    let antes = app.blocks[0].id.clone();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&antes)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(ctx.memory(|m| m.has_focus(app.block_field_id(&file.id))));
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(ctx.memory(|m| m.has_focus(app.block_field_id(&antes))));
    // ↓ on the card, the last block: the focus stays (egui would move it
    // to the nearest button).
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(ctx.memory(|m| m.has_focus(app.block_field_id(&file.id))));
    assert_eq!(file_block(&app).content, "foto.png");
    Ok(())
}

#[test]
fn files_being_added_are_shown_until_they_arrive() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[])?;
    app.files.adding = 2;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t == "Añadiendo 2 archivos…"),
        "{painted:?}"
    );
    app.files.adding = 0;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(!painted.iter().any(|t| t.starts_with("Añadiendo")));
    Ok(())
}

#[test]
fn files_dropped_on_a_database_are_not_attached() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx, _) = database_app()?;
    let path = tmp.0.join("nota.txt");
    std::fs::write(&path, "hola")?;
    drop_frame(&mut app, &ctx, &[path]);
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.contains("no una base de datos"), "{notice}");
    Ok(())
}

// ---- H7.2: image previews and pasted images ----

/// An RGBA image of `w` × `h` (a gradient), as egui pastes it.
fn pasted_image(w: usize, h: usize) -> egui::Event {
    let pixels = (0..w * h)
        .map(|i| egui::Color32::from_rgb((i % 256) as u8, 120, 200))
        .collect();
    egui::Event::PasteImage(std::sync::Arc::new(egui::ColorImage::new([w, h], pixels)))
}

#[test]
fn a_pasted_image_is_attached_and_previewed() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[])?;
    let antes = app.blocks[0].id.clone();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&antes)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![pasted_image(300, 200)]);
    let file = file_block(&app);
    assert_eq!(file.content, "Imagen pegada.png");
    assert_eq!(app.blocks[1].id, file.id, "after the block being edited");
    let id = file.attachment.clone().expect("its file");
    let info = app.files.attachments.get(&id).cloned().expect("known");
    assert_eq!(
        (info.mime.as_str(), info.width, info.height),
        ("image/png", Some(300), Some(200))
    );

    // Its preview is decoded once it is in sight.
    for _ in 0..3 {
        app_frame(&mut app, &ctx, vec![]);
    }
    assert!(matches!(
        app.files.previews.get(&id),
        Some(PreviewState::Ready(_))
    ));
    // The card holds the image (300 × 200 points) above its caption.
    let card = ctx
        .read_response(app.block_field_id(&file.id))
        .map(|r| r.rect)
        .expect("drawn");
    assert_eq!(card.height(), 200.0 + 8.0 + 44.0);

    // Cmd+Z takes the pasted image away, like any block.
    ctx.memory_mut(|m| m.surrender_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, undo());
    assert!(app.blocks.iter().all(|b| b.kind != BlockKind::File));

    // Another page: its previews are dropped.
    let other = app.db.create_page("Otra", None)?;
    app.reload_pages()?;
    app.open_page(other)?;
    assert!(app.files.previews.is_empty());
    Ok(())
}

#[test]
fn a_broken_image_says_why_and_stays_a_file() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    // A PNG header with a size, and nothing an image decoder can read.
    let mut broken = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    broken.extend(640u32.to_be_bytes());
    broken.extend(480u32.to_be_bytes());
    broken.extend([8, 6, 0, 0, 0, 1, 2, 3]);
    let (mut app, ctx) = file_app(&tmp, &[("roto.png", &broken)])?;
    let mut painted = Vec::new();
    for _ in 0..3 {
        painted = app_frame(&mut app, &ctx, vec![]);
    }
    assert!(
        painted
            .iter()
            .any(|t| t.starts_with("No se puede mostrar la imagen")),
        "{painted:?}"
    );
    // Still a file: it opens and saves like any other.
    assert!(painted.iter().any(|t| t.trim() == "Abrir"), "{painted:?}");
    Ok(())
}

#[test]
fn pasting_an_image_on_a_database_is_explained() -> anyhow::Result<()> {
    let (mut app, ctx, _) = database_app()?;
    app_frame(&mut app, &ctx, vec![pasted_image(2, 2)]);
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.contains("no una base de datos"), "{notice}");
    Ok(())
}

#[test]
fn previews_out_of_sight_are_dropped_when_memory_is_short() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[])?;
    app_frame(&mut app, &ctx, vec![pasted_image(40, 30)]);
    for _ in 0..3 {
        app_frame(&mut app, &ctx, vec![]);
    }
    let id = file_block(&app).attachment.expect("its file");
    // A preview of an image no longer drawn (as if scrolled far away).
    let texture = ctx.load_texture(
        "lejos",
        egui::ColorImage::new([50, 50], vec![egui::Color32::RED; 2500]),
        egui::TextureOptions::LINEAR,
    );
    app.files
        .previews
        .insert("lejos".into(), PreviewState::Ready(texture));
    // Within budget, both stay.
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.files.previews.contains_key("lejos"));
    // Over budget: the one out of sight goes, the one in sight stays.
    app.files.budget = Some(1);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(!app.files.previews.contains_key("lejos"));
    assert!(matches!(
        app.files.previews.get(&id),
        Some(PreviewState::Ready(_))
    ));
    Ok(())
}

// ---- H7.3: the file dialog ----

thread_local! {
    /// What the fake file dialog answers in this test thread.
    static PICKED: std::cell::RefCell<Vec<std::path::PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn fake_picker() -> Vec<std::path::PathBuf> {
    PICKED.with(|p| p.borrow().clone())
}

/// A page with «Antes» and an empty block below it, focused, and a fake
/// file dialog that answers `picked`.
fn picker_app(
    tmp: &TempFolder,
    picked: &[(&str, &[u8])],
) -> anyhow::Result<(SilexApp, egui::Context, String)> {
    let (mut app, ctx) = file_app(tmp, &[])?;
    let mut paths = Vec::new();
    for (name, content) in picked {
        let path = tmp.0.join(name);
        std::fs::write(&path, content)?;
        paths.push(path);
    }
    PICKED.with(|p| *p.borrow_mut() = paths);
    app.files.picker = Some(fake_picker);
    let page = app.selected.clone().unwrap_or_default();
    let empty = Block {
        page_id: page,
        ..block("vacio")
    };
    app.blocks.push(empty);
    app.mark_dirty();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id("vacio")));
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, "vacio".into()))
}

#[test]
fn slash_archivo_attaches_in_place_of_the_empty_block() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx, _) = picker_app(&tmp, &[("acta.pdf", b"%PDF-1.4"), ("nota.txt", b"hola")])?;
    app_frame(&mut app, &ctx, type_text("/archivo"));
    let menu = app.slash.clone().expect("slash menu");
    assert_eq!(
        menu.matches,
        [silex_desktop::editor::SlashItem::AttachFiles]
    );
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    let kinds: Vec<(BlockKind, String)> = app
        .blocks
        .iter()
        .map(|b| (b.kind, b.content.clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (BlockKind::Paragraph, "Antes".to_owned()),
            (BlockKind::File, "acta.pdf".to_owned()),
            (BlockKind::File, "nota.txt".to_owned()),
        ],
        "the empty block gave its place"
    );
    assert_eq!(app.notice.as_deref(), Some("Se adjuntaron 2 archivos."));
    Ok(())
}

#[test]
fn a_cancelled_dialog_only_removes_the_command() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx, empty) = picker_app(&tmp, &[])?;
    app_frame(&mut app, &ctx, type_text("/adj"));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks.len(), 2);
    let block = app
        .blocks
        .iter()
        .find(|b| b.id == empty)
        .expect("still there");
    assert_eq!(block.content, "");
    assert!(app.blocks.iter().all(|b| b.kind != BlockKind::File));
    assert!(app.files.pick.is_none());
    Ok(())
}

#[test]
fn the_palette_attaches_files_after_the_block_being_edited() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx, _) = picker_app(&tmp, &[("plano.png", b"\x89PNG\r\n\x1a\n")])?;
    let antes = app.blocks[0].id.clone();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&antes)));
    app_frame(&mut app, &ctx, vec![]);
    app.run_command(&ctx, silex_desktop::palette::Command::AttachFiles)?;
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks[1].content, "plano.png", "right after «Antes»");
    assert_eq!(
        app.blocks.len(),
        3,
        "the empty block stays: it was not «/archivo»"
    );
    Ok(())
}

// ---- H7.4: freeing space ----

#[test]
fn freeing_space_keeps_what_undo_can_bring_back() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let tmp = TempFolder::new("silex-files")?;
    let (mut app, ctx) = file_app(&tmp, &[("acta.pdf", b"%PDF-1.4")])?;
    let file = file_block(&app);
    let attachment = file.attachment.clone().expect("its file");
    // Deleted from the page, but Cmd+Z could bring it back.
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&file.id)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert!(app.blocks.iter().all(|b| b.kind != BlockKind::File));
    app.start_free_space(&ctx)?;
    assert!(app.db.attachment(&attachment)?.is_some(), "kept for undo");
    // Undo brings the block back, and it saves.
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, undo());
    assert_eq!(file_block(&app).attachment, Some(attachment.clone()));
    app.save_now()?;
    // With the history gone (another page), it can go.
    app.blocks.retain(|b| b.kind != BlockKind::File);
    app.mark_dirty();
    app.save_now()?;
    app.history.clear();
    app.start_free_space(&ctx)?;
    assert!(app.db.attachment(&attachment)?.is_none());
    Ok(())
}

// ---- H7.5: links between pages ----

/// «Recetas» (with «Pan» inside) and «Diario», open, with «Hoy» and an
/// empty block below it, focused. Returns (app, ctx, recetas, empty block).
fn links_app() -> anyhow::Result<(SilexApp, egui::Context, String, String)> {
    let db = Database::open_in_memory()?;
    let recetas = db.create_page("Recetas", None)?;
    db.create_page("Pan", Some(&recetas))?;
    let diario = db.create_page("Diario", None)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(diario.clone())?;
    app.blocks = vec![
        Block {
            content: "Hoy".into(),
            page_id: diario.clone(),
            ..block("hoy")
        },
        Block {
            page_id: diario,
            ..block("vacio")
        },
    ];
    app.mark_dirty();
    let ctx = egui::Context::default();
    ctx.memory_mut(|m| m.request_focus(app.block_field_id("vacio")));
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, recetas, "vacio".into()))
}

fn link_block(app: &SilexApp) -> Block {
    app.blocks
        .iter()
        .find(|b| b.kind == BlockKind::PageLink)
        .cloned()
        .expect("a link block")
}

#[test]
fn double_brackets_link_a_page_in_place_of_the_empty_block() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, recetas, empty) = links_app()?;
    app_frame(&mut app, &ctx, type_text("[["));
    let picker = app.links.picker.clone().expect("the page picker opens");
    assert_eq!(picker.replace.as_deref(), Some(empty.as_str()));
    assert!(
        app.blocks.iter().all(|b| !b.content.contains("[[")),
        "brackets gone"
    );
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, type_text("rece"));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.links.picker.is_none());
    let kinds: Vec<BlockKind> = app.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [BlockKind::Paragraph, BlockKind::PageLink],
        "in its place"
    );
    let link = link_block(&app);
    assert_eq!(link.link.as_deref(), Some(recetas.as_str()));
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.starts_with("Recetas")),
        "{painted:?}"
    );
    app.save_now()?;
    let page = app.selected.clone().unwrap_or_default();
    assert_eq!(
        app.db.list_blocks(&page)?[1].link.as_deref(),
        Some(recetas.as_str())
    );
    // Cmd+Z takes it away.
    ctx.memory_mut(|m| m.surrender_focus(app.block_field_id(&link.id)));
    app_frame(&mut app, &ctx, undo());
    assert!(app.blocks.iter().all(|b| b.kind != BlockKind::PageLink));
    Ok(())
}

#[test]
fn intro_on_a_link_opens_its_page_and_renames_show_at_once() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, recetas, _) = links_app()?;
    app.open_link_picker(None, None);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, type_text("Pan"));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let link = link_block(&app);
    // Its card shows the page's path, and its current title after a rename.
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t.starts_with("Pan") && t.ends_with("Recetas")),
        "{painted:?}"
    );
    let pan = link.link.clone().expect("its page");
    app.db.rename_page(&pan, "Pan de centeno")?;
    app.reload_pages()?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.starts_with("Pan de centeno")),
        "{painted:?}"
    );
    // Intro on the card opens it.
    ctx.memory_mut(|m| m.request_focus(app.block_field_id(&link.id)));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.selected.as_deref(), Some(pan.as_str()));
    let _ = recetas;
    Ok(())
}

#[test]
fn a_link_to_a_page_in_the_trash_offers_to_restore_it() -> anyhow::Result<()> {
    let (mut app, ctx, recetas, _) = links_app()?;
    app.open_link_picker(None, None);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, type_text("Recetas"));
    app_frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    let link = link_block(&app);
    app.save_now()?;
    app.db.trash_page(&recetas)?;
    app.reload_pages()?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.contains("en la papelera")),
        "{painted:?}"
    );
    app.block_action(BlockAction::OpenLink, &link.id);
    assert_eq!(
        app.links.restore.as_ref().map(|(id, _)| id.clone()),
        Some(recetas.clone())
    );
    // A new window is measured, unseen, on its first frame.
    app_frame(&mut app, &ctx, vec![]);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t.contains("¿Quieres restaurarla y abrirla?")),
        "{painted:?}"
    );
    // «Restaurar y abrir»: as the restore button would.
    app.links.restore = None;
    app.restore_from_trash(&recetas)?;
    assert_eq!(app.selected.as_deref(), Some(recetas.as_str()));
    assert!(app.db.get_page(&recetas)?.is_some());
    Ok(())
}

#[test]
fn slash_enlace_and_the_palette_open_the_picker_and_escape_closes_it() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, _, empty) = links_app()?;
    app_frame(&mut app, &ctx, type_text("/enlace"));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    let picker = app.links.picker.clone().expect("opened by /enlace");
    assert_eq!(picker.replace.as_deref(), Some(empty.as_str()));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.links.picker.is_none(), "Esc closes it");
    assert!(app.blocks.iter().all(|b| b.kind != BlockKind::PageLink));

    app.run_command(&ctx, silex_desktop::palette::Command::LinkPage)?;
    assert!(app.links.picker.is_some(), "opened by the palette");
    // The page that is open is not offered.
    let page = app.selected.clone();
    let offered = links::picker_matches(&app.pages, "", page.as_deref());
    assert!(offered.iter().all(|p| Some(&p.id) != page.as_ref()));
    Ok(())
}

// ---- H7.8: page history ----

#[test]
fn the_history_lists_versions_and_restores_one_that_cmd_z_takes_back() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, page) = page_history::history_fixture()?;
    let ctx = egui::Context::default();
    page_history::open_fixed_history(&mut app)?;
    let history = app.page_history.as_ref().expect("the window opens");
    let whens: Vec<String> = history
        .versions
        .iter()
        .map(|v| silex_desktop::versions::describe_when(&v.local, &history.today))
        .collect();
    assert_eq!(whens, ["Hoy, 12:00", "Ayer, 09:05", "18 oct, 17:40"]);
    // The editor does not take keys meanwhile: ↓ chooses a version.
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    assert_eq!(app.page_history.as_ref().map(|h| h.selected), Some(1));
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.contains("Comprar calabaza")),
        "the chosen version is shown: {painted:?}"
    );
    assert_eq!(app.title, "Recetas", "nothing changes before restoring");

    // Intro restores it: title and blocks, saved at once.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.page_history.is_none(), "the window closes");
    let contents = |blocks: &[Block]| blocks.iter().map(|b| b.content.clone()).collect::<Vec<_>>();
    assert_eq!(app.title, "Recetas de otoño");
    assert_eq!(contents(&app.blocks).len(), 8);
    assert!(app.blocks.iter().any(|b| b.checked));
    assert_eq!(contents(&app.db.list_blocks(&page)?), contents(&app.blocks));
    let notice = app.notice.clone().unwrap_or_default();
    assert!(notice.starts_with("Se restauró la versión de "), "{notice}");
    // What there was before is a version too.
    let versions = app.db.page_versions(&page)?;
    assert_eq!(versions.len(), 4);
    assert_eq!(versions[0].title, "Recetas");

    // Cmd+Z brings it back.
    app.undo();
    assert_eq!(app.title, "Recetas");
    assert_eq!(contents(&app.blocks), ["Solo queda la idea de la sopa."]);
    app.save_now()?;
    assert_eq!(
        app.db.get_page(&page)?.map(|p| p.title),
        Some("Recetas".into())
    );
    Ok(())
}

#[test]
fn escape_closes_the_history_without_changes() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, _) = page_history::history_fixture()?;
    let ctx = egui::Context::default();
    app.run_command(&ctx, silex_desktop::palette::Command::PageHistory)?;
    assert!(app.page_history.is_some(), "opened by the palette");
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.page_history.is_none());
    assert_eq!(app.title, "Recetas");
    assert!(!app.history.can_undo());
    Ok(())
}

#[test]
fn a_database_has_no_history_and_a_new_page_says_it_has_none() -> anyhow::Result<()> {
    let (mut app, _) = page_history::history_fixture()?;
    let ctx = egui::Context::default();
    app.create_database(None)?;
    app.open_page_history()?;
    assert!(app.page_history.is_none());
    assert!(
        app.notice
            .as_deref()
            .unwrap_or_default()
            .contains("no una base de datos"),
        "{:?}",
        app.notice
    );
    app.notice = None;
    app.create_root_page("Nueva")?;
    app.open_page_history()?;
    let painted = {
        app_frame(&mut app, &ctx, vec![]);
        app_frame(&mut app, &ctx, vec![])
    };
    assert!(
        painted
            .iter()
            .any(|t| t.starts_with("Todavía no hay versiones")),
        "{painted:?}"
    );
    assert!(app.page_history.is_some());
    Ok(())
}

// ---- H7.9: page export with attachments, on a worker ----

#[test]
fn a_page_with_files_is_exported_in_the_background() -> anyhow::Result<()> {
    use silex_desktop::attachment::NewAttachment;
    // Declared before the app: dropped after it (see above).
    let tmp = TempFolder::new("silex-app-page-export")?;
    let dir = tmp.0.clone();
    let db = Database::open(dir.join("silex.sqlite3"))?;
    let page = db.create_page("Viaje", None)?;
    let mut app = SilexApp::new(db)?;
    app.startup_backup_pending = false;
    app.open_page(page.clone())?;
    let mut pdf = b"%PDF-1.7".to_vec();
    pdf.resize(3_000_000, b' ');
    let file = app
        .db
        .add_attachment(&NewAttachment::from_bytes("billetes.pdf", pdf)?)?;
    app.blocks = vec![Block {
        page_id: page.clone(),
        kind: BlockKind::File,
        content: file.name.clone(),
        attachment: Some(file.id.clone()),
        ..block("f")
    }];
    app.mark_dirty();
    let ctx = egui::Context::default();

    let out = dir.join("exportaciones");
    app.export_current_page_to(&ctx, &out)?;
    assert!(app.save.can_close(), "saved before exporting");
    assert!(app.page_export.is_some(), "on a worker");
    // A second one meanwhile is refused, not queued.
    app.export_current_page_to(&ctx, &out)?;
    assert_eq!(
        app.notice.as_deref(),
        Some("Ya se está exportando una página.")
    );
    // The first frame is drawn before the result is collected: it always
    // says the export is running.
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t == "Exportando «Viaje» y sus adjuntos…"),
        "{painted:?}"
    );
    let start = std::time::Instant::now();
    while app.page_export.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_frame(&mut app, &ctx, vec![]);
    }
    let path = app.last_export.clone().expect("export finished");
    assert_eq!(path, out.join("Viaje.md"));
    let copy = out.join("Viaje (adjuntos)").join("billetes.pdf");
    assert_eq!(std::fs::metadata(copy)?.len(), 3_000_000);
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted.iter().any(|t| t.starts_with("Exportado a")),
        "{painted:?}"
    );

    // A folder that cannot be created: the error is shown.
    let blocked = dir.join("no-es-carpeta");
    std::fs::write(&blocked, b"x")?;
    app.notice = None;
    app.export_current_page_to(&ctx, &blocked)?;
    let start = std::time::Instant::now();
    while app.page_export.is_some() && start.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_frame(&mut app, &ctx, vec![]);
    }
    let notice = app.notice.clone().unwrap_or_default();
    assert!(
        notice.starts_with("No se pudo exportar la página"),
        "{notice}"
    );
    Ok(())
}

// ---- H7.11: templates in the window ----

/// «Notas» (with a paragraph) and «Proyectos», «Notas» open.
fn templates_app() -> anyhow::Result<(SilexApp, egui::Context, String, String)> {
    let mut db = Database::open_in_memory()?;
    let notas = db.create_page("Notas", None)?;
    let proyectos = db.create_page("Proyectos", None)?;
    db.save_document(
        &notas,
        "Notas",
        &[Block {
            page_id: notas.clone(),
            content: "Escrito el {{fecha}}".into(),
            ..block("n")
        }],
    )?;
    let mut app = SilexApp::new(db)?;
    app.open_page(notas.clone())?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx, notas, proyectos))
}

#[test]
fn a_page_saved_as_a_template_is_listed_and_stays_as_it_was() -> anyhow::Result<()> {
    let (mut app, ctx, notas, _) = templates_app()?;
    assert!(app.templates.list.is_empty());
    app.run_command(&ctx, silex_desktop::palette::Command::SaveAsTemplate)?;
    assert_eq!(app.templates.list.len(), 1);
    assert!(app.templates.open, "the section unfolds to show it");
    assert_eq!(
        app.selected.as_deref(),
        Some(notas.as_str()),
        "the page stays open"
    );
    assert!(app.pages.iter().all(|p| p.id != app.templates.list[0].id));
    let notice = app.notice.clone().unwrap_or_default();
    assert_eq!(notice, "«Notas» se guardó como plantilla, en «Plantillas».");
    // Not from a database.
    app.create_database(None)?;
    assert!(
        app.run_command(&ctx, silex_desktop::palette::Command::SaveAsTemplate)
            .is_err()
    );
    Ok(())
}

#[test]
fn the_picker_makes_a_page_from_a_template_with_todays_date() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, notas, proyectos) = templates_app()?;
    // The «…» menu of «Proyectos»: the page goes inside it.
    app.apply(RowAction::FromTemplate(Some(proyectos.clone())))?;
    assert_eq!(app.templates.list.len(), 3, "the examples, the first time");
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, type_text("diar"));
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t == "Se creará dentro de «Proyectos»."),
        "{painted:?}"
    );
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.templates.picker.is_none());
    let today = app.db.local_today()?;
    let expected = silex_desktop::templates::fill_date("Diario {{fecha}}", &today);
    assert_eq!(app.title, expected);
    assert!(app.focus_title, "the title is ready to edit");
    let page = app.selected.clone().expect("the new page is open");
    assert_ne!(page, notas);
    let parent = app
        .pages
        .iter()
        .find(|p| p.id == page)
        .and_then(|p| p.parent_id.clone());
    assert_eq!(parent.as_deref(), Some(proyectos.as_str()));
    assert!(app.expanded.contains(&proyectos), "visible in the tree");
    assert!(!app.blocks.is_empty());
    // Esc closes the picker without creating anything.
    let pages = app.pages.len();
    app.run_command(&ctx, silex_desktop::palette::Command::NewFromTemplate)?;
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.templates.picker.is_none());
    assert_eq!(app.pages.len(), pages);
    Ok(())
}

#[test]
fn an_open_template_says_so_is_editable_and_survives_other_trashing() -> anyhow::Result<()> {
    let (mut app, ctx, notas, proyectos) = templates_app()?;
    app.save_as_template(&notas)?;
    let template = app.templates.list[0].id.clone();
    app.open_page(template.clone())?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Plantilla"), "{painted:?}");
    assert!(
        painted.iter().any(|t| t.ends_with("Usar esta plantilla")),
        "{painted:?}"
    );
    // Edited like a page, with history and links.
    assert!(app.page_shows_blocks(&template));
    app.title = "Notas de reunión".into();
    app.mark_dirty();
    app.save_now()?;
    assert_eq!(app.templates.list[0].title, "Notas de reunión");
    assert!(
        app.pages.iter().all(|p| p.id != template),
        "still not a page"
    );
    // Trashing another page does not leave it.
    app.trash_page(&proyectos)?;
    assert_eq!(app.selected.as_deref(), Some(template.as_str()));
    // «Usar esta plantilla»: a page at the root, with the date filled.
    app.apply(RowAction::UseTemplate(template.clone(), None))?;
    assert_eq!(app.title, "Notas de reunión");
    let text = app.blocks[0].content.clone();
    assert!(
        text.starts_with("Escrito el ") && !text.contains("{{"),
        "{text}"
    );
    // Trashing the open template opens a page.
    app.open_page(template.clone())?;
    app.trash_page(&template)?;
    assert!(app.templates.list.is_empty());
    assert_ne!(app.selected.as_deref(), Some(template.as_str()));
    Ok(())
}

// ---- H8.1: tabs ----

/// «Uno», «Dos» and «Tres», each with a paragraph; «Uno» open.
fn tabs_app() -> anyhow::Result<(SilexApp, egui::Context, [String; 3])> {
    let mut db = Database::open_in_memory()?;
    let mut ids = Vec::new();
    for title in ["Uno", "Dos", "Tres"] {
        let id = db.create_page(title, None)?;
        db.save_document(
            &id,
            title,
            &[Block {
                page_id: id.clone(),
                content: format!("Texto de {title}"),
                ..block(&format!("b-{title}"))
            }],
        )?;
        ids.push(id);
    }
    let mut app = SilexApp::new(db)?;
    app.open_page(ids[0].clone())?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    let ids: [String; 3] = ids.try_into().map_err(|_| anyhow::anyhow!("three pages"))?;
    Ok((app, ctx, ids))
}

fn tab_ids(app: &SilexApp) -> Vec<String> {
    app.tabs.ids().to_vec()
}

#[test]
fn a_click_replaces_the_tab_and_cmd_click_opens_another() -> anyhow::Result<()> {
    let (mut app, _ctx, [uno, dos, tres]) = tabs_app()?;
    assert_eq!(tab_ids(&app), std::slice::from_ref(&uno));
    app.open_page(dos.clone())?;
    assert_eq!(
        tab_ids(&app),
        std::slice::from_ref(&dos),
        "a click: the same tab"
    );
    app.open_in_new_tab(&tres)?;
    assert_eq!(tab_ids(&app), [dos.clone(), tres.clone()]);
    assert_eq!(app.selected.as_deref(), Some(tres.as_str()));
    // A page that has a tab: its tab is shown, not a copy.
    app.open_page(dos.clone())?;
    assert_eq!(tab_ids(&app), [dos.clone(), tres.clone()]);
    assert_eq!(app.tabs.active(), 0);
    // The «…» menu and Cmd/Ctrl+click in the sidebar.
    app.apply(RowAction::OpenInNewTab(uno.clone()))?;
    assert_eq!(tab_ids(&app), [dos, uno.clone(), tres]);
    assert_eq!(app.selected.as_deref(), Some(uno.as_str()));
    Ok(())
}

#[test]
fn each_tab_keeps_its_undo_and_scroll() -> anyhow::Result<()> {
    let (mut app, ctx, [uno, dos, _]) = tabs_app()?;
    app.blocks[0].content = "Uno cambiado".into();
    app.mark_dirty();
    app.editor_scroll = 120.0;
    app.open_in_new_tab(&dos)?;
    assert_eq!(
        app.db.list_blocks(&uno)?[0].content,
        "Uno cambiado",
        "saved on leaving"
    );
    assert!(!app.history.can_undo(), "«Dos» has its own history");
    // Back to «Uno»: Cmd+Z still undoes the change, and the scroll is back.
    app.select_tab(0)?;
    assert_eq!(app.pending_scroll, Some(120.0));
    assert!(app.history.can_undo());
    app.undo();
    assert_eq!(app.blocks[0].content, "Texto de Uno");
    app_frame(&mut app, &ctx, vec![]);
    // Closing a tab forgets its undo.
    app.close_tab(0)?;
    app.open_in_new_tab(&uno)?;
    assert!(!app.history.can_undo());
    Ok(())
}

#[test]
fn tabs_close_and_switch_with_the_keyboard() -> anyhow::Result<()> {
    use egui::{Key, Modifiers as M};
    let (mut app, ctx, [uno, dos, tres]) = tabs_app()?;
    app.open_in_new_tab(&dos)?;
    app.open_in_new_tab(&tres)?;
    assert_eq!(tab_ids(&app), [uno.clone(), dos.clone(), tres.clone()]);
    app_frame(&mut app, &ctx, vec![key(Key::Num1, M::COMMAND)]);
    assert_eq!(app.selected.as_deref(), Some(uno.as_str()));
    app_frame(&mut app, &ctx, vec![key(Key::Num9, M::COMMAND)]);
    assert_eq!(app.selected.as_deref(), Some(tres.as_str()), "9: the last");
    app_frame(&mut app, &ctx, vec![key(Key::Tab, M::CTRL)]);
    assert_eq!(app.selected.as_deref(), Some(uno.as_str()), "goes round");
    app_frame(&mut app, &ctx, vec![key(Key::Tab, M::CTRL | M::SHIFT)]);
    assert_eq!(app.selected.as_deref(), Some(tres.as_str()));
    app_frame(&mut app, &ctx, vec![key(Key::W, M::COMMAND)]);
    assert_eq!(tab_ids(&app), [uno.clone(), dos.clone()]);
    assert_eq!(app.selected.as_deref(), Some(dos.as_str()));
    app_frame(&mut app, &ctx, vec![key(Key::W, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::W, M::COMMAND)]);
    assert_eq!(
        tab_ids(&app),
        std::slice::from_ref(&uno),
        "the last tab stays"
    );
    // Cmd+T: the search, and what is chosen opens in a new tab.
    app_frame(&mut app, &ctx, vec![key(Key::T, M::COMMAND)]);
    assert!(app.search.as_ref().is_some_and(|s| s.new_tab));
    app_frame(&mut app, &ctx, type_text("Dos"));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(tab_ids(&app), [uno, dos.clone()]);
    assert_eq!(app.selected.as_deref(), Some(dos.as_str()));
    Ok(())
}

#[test]
fn a_page_sent_to_the_trash_closes_its_tab() -> anyhow::Result<()> {
    let (mut app, _ctx, [uno, dos, tres]) = tabs_app()?;
    app.open_in_new_tab(&dos)?;
    app.open_in_new_tab(&tres)?;
    // Another tab's page: the open one stays.
    app.trash_page(&uno)?;
    assert_eq!(tab_ids(&app), [dos.clone(), tres.clone()]);
    assert_eq!(app.selected.as_deref(), Some(tres.as_str()));
    // The open one: its neighbour is shown.
    app.trash_page(&tres)?;
    assert_eq!(tab_ids(&app), std::slice::from_ref(&dos));
    assert_eq!(app.selected.as_deref(), Some(dos.as_str()));
    // Restoring does not reopen a tab.
    app.restore_from_trash(&uno)?;
    assert_eq!(tab_ids(&app), [uno]);
    Ok(())
}

#[test]
fn the_thirteenth_tab_closes_the_oldest() -> anyhow::Result<()> {
    let (mut app, _ctx, [uno, _, _]) = tabs_app()?;
    let mut ids = Vec::new();
    for n in 0..12 {
        ids.push(app.db.create_page(&format!("Página {n}"), None)?);
    }
    app.reload_pages()?;
    for id in &ids {
        app.open_in_new_tab(id)?;
    }
    assert_eq!(app.tabs.len(), silex_desktop::tabs::MAX_TABS);
    assert!(
        app.tabs.position(&uno).is_none(),
        "«Uno», the oldest, closed"
    );
    Ok(())
}

#[test]
fn tabs_come_back_after_closing_the_app() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-app-tabs")?;
    let path = tmp.0.join("silex.sqlite3");
    let (uno, dos, tres) = {
        let db = Database::open(&path)?;
        let uno = db.create_page("Uno", None)?;
        let dos = db.create_page("Dos", None)?;
        let tres = db.create_page("Tres", None)?;
        let mut app = SilexApp::new(db)?;
        app.startup_backup_pending = false;
        app.open_page(uno.clone())?;
        app.open_in_new_tab(&dos)?;
        app.open_in_new_tab(&tres)?;
        app.select_tab(1)?;
        (uno, dos, tres)
    };
    // «Tres» went to the trash meanwhile (another copy of the app).
    Database::open(&path)?.trash_page(&tres)?;
    let mut app = SilexApp::new(Database::open(&path)?)?;
    app.startup_backup_pending = false;
    assert_eq!(tab_ids(&app), [uno, dos.clone()]);
    assert_eq!(app.selected.as_deref(), Some(dos.as_str()));
    Ok(())
}

// ---- H8.2: the page beside ----

#[test]
fn a_page_beside_is_read_only_kept_up_to_date_and_can_be_edited_here() -> anyhow::Result<()> {
    let (mut app, ctx, [uno, dos, _]) = tabs_app()?;
    assert!(app.main_width >= side_panel::MIN_PAGE_WIDTH + side_panel::MIN_SIDE_WIDTH);
    app.apply(RowAction::OpenBeside(dos.clone()))?;
    let side = app.side.as_ref().expect("the panel opens");
    assert_eq!(side.page, dos);
    assert_eq!(
        app.selected.as_deref(),
        Some(uno.as_str()),
        "the page stays open"
    );
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Texto de Dos"), "{painted:?}");
    assert!(painted.iter().any(|t| t == "Solo lectura"), "{painted:?}");
    // Saving another copy of «Dos» shows at once (the panel reads it again).
    let mut blocks = app.db.list_blocks(&dos)?;
    blocks[0].content = "Dos, cambiado".into();
    app.db.save_document(&dos, "Dos", &blocks)?;
    app.blocks[0].content = "Uno, cambiado".into();
    app.mark_dirty();
    app.save_now()?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(painted.iter().any(|t| t == "Dos, cambiado"), "{painted:?}");
    // «Editar aquí»: in the active tab, and the panel closes.
    let side = app.side.take().map(|s| s.page).unwrap_or_default();
    app.open_page(side)?;
    assert_eq!(app.selected.as_deref(), Some(dos.as_str()));
    assert!(app.side.is_none());
    Ok(())
}

#[test]
fn a_narrow_window_says_so_instead_of_a_page_beside() -> anyhow::Result<()> {
    let (mut app, _ctx, [_, dos, _]) = tabs_app()?;
    app.main_width = 700.0;
    app.open_beside(&dos)?;
    assert!(app.side.is_none());
    let notice = app.notice.clone().unwrap_or_default();
    assert!(
        notice.starts_with("La ventana es demasiado estrecha"),
        "{notice}"
    );
    Ok(())
}

#[test]
fn alt_click_on_a_link_opens_beside_and_the_trash_shows_there() -> anyhow::Result<()> {
    let (mut app, ctx, [uno, dos, _]) = tabs_app()?;
    let link = Block {
        page_id: uno.clone(),
        kind: BlockKind::PageLink,
        link: Some(dos.clone()),
        ..block("enlace")
    };
    app.blocks.push(link);
    app.mark_dirty();
    app.save_now()?;
    app_frame(&mut app, &ctx, vec![]);
    app.click_modifiers = egui::Modifiers::ALT;
    app.open_link("enlace")?;
    assert_eq!(app.side.as_ref().map(|s| s.page.clone()), Some(dos.clone()));
    assert_eq!(app.tabs.len(), 1, "not a tab");
    // Sent to the trash: the panel says so.
    app.trash_page(&dos)?;
    assert!(matches!(
        app.side.as_ref().map(|s| &s.content),
        Some(SideContent::Gone)
    ));
    let painted = app_frame(&mut app, &ctx, vec![]);
    assert!(
        painted
            .iter()
            .any(|t| t == "Esta página está en la papelera o ya no existe."),
        "{painted:?}"
    );
    Ok(())
}

#[test]
fn a_database_beside_shows_its_table() -> anyhow::Result<()> {
    let (mut app, ctx, _) = tabs_app()?;
    let base = app.db.create_database("Lecturas", None)?;
    let row = app.db.create_row(&base, "Rayuela")?;
    let autor = app
        .db
        .add_property(&base, "Autor", silex_desktop::property::PropertyKind::Text)?;
    app.db.set_value(
        &row,
        &autor,
        Some(&silex_desktop::property::CellValue::Text("Cortázar".into())),
    )?;
    app.reload_pages()?;
    app.open_beside(&base)?;
    let painted = app_frame(&mut app, &ctx, vec![]);
    for text in ["Lecturas", "Nombre", "Autor", "Rayuela", "Cortázar"] {
        assert!(painted.iter().any(|t| t == text), "{text}: {painted:?}");
    }
    Ok(())
}

// ---- H7.12: local data kept private ----

#[test]
fn copies_of_opened_files_are_deleted_when_the_app_closes() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-app-opened")?;
    let (mut app, ctx) = file_app(&tmp, &[("informe.pdf", b"%PDF-1.7 informe")])?;
    let file = file_block(&app);
    app.block_action(BlockAction::OpenFile, &file.id);
    app_frame(&mut app, &ctx, vec![]);
    let copy = opened().last().cloned().expect("a copy was opened");
    assert!(copy.starts_with(tmp.0.join("abiertos")) && copy.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(tmp.0.join("abiertos"))?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "only the user lists them");
    }
    drop(app);
    assert!(!tmp.0.join("abiertos").exists(), "emptied on closing");
    Ok(())
}

#[cfg(unix)]
#[test]
fn starting_makes_the_database_and_its_copies_private() -> anyhow::Result<()> {
    use silex_desktop::db::backup;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempFolder::new("silex-app-private")?;
    let path = tmp.0.join("silex.sqlite3");
    Database::open(&path)?.create_page("Notas", None)?;
    backup::create_backup(&path, &backup::backups_dir(&path))?;
    let mode = |p: &std::path::Path| -> anyhow::Result<u32> {
        Ok(std::fs::metadata(p)?.permissions().mode() & 0o777)
    };
    // As an older Silex (or a lax umask) left them.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
    std::fs::set_permissions(
        backup::backups_dir(&path),
        std::fs::Permissions::from_mode(0o755),
    )?;
    std::fs::set_permissions(&tmp.0, std::fs::Permissions::from_mode(0o755))?;
    let mut app = SilexApp::new(Database::open(&path)?)?;
    app.startup_backup_pending = false;
    assert_eq!(mode(&path)?, 0o600);
    assert_eq!(mode(&backup::backups_dir(&path))?, 0o700);
    for copy in std::fs::read_dir(backup::backups_dir(&path))? {
        assert_eq!(mode(&copy?.path())?, 0o600);
    }
    // A folder the user chose is not Silex's to change.
    assert_eq!(mode(&tmp.0)?, 0o755);
    assert!(app.notice.is_none(), "{:?}", app.notice);
    Ok(())
}

#[test]
fn a_database_in_a_synced_folder_is_warned_about_until_told_not_to() -> anyhow::Result<()> {
    let tmp = TempFolder::new("silex-app-synced")?;
    let folder = tmp.0.join("Dropbox").join("notas");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("silex.sqlite3");
    let ctx = egui::Context::default();
    {
        let mut app = SilexApp::new(Database::open(&path)?)?;
        app.startup_backup_pending = false;
        let warning = app.synced_warning.as_ref().expect("warned");
        assert_eq!(warning.service, "Dropbox");
        app_frame(&mut app, &ctx, vec![]);
        let painted = app_frame(&mut app, &ctx, vec![]);
        assert!(
            painted
                .iter()
                .any(|t| t == "Tus datos están en una carpeta de Dropbox"),
            "{painted:?}"
        );
        // «No volver a avisar».
        app.db.dismiss_synced_warning()?;
    }
    let mut app = SilexApp::new(Database::open(&path)?)?;
    app.startup_backup_pending = false;
    assert!(app.synced_warning.is_none());
    Ok(())
}

/// The window's minimum size follows the interface size, so a zoomed
/// sidebar and page still fit side by side.
#[test]
fn the_window_minimum_grows_with_the_interface_size() -> anyhow::Result<()> {
    use silex_desktop::palette::Command;
    let min_sizes = |ctx: &egui::Context, app: &mut SilexApp| -> Vec<egui::Vec2> {
        let input = egui::RawInput::default();
        let mut output = ctx.run_ui(input, |ui| app.update_ui(ui));
        output.textures_delta.clear();
        output
            .viewport_output
            .values()
            .flat_map(|v| v.commands.iter())
            .filter_map(|c| match c {
                egui::ViewportCommand::MinInnerSize(size) => Some(*size),
                _ => None,
            })
            .collect()
    };
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    // At 100 % the window already has it (`main.rs`): nothing to send.
    assert!(min_sizes(&ctx, &mut app).is_empty());
    app.run_command(&ctx, Command::ZoomIn)?;
    let sent = min_sizes(&ctx, &mut app);
    let expected = MIN_WINDOW * 1.1;
    assert!(
        sent.iter().any(|s| (*s - expected).length() < 0.01),
        "{sent:?}"
    );
    // Sent once, not on every frame.
    assert!(min_sizes(&ctx, &mut app).is_empty());
    Ok(())
}

/// Manual (H9.3b): `cargo test --release --bin silex-desktop startup_and_navigation -- --ignored --nocapture`.
/// Prints what a user waits for, on a database on disk: opening it,
/// building the app, the first frame (fonts included), a frame at rest,
/// and opening a page (plus its first frame). Workspaces of 100, 1000 and
/// 5000 pages, in trees of ten, with 10–20 text blocks each.
#[test]
#[ignore = "scale smoke test; run manually"]
fn startup_and_navigation_smoke() -> anyhow::Result<()> {
    use silex_desktop::model::NewBlock;
    use std::time::{Duration, Instant};
    for (pages, blocks) in [(100, 10), (1_000, 20), (5_000, 20)] {
        let folder = TempFolder::new("silex-perf")?;
        let path = folder.0.join("silex.sqlite3");
        let mut ids = Vec::new();
        {
            let mut db = Database::open(&path)?;
            let mut parent: Option<String> = None;
            for p in 0..pages {
                let content: Vec<NewBlock> = (0..blocks)
                    .map(|b| NewBlock {
                        kind: BlockKind::ALL[b % BlockKind::ALL.len()],
                        indent: 0,
                        checked: false,
                        content: format!("Bloque {b} de la página {p}: texto de ejemplo"),
                        marks: Vec::new(),
                    })
                    .collect();
                let under = if p % 10 == 0 { None } else { parent.as_deref() };
                let id = db.create_page_with_blocks(&format!("Página {p}"), under, &content)?;
                if p % 10 == 0 {
                    parent = Some(id.clone());
                }
                ids.push(id);
            }
        }

        let t = Instant::now();
        let db = Database::open(&path)?;
        let open = t.elapsed();
        let t = Instant::now();
        let mut app = SilexApp::new(db)?;
        let new = t.elapsed();
        let ctx = egui::Context::default();
        let t = Instant::now();
        sized_frame(&mut app, &ctx, vec![]);
        let first = t.elapsed();
        let t = Instant::now();
        for _ in 0..20 {
            sized_frame(&mut app, &ctx, vec![]);
        }
        let rest = t.elapsed() / 20;

        // Opening pages spread over the workspace, then their first frame.
        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;
        let step = (pages / 50).max(1);
        let picks: Vec<&String> = ids.iter().step_by(step).take(50).collect();
        for id in &picks {
            let t = Instant::now();
            app.open_page((*id).clone())?;
            sized_frame(&mut app, &ctx, vec![]);
            let took = t.elapsed();
            worst = worst.max(took);
            total += took;
        }
        let average = total / picks.len() as u32;
        println!(
            "{pages} páginas × {blocks} bloques: abrir la base {open:?}, crear la app {new:?}, \
             primer frame {first:?}, frame en reposo {rest:?}, abrir página {average:?} \
             (peor {worst:?})"
        );
    }
    Ok(())
}

// ---- H11.7: find and replace in the page ----

fn find_counter(app: &SilexApp) -> String {
    app.find.as_ref().map(|f| f.counter()).unwrap_or_default()
}

fn shown_match(app: &SilexApp) -> Option<(usize, usize, usize)> {
    let m = app.find.as_ref()?.shown()?;
    Some((m.block, m.start, m.end))
}

/// The selection (anchor, cursor) of block `index`'s field, as stored.
fn block_selection(app: &SilexApp, ctx: &egui::Context, index: usize) -> Option<(usize, usize)> {
    let id = app.block_field_id(&app.blocks[index].id);
    let range = egui::text_edit::TextEditState::load(ctx, id)?
        .cursor
        .char_range()?;
    Some((range.secondary.index.0, range.primary.index.0))
}

#[test]
fn cmd_f_finds_counts_and_steps_through_the_page() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(
        &[
            (Par, 0, "Árbol y arbol"),
            (Cod, 0, "let arbol = 1;"),
            (Bul, 0, "sin nada"),
            (Par, 0, "ÁRBOL 🌳"),
        ],
        // The caret in the third block: the search starts after it.
        2,
        0,
    )?;
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.find.is_some(), "the bar opens");
    assert!(
        ctx.memory(|m| m.has_focus(FindField::Query.id())),
        "its field has the keyboard"
    );
    type_into(&mut app, &ctx, "arbol");
    assert_eq!(find_counter(&app), "4 de 4", "the first after the caret");
    assert_eq!(shown_match(&app), Some((3, 0, 5)));
    // Enter goes round to the first; Shift+Enter and ↑ go back; ↓ forward.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(find_counter(&app), "1 de 4");
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::SHIFT)]);
    assert_eq!(find_counter(&app), "4 de 4");
    app_frame(&mut app, &ctx, vec![key(Key::ArrowUp, M::NONE)]);
    assert_eq!(find_counter(&app), "3 de 4");
    assert_eq!(shown_match(&app), Some((1, 4, 9)), "in the code block");
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    assert_eq!(find_counter(&app), "4 de 4");
    assert!(
        ctx.memory(|m| m.has_focus(FindField::Query.id())),
        "Enter keeps the keyboard in the field"
    );
    // A query with no match.
    type_into(&mut app, &ctx, "x");
    assert_eq!(find_counter(&app), "Sin resultados");
    assert_eq!(
        contents(&app)[0].2,
        "Árbol y arbol",
        "searching changes nothing"
    );
    Ok(())
}

#[test]
fn esc_closes_the_bar_and_leaves_the_match_chosen() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno"), (Par, 0, "dos y tres")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    type_into(&mut app, &ctx, "TRES");
    assert_eq!(find_counter(&app), "1 de 1");
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.find.is_none());
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    assert_eq!(
        block_selection(&app, &ctx, 1),
        Some((6, 10)),
        "«tres» chosen"
    );
    // Typing replaces it, as in any text field.
    type_into(&mut app, &ctx, "4");
    assert_eq!(contents(&app)[1].2, "dos y 4");
    Ok(())
}

#[test]
fn replace_one_then_all_each_one_undo_step() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "gato, Gato y GATO"), (Bul, 0, "un gato")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND | M::ALT)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(
        app.find.as_ref().is_some_and(|f| f.replace),
        "with «Reemplazar»"
    );
    type_into(&mut app, &ctx, "gato");
    assert_eq!(find_counter(&app), "1 de 4");
    // Into «Reemplazar por»: Cmd+Opción+F again goes there.
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND | M::ALT)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(ctx.memory(|m| m.has_focus(FindField::Replacement.id())));
    type_into(&mut app, &ctx, "gatos");
    // Enter replaces the one shown and shows the next (not inside «gatos»).
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(contents(&app)[0].2, "gatos, Gato y GATO");
    assert_eq!(shown_match(&app), Some((0, 7, 11)), "the next one");
    assert_eq!(find_counter(&app), "2 de 4", "«gatos» still matches «gato»");

    let before_all = contents(&app);
    app.run_find(FindAction::ReplaceAll);
    assert_eq!(contents(&app)[0].2, "gatoss, gatos y gatos");
    assert_eq!(contents(&app)[1].2, "un gatos");
    assert_eq!(
        app.find.as_ref().and_then(|f| f.status.clone()).as_deref(),
        Some("4 reemplazadas")
    );
    // Cmd+Z with the bar's field focused undoes the page, in one step each.
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(contents(&app), before_all, "«Reemplazar todo» is one step");
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(
        contents(&app)[0].2,
        "gato, Gato y GATO",
        "and the single one"
    );
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND | M::SHIFT)]);
    assert_eq!(contents(&app)[0].2, "gatos, Gato y GATO");
    assert!(app.save.is_dirty(), "saved by the autosave");
    Ok(())
}

#[test]
fn ctrl_h_opens_replace_outside_a_mac() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "hola")], 0, 0)?;
    ctx.set_os(egui::os::OperatingSystem::Windows);
    app_frame(&mut app, &ctx, vec![key(Key::H, M::CTRL)]);
    assert!(app.find.as_ref().is_some_and(|f| f.replace));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.find.is_none(), "Esc closes it");
    Ok(())
}

#[test]
fn enter_confirming_an_ime_composition_does_not_jump_or_replace() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "にほん y にほん")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND | M::ALT)]);
    app_frame(&mut app, &ctx, vec![]);
    type_into(&mut app, &ctx, "に");
    assert_eq!(find_counter(&app), "1 de 2");
    let preedit = egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "ほ".into(),
        active_range_chars: None,
    });
    app_frame(&mut app, &ctx, vec![preedit]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("ほ".into()))],
    );
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(find_counter(&app), "1 de 2", "Enter did not jump");
    assert!(
        ctx.memory(|m| m.has_focus(FindField::Query.id())),
        "still typing"
    );
    assert_eq!(contents(&app)[0].2, "にほん y にほん", "nothing replaced");
    // Once composed, Enter jumps again.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(find_counter(&app), "2 de 2");
    Ok(())
}

#[test]
fn the_bar_is_for_pages_and_closes_with_another_page() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "hola")], 0, 0)?;
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND)]);
    assert!(app.find.is_some());
    let other = app.db.create_page("Otra", None)?;
    app.reload_pages()?;
    app.open_page(other)?;
    assert!(app.find.is_none(), "another page: closed");
    // A database has no blocks: Cmd+F does nothing there.
    app.create_database(None)?;
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![key(Key::F, M::COMMAND)]);
    assert!(app.find.is_none());
    Ok(())
}

#[test]
fn the_page_scrolls_to_the_match_shown() -> anyhow::Result<()> {
    let mut specs: Vec<(BlockKind, u8, &str)> = vec![(Par, 0, "aguja al principio")];
    specs.extend(std::iter::repeat_n((Par, 0, "paja"), 120));
    specs.push((Par, 0, "aguja al final"));
    let (mut app, ctx) = editor_app(&specs, 0, 0)?;
    // A window of a real size, so the page does not fit.
    let frame = |app: &mut SilexApp, events: Vec<egui::Event>| {
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
    };
    frame(&mut app, vec![key(Key::F, M::COMMAND)]);
    frame(&mut app, vec![]);
    for c in "aguja".chars() {
        frame(&mut app, vec![egui::Event::Text(c.to_string())]);
    }
    frame(&mut app, vec![]);
    assert_eq!(find_counter(&app), "1 de 2");
    let top = app.editor_scroll;
    frame(&mut app, vec![key(Key::Enter, M::NONE)]);
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert_eq!(find_counter(&app), "2 de 2");
    assert!(
        app.editor_scroll > top + 500.0,
        "{} → {}",
        top,
        app.editor_scroll
    );
    frame(&mut app, vec![key(Key::Enter, M::NONE)]);
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    assert!(
        app.editor_scroll < 200.0,
        "back at the top: {}",
        app.editor_scroll
    );
    Ok(())
}

// ---- H11.8: recent pages in the search window ----

fn recent_titles(app: &SilexApp) -> Vec<String> {
    app.search
        .as_ref()
        .map(|w| w.recent.iter().map(|h| h.page.title.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn the_empty_search_lists_recent_pages_kept_between_sessions() -> anyhow::Result<()> {
    // Declared before the apps: removed after their files are closed.
    let tmp = TempFolder::new("silex-app-recent")?;
    let path = tmp.0.join("silex.sqlite3");
    let start = |path: &std::path::Path| -> anyhow::Result<SilexApp> {
        let mut app = SilexApp::new(Database::open(path)?)?;
        app.startup_backup_pending = false;
        Ok(app)
    };
    let ctx = egui::Context::default();
    let (proyecto, notas) = {
        let mut app = start(&path)?;
        let proyecto = app.db.create_page("Proyecto", None)?;
        let notas = app.db.create_page("Notas", Some(&proyecto))?;
        let recetas = app.db.create_page("Recetas", None)?;
        let borrador = app.db.create_page("Borrador", None)?;
        app.reload_pages()?;
        for id in [&borrador, &recetas, &proyecto, &notas] {
            app.open_page(id.clone())?;
        }
        // A template opened is not listed; a page in the trash neither.
        let plantilla = app.db.save_as_template(&recetas)?;
        app.reload_pages()?;
        app.open_page(plantilla)?;
        app.open_page(notas.clone())?;
        app.trash_page(&borrador)?;
        app_frame(&mut app, &ctx, vec![]);
        (proyecto, notas)
    };

    let mut app = start(&path)?;
    assert_eq!(app.selected.as_deref(), Some(notas.as_str()), "its tab");
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    // Most recent first, without the open page, the template or the trash.
    // (the welcome page opened when the file was new counts too).
    assert_eq!(
        recent_titles(&app),
        ["Proyecto", "Recetas", "Mi primera página"]
    );
    assert_eq!(app.search.as_ref().map(|w| w.selected), Some(0));
    // Enter opens the first: going back to the page before is one key.
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.search.is_none());
    assert_eq!(app.selected.as_deref(), Some(proyecto.as_str()));

    // ↓ then Enter: the second. Now «Notas» (with its path) is the first.
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert_eq!(
        recent_titles(&app),
        ["Notas", "Recetas", "Mi primera página"]
    );
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        app.selected
            .as_ref()
            .and_then(|id| app.find_page(id))
            .map(|p| p.title.clone()),
        Some("Recetas".to_owned())
    );

    // Typing leaves the recent pages out; clearing brings them back.
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![egui::Event::Text("not".into())]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(recent_titles(&app).is_empty());
    // The search runs on its worker thread: wait for its results.
    let found = |app: &SilexApp| {
        app.search
            .as_ref()
            .is_some_and(|w| w.results.iter().any(|h| h.page.title == "Notas"))
    };
    let waiting = std::time::Instant::now();
    while !found(&app) && waiting.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_frame(&mut app, &ctx, vec![]);
    }
    assert!(found(&app), "«Notas» found");
    for _ in 0..3 {
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    }
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        recent_titles(&app),
        ["Proyecto", "Notas", "Mi primera página"]
    );
    assert!(app.notice.is_none(), "{:?}", app.notice);
    Ok(())
}

#[test]
fn cmd_t_opens_a_recent_page_in_a_new_tab() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    let first = app.selected.clone().expect("a page");
    let otra = app.db.create_page("Otra", None)?;
    app.reload_pages()?;
    app.open_page(otra.clone())?;
    let tabs = app.tabs.len();
    app_frame(&mut app, &ctx, vec![key(Key::T, M::COMMAND)]);
    assert_eq!(app.search.as_ref().map(|w| w.recent.len()), Some(1));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.selected.as_deref(), Some(first.as_str()));
    assert_eq!(app.tabs.len(), tabs + 1, "in a new tab");
    Ok(())
}

// ---- H9.4: «Buscar actualizaciones» (ADR 0014) ----

#[test]
fn check_updates_only_asks_the_system_to_open_the_releases_page() -> anyhow::Result<()> {
    let mut app = SilexApp::new(Database::open_in_memory()?)?;
    let ctx = egui::Context::default();
    app_frame(&mut app, &ctx, vec![]);
    app.run_command(&ctx, silex_desktop::palette::Command::CheckUpdates)?;
    let commands = app_frame_commands(&mut app, &ctx, vec![]);
    let urls: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(urls, ["https://github.com/enshetinin/silex/releases"]);
    Ok(())
}

// ---- Inline formatting (H12.1b, ADR 0015) ----

/// One paragraph with `text` and `marks`, saved, focused at `char`.
fn formatted_app(
    text: &str,
    marks: Vec<silex_desktop::marks::Mark>,
    char: usize,
) -> anyhow::Result<(SilexApp, egui::Context)> {
    let (mut app, ctx) = editor_app(&[(Par, 0, text)], 0, char)?;
    app.blocks[0].marks = marks;
    let page = app.selected.clone().expect("page");
    let title = app.title.clone();
    app.db.save_document(&page, &title, &app.blocks)?;
    app.reset_history();
    app_frame(&mut app, &ctx, vec![]);
    Ok((app, ctx))
}

fn bold(start: usize, end: usize) -> silex_desktop::marks::Mark {
    silex_desktop::marks::Mark::new(start, end, silex_desktop::marks::MarkKind::Bold)
}

/// The characters each mark of block `index` covers.
fn marked_words(app: &SilexApp, index: usize) -> Vec<String> {
    let block = &app.blocks[index];
    block
        .marks
        .iter()
        .map(|m| {
            block
                .content
                .chars()
                .skip(m.start)
                .take(m.end - m.start)
                .collect()
        })
        .collect()
}

#[test]
fn typing_inside_and_at_the_end_of_bold_continues_it() -> anyhow::Result<()> {
    // «hola mundo», «mundo» bold, caret inside it.
    let (mut app, ctx) = formatted_app("hola mundo", vec![bold(5, 10)], 7)?;
    app_frame(&mut app, &ctx, type_text("XY"));
    assert_eq!(app.blocks[0].content, "hola muXYndo");
    assert_eq!(marked_words(&app, 0), ["muXYndo"]);
    // At the end: still bold.
    app_frame(&mut app, &ctx, vec![key(Key::End, M::NONE)]);
    app_frame(&mut app, &ctx, type_text("!"));
    assert_eq!(marked_words(&app, 0), ["muXYndo!"]);
    // At the start of the line: not bold, and the bold word moves.
    app_frame(&mut app, &ctx, vec![key(Key::Home, M::NONE)]);
    app_frame(&mut app, &ctx, type_text("¡"));
    assert_eq!(app.blocks[0].content, "¡hola muXYndo!");
    assert_eq!(marked_words(&app, 0), ["muXYndo!"]);
    // Deleting inside the bold word shrinks it.
    app_frame(&mut app, &ctx, vec![key(Key::End, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(marked_words(&app, 0), ["muXYndo"]);

    // Saved with the text.
    app.save_now()?;
    let page = app.selected.clone().expect("page");
    let stored = app.db.list_blocks(&page)?;
    assert_eq!(stored[0].marks, app.blocks[0].marks);
    Ok(())
}

#[test]
fn enter_and_backspace_split_and_join_the_formatting() -> anyhow::Result<()> {
    // Caret between «mun» and «do».
    let (mut app, ctx) = formatted_app("hola mundo", vec![bold(5, 10)], 8)?;
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        contents(&app),
        [(Par, 0, "hola mun".into()), (Par, 0, "do".into())]
    );
    assert_eq!(marked_words(&app, 0), ["mun"]);
    assert_eq!(marked_words(&app, 1), ["do"]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(1));
    // Backspace at the start joins them back into one bold word.
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(contents(&app), [(Par, 0, "hola mundo".into())]);
    assert_eq!(app.blocks[0].marks, vec![bold(5, 10)]);
    Ok(())
}

#[test]
fn undo_brings_the_formatting_back() -> anyhow::Result<()> {
    let (mut app, ctx) = formatted_app("hola mundo", vec![bold(5, 10)], 10)?;
    // Delete the whole bold word, then undo.
    for _ in 0..5 {
        app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    }
    assert_eq!(app.blocks[0].content, "hola ");
    assert!(app.blocks[0].marks.is_empty());
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(app.blocks[0].content, "hola mundo");
    assert_eq!(app.blocks[0].marks, vec![bold(5, 10)]);
    Ok(())
}

#[test]
fn turning_a_block_into_code_drops_its_formatting_undoably() -> anyhow::Result<()> {
    let (mut app, ctx) = formatted_app("hola mundo", vec![bold(5, 10)], 0)?;
    let id = app.blocks[0].id.clone();
    app.block_action(BlockAction::Convert(Cod), &id);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks[0].kind, Cod);
    assert!(app.blocks[0].marks.is_empty());
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(app.blocks[0].kind, Par);
    assert_eq!(app.blocks[0].marks, vec![bold(5, 10)]);
    Ok(())
}

#[test]
fn ime_text_at_the_end_of_bold_stays_bold() -> anyhow::Result<()> {
    let (mut app, ctx) = formatted_app("negrita", vec![bold(0, 7)], 7)?;
    let preedit = |text: &str| {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: None,
        })
    };
    app_frame(&mut app, &ctx, vec![preedit("ka")]);
    app_frame(&mut app, &ctx, vec![preedit("kan")]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("漢".into()))],
    );
    assert_eq!(app.blocks[0].content, "negrita漢");
    assert_eq!(app.blocks[0].marks, vec![bold(0, 8)]);
    Ok(())
}

#[test]
fn formatted_text_is_painted_in_runs_over_the_same_text() -> anyhow::Result<()> {
    use silex_desktop::marks::{Mark, MarkKind};
    let text = "negrita, cursiva, código y enlace";
    let marks = vec![
        bold(0, 7),
        Mark::new(9, 16, MarkKind::Italic),
        Mark::new(18, 24, MarkKind::Code),
        Mark::new(27, 33, MarkKind::Link("https://ejemplo.com".into())),
    ];
    let (mut app, ctx) = formatted_app(text, marks, 0)?;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.update_ui(ui));
    output.textures_delta.clear();
    fn find(shape: &egui::Shape, text: &str) -> Option<std::sync::Arc<egui::Galley>> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some(t.galley.clone()),
            egui::Shape::Vec(inner) => inner.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    let galley = output
        .shapes
        .iter()
        .find_map(|c| find(&c.shape, text))
        .expect("the block's text is painted");
    let sections = &galley.job.sections;
    assert!(sections.len() >= 7, "{} runs", sections.len());
    let format_at = |byte: usize| {
        &sections
            .iter()
            .find(|s| (s.byte_range.start.0..s.byte_range.end.0).contains(&byte))
            .expect("covered")
            .format
    };
    let p = theme::current(&ctx);
    assert_eq!(
        format_at(0).font_id.family,
        egui::FontFamily::Name(theme::SEMIBOLD.into())
    );
    assert!(format_at(10).italics);
    assert_eq!(
        format_at(text.find("código").expect("word")).background,
        p.code_bg
    );
    let link = format_at(text.find("enlace").expect("word"));
    assert_eq!(link.color, p.accent_strong);
    assert_eq!(link.underline.color, p.accent_strong);
    assert_eq!(format_at(text.find(" y ").expect("word")).color, p.text);
    Ok(())
}

#[test]
fn find_and_replace_keeps_the_formatting_on_the_replaced_word() {
    use silex_desktop::editor::{find_in_blocks, replace_matches};
    let mut blocks = vec![Block {
        content: "pan y pan".into(),
        marks: vec![bold(6, 9)],
        ..block("a")
    }];
    let matches = find_in_blocks(&blocks, "pan");
    assert_eq!(replace_matches(&mut blocks, &matches, "centeno"), 2);
    assert_eq!(blocks[0].content, "centeno y centeno");
    assert_eq!(blocks[0].marks, vec![bold(10, 17)]);
}

// ---- Formatting shortcuts, menu and palette (H12.1c, ADR 0015) ----

/// Selects characters `anchor..cursor` of block `index`.
fn select_text(
    app: &mut SilexApp,
    ctx: &egui::Context,
    index: usize,
    anchor: usize,
    cursor: usize,
) {
    let id = app.blocks[index].id.clone();
    app.pending_focus = Some((id.clone(), cursor));
    app.pending_anchor = Some((id, anchor));
    app_frame(app, ctx, vec![]);
    app_frame(app, ctx, vec![]);
}

fn italic(start: usize, end: usize) -> silex_desktop::marks::Mark {
    silex_desktop::marks::Mark::new(start, end, silex_desktop::marks::MarkKind::Italic)
}

#[test]
fn shortcuts_format_the_selection_and_keep_it() -> anyhow::Result<()> {
    use silex_desktop::marks::{Mark, MarkKind};
    let (mut app, ctx) = editor_app(&[(Par, 0, "pan de centeno")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 0, 3);
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    assert_eq!(marked_words(&app, 0), ["pan"]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(
        block_selection(&app, &ctx, 0),
        Some((0, 3)),
        "still selected"
    );
    // Again: the whole selection is bold, so it goes.
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    assert!(app.blocks[0].marks.is_empty());
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);

    // A selection made backwards; the other shortcuts.
    select_text(&mut app, &ctx, 0, 14, 7);
    app_frame(&mut app, &ctx, vec![key(Key::I, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::E, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::X, M::COMMAND | M::SHIFT)]);
    assert_eq!(app.blocks[0].content, "pan de centeno", "no text was cut");
    assert_eq!(
        app.blocks[0].marks,
        vec![
            bold(0, 3),
            italic(7, 14),
            Mark::new(7, 14, MarkKind::Code),
            Mark::new(7, 14, MarkKind::Strike),
        ]
    );
    // Each change is one undo step.
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(app.blocks[0].marks.len(), 3);
    Ok(())
}

#[test]
fn with_nothing_selected_the_shortcut_formats_what_is_typed_next() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "Hoy ")], 0, 4)?;
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    assert!(app.blocks[0].marks.is_empty(), "nothing to format yet");
    app_frame(&mut app, &ctx, type_text("pan"));
    app_frame(&mut app, &ctx, type_text(" caliente"));
    assert_eq!(marked_words(&app, 0), ["pan caliente"]);
    // Cmd+B again at the end of bold: what follows is plain.
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    app_frame(&mut app, &ctx, type_text(" y café"));
    assert_eq!(app.blocks[0].content, "Hoy pan caliente y café");
    assert_eq!(marked_words(&app, 0), ["pan caliente"]);

    // Moving the cursor forgets it.
    app_frame(&mut app, &ctx, vec![key(Key::I, M::COMMAND)]);
    assert!(app.typing_format.is_some());
    app_frame(&mut app, &ctx, vec![key(Key::ArrowLeft, M::NONE)]);
    app_frame(&mut app, &ctx, vec![]);
    assert!(app.typing_format.is_none());
    app_frame(&mut app, &ctx, type_text("!"));
    assert!(
        !app.blocks[0]
            .marks
            .iter()
            .any(|m| m.kind == silex_desktop::marks::MarkKind::Italic)
    );
    Ok(())
}

#[test]
fn a_typing_format_survives_an_ime_composition() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "Kanji: ")], 0, 7)?;
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    let preedit = |text: &str| {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: None,
        })
    };
    app_frame(&mut app, &ctx, vec![preedit("ka")]);
    app_frame(&mut app, &ctx, vec![preedit("kan")]);
    app_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("漢".into()))],
    );
    app_frame(&mut app, &ctx, type_text("字"));
    assert_eq!(app.blocks[0].content, "Kanji: 漢字");
    assert_eq!(marked_words(&app, 0), ["漢字"]);
    Ok(())
}

#[test]
fn a_code_block_says_it_takes_no_formatting() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Cod, 0, "let x = 1;")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 0, 3);
    app_frame(&mut app, &ctx, vec![key(Key::B, M::COMMAND)]);
    assert!(app.blocks[0].marks.is_empty());
    assert!(
        app.notice.as_deref().is_some_and(|n| n.contains("código")),
        "{:?}",
        app.notice
    );
    Ok(())
}

#[test]
fn the_block_menu_formats_the_selection_or_the_whole_block() -> anyhow::Result<()> {
    use silex_desktop::marks::Format;
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno dos"), (Bul, 0, "tres")], 0, 0)?;
    // No selection: the whole block.
    let second = app.blocks[1].id.clone();
    app.block_action(BlockAction::Format(Format::Bold), &second);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(marked_words(&app, 1), ["tres"]);
    // With a selection: just that.
    select_text(&mut app, &ctx, 0, 4, 7);
    // Opening the menu takes the focus away (and egui drops the selection).
    let field = app.block_field_id(&app.blocks[0].id);
    ctx.memory_mut(|m| m.surrender_focus(field));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    let first = app.blocks[0].id.clone();
    app.block_action(BlockAction::Format(Format::Italic), &first);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.blocks[0].marks, vec![italic(4, 7)]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0), "focus back on the text");
    assert_eq!(block_selection(&app, &ctx, 0), Some((4, 7)));
    Ok(())
}

#[test]
fn the_palette_formats_the_last_block_edited() -> anyhow::Result<()> {
    use silex_desktop::marks::Format;
    use silex_desktop::palette::Command;
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno dos")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 0, 3);
    // The search window takes the focus away, and egui drops the selection
    // of a field without focus: the command still formats what was
    // selected. (Cmd+K over a selection makes a link instead, H12.1d.)
    app.open_search(&ctx);
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, type_text("negr"));
    assert_eq!(block_selection(&app, &ctx, 0), Some((3, 3)));
    assert!(app.search.is_some());
    assert!(Command::Format(Format::Bold).available(&app.palette_context()));
    app.search = None;
    app.run_command(&ctx, Command::Format(Format::Bold))?;
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(marked_words(&app, 0), ["uno"]);
    assert_eq!(block_selection(&app, &ctx, 0), Some((0, 3)));
    Ok(())
}

// ---- Web links (H12.1d, ADR 0015) ----

fn web_link(start: usize, end: usize, url: &str) -> silex_desktop::marks::Mark {
    silex_desktop::marks::Mark::new(start, end, silex_desktop::marks::MarkKind::Link(url.into()))
}

/// The URLs this frame asked the system to open.
fn opened_urls(app: &mut SilexApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<String> {
    app_frame_commands(app, ctx, events)
        .iter()
        .filter_map(|c| match c {
            egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
            _ => None,
        })
        .collect()
}

fn url_field_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.has_focus(super::web_links::url_field_id()))
}

#[test]
fn cmd_k_over_a_selection_gives_it_a_link() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "lee la documentación")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 7, 20);
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.search.is_none(), "not the search window");
    let edit = app.link_edit.as_ref().expect("link field open");
    assert_eq!((edit.start, edit.end), (7, 20));
    app_frame(&mut app, &ctx, vec![]);
    assert!(url_field_focused(&ctx));
    app_frame(&mut app, &ctx, type_text("ejemplo.com/docs"));
    assert_eq!(
        app.blocks[0].content, "lee la documentación",
        "typed in the field"
    );
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert!(app.link_edit.is_none());
    assert_eq!(
        app.blocks[0].marks,
        vec![web_link(7, 20, "https://ejemplo.com/docs")]
    );
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(focused_block(&app, &ctx), Some(0), "focus back on the text");
    assert_eq!(block_selection(&app, &ctx, 0), Some((7, 20)));
    // One undo step.
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert!(app.blocks[0].marks.is_empty());
    Ok(())
}

#[test]
fn cmd_k_with_nothing_selected_still_opens_the_search_window() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno dos")], 0, 2)?;
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.search.is_some());
    assert!(app.link_edit.is_none());
    Ok(())
}

#[test]
fn a_url_that_is_not_allowed_is_refused_and_esc_changes_nothing() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "pulsa aquí")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 6, 10);
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![]);
    for url in ["javascript:alert(1)", "file:///etc/passwd"] {
        app.link_edit.as_mut().expect("open").url = url.into();
        app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
        let edit = app.link_edit.as_ref().expect("still open");
        assert!(edit.error.is_some(), "{url}");
        assert!(app.blocks[0].marks.is_empty());
    }
    app_frame(&mut app, &ctx, vec![]);
    assert!(url_field_focused(&ctx), "the field keeps the keyboard");
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    assert!(app.link_edit.is_none());
    assert!(app.blocks[0].marks.is_empty());
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    assert_eq!(block_selection(&app, &ctx, 0), Some((6, 10)));
    Ok(())
}

#[test]
fn an_empty_url_removes_the_link_and_the_field_starts_with_it() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ver la web")], 0, 0)?;
    app.blocks[0].marks = vec![bold(0, 3), web_link(4, 10, "https://a.es")];
    app_frame(&mut app, &ctx, vec![]);
    select_text(&mut app, &ctx, 0, 7, 10);
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert_eq!(
        app.link_edit.as_ref().map(|e| e.url.as_str()),
        Some("https://a.es")
    );
    app_frame(&mut app, &ctx, vec![]);
    // Select all in the field and delete it.
    app_frame(&mut app, &ctx, vec![key(Key::A, M::COMMAND)]);
    app_frame(&mut app, &ctx, vec![key(Key::Backspace, M::NONE)]);
    assert_eq!(app.link_edit.as_ref().map(|e| e.url.as_str()), Some(""));
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(
        app.blocks[0].marks,
        vec![bold(0, 3), web_link(4, 7, "https://a.es")],
        "only the selected part loses it"
    );
    Ok(())
}

#[test]
fn the_menu_edits_the_link_at_the_cursor_or_links_the_whole_block() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ver la web"), (Bul, 0, "dos")], 0, 6)?;
    app.blocks[0].marks = vec![web_link(4, 10, "https://a.es")];
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    let first = app.blocks[0].id.clone();
    // The cursor is in the link: that link.
    app.block_action(BlockAction::Link, &first);
    app_frame(&mut app, &ctx, vec![]);
    let edit = app.link_edit.as_ref().expect("open");
    assert_eq!(
        (edit.start, edit.end, edit.url.as_str()),
        (4, 10, "https://a.es")
    );
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    // No link and nothing selected: the whole block.
    let second = app.blocks[1].id.clone();
    app.block_action(BlockAction::Link, &second);
    app_frame(&mut app, &ctx, vec![]);
    app.link_edit.as_mut().expect("open").url = "https://b.es".into();
    app_frame(&mut app, &ctx, vec![key(Key::Enter, M::NONE)]);
    assert_eq!(app.blocks[1].marks, vec![web_link(0, 3, "https://b.es")]);
    Ok(())
}

#[test]
fn the_palette_links_the_last_text_selected() -> anyhow::Result<()> {
    use silex_desktop::palette::Command;
    let (mut app, ctx) = editor_app(&[(Par, 0, "uno dos")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 4, 7);
    app.open_search(&ctx);
    app_frame(&mut app, &ctx, vec![]);
    assert!(Command::Link.available(&app.palette_context()));
    app.search = None;
    app.run_command(&ctx, Command::Link)?;
    app_frame(&mut app, &ctx, vec![]);
    let edit = app.link_edit.as_ref().expect("open");
    assert_eq!((edit.start, edit.end), (4, 7));
    Ok(())
}

#[test]
fn a_code_block_takes_no_link() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Cod, 0, "let x = 1;")], 0, 0)?;
    select_text(&mut app, &ctx, 0, 0, 3);
    app_frame(&mut app, &ctx, vec![key(Key::K, M::COMMAND)]);
    assert!(app.link_edit.is_none());
    assert!(app.search.is_none());
    assert!(app.notice.as_deref().is_some_and(|n| n.contains("código")));
    Ok(())
}

#[test]
fn cmd_enter_opens_the_link_at_the_cursor_and_nothing_else() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "ver la web y más")], 0, 6)?;
    app.blocks[0].marks = vec![web_link(7, 10, "https://ejemplo.com/a")];
    app_frame(&mut app, &ctx, vec![]);
    // Outside the link: no URL.
    assert!(opened_urls(&mut app, &ctx, vec![key(Key::Enter, M::COMMAND)]).is_empty());
    app.pending_focus = Some((app.blocks[0].id.clone(), 8));
    app_frame(&mut app, &ctx, vec![]);
    app_frame(&mut app, &ctx, vec![]);
    let before = app.blocks[0].content.clone();
    assert_eq!(
        opened_urls(&mut app, &ctx, vec![key(Key::Enter, M::COMMAND)]),
        ["https://ejemplo.com/a"]
    );
    assert_eq!(app.blocks[0].content, before, "no new line");
    assert_eq!(app.blocks.len(), 1, "no new block");
    Ok(())
}

#[test]
fn screen_readers_hear_the_links_of_a_block() {
    use super::web_links::links_description;
    assert_eq!(links_description("sin enlaces", &[bold(0, 3)]), None);
    assert_eq!(
        links_description("lee la guía", &[web_link(7, 11, "https://a.es")]).as_deref(),
        Some("Enlace: guía (https://a.es)")
    );
    assert_eq!(
        links_description(
            "uno dos",
            &[
                web_link(0, 3, "https://a.es"),
                web_link(4, 7, "mailto:b@c.es")
            ]
        )
        .as_deref(),
        Some("Enlaces: uno (https://a.es); dos (mailto:b@c.es)")
    );
}

// ---- Copy and paste with formatting (H12.1e, ADR 0015) ----

/// Focuses block `index` with the cursor at `char`.
fn focus_at(app: &mut SilexApp, ctx: &egui::Context, index: usize, char: usize) {
    app.pending_focus = Some((app.blocks[index].id.clone(), char));
    app_frame(app, ctx, vec![]);
    app_frame(app, ctx, vec![]);
}

#[test]
fn text_copied_in_a_block_is_pasted_with_its_formatting() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "pan negro"), (Par, 0, "un  más")], 0, 0)?;
    app.blocks[0].marks = vec![bold(4, 9)];
    app.blocks[1].marks = vec![italic(0, 7)];
    app.mark_dirty();
    app_frame(&mut app, &ctx, vec![]);
    // Copy «n negro»: the bold part comes along.
    select_text(&mut app, &ctx, 0, 2, 9);
    let commands = app_frame_commands(&mut app, &ctx, vec![egui::Event::Copy]);
    assert!(commands.contains(&egui::OutputCommand::CopyText("n negro".into())));
    // Pasted inside the italic text: exactly its own formatting.
    focus_at(&mut app, &ctx, 1, 3);
    app_frame(&mut app, &ctx, vec![paste("n negro")]);
    assert_eq!(app.blocks[1].content, "un n negro más");
    assert_eq!(
        app.blocks[1].marks,
        vec![italic(0, 3), bold(5, 10), italic(10, 14)]
    );
    // One undo step takes the paste away, formatting included.
    app_frame(&mut app, &ctx, vec![key(Key::Z, M::COMMAND)]);
    assert_eq!(app.blocks[1].content, "un  más");
    assert_eq!(app.blocks[1].marks, vec![italic(0, 7)]);
    Ok(())
}

#[test]
fn other_text_pasted_follows_the_text_around_it() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "pan negro"), (Par, 0, "abc")], 0, 0)?;
    app.blocks[0].marks = vec![bold(4, 9)];
    app.blocks[1].marks = vec![bold(0, 3)];
    app_frame(&mut app, &ctx, vec![]);
    select_text(&mut app, &ctx, 0, 0, 3);
    app_frame(&mut app, &ctx, vec![egui::Event::Copy]);
    focus_at(&mut app, &ctx, 1, 1);
    // Not what was copied: it takes the bold it is pasted into.
    app_frame(&mut app, &ctx, vec![paste("xyz")]);
    assert_eq!(app.blocks[1].marks, vec![bold(0, 6)]);
    Ok(())
}

#[test]
fn copied_lines_and_blocks_are_pasted_with_their_formatting() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(
        &[(Bul, 0, "pan negro"), (Bul, 0, "café"), (Par, 0, "")],
        0,
        0,
    )?;
    app.blocks[0].marks = vec![bold(4, 9)];
    app.blocks[1].marks = vec![italic(0, 4)];
    app_frame(&mut app, &ctx, vec![]);
    // A block selection, copied.
    app_frame(&mut app, &ctx, vec![key(Key::Escape, M::NONE)]);
    app_frame(&mut app, &ctx, vec![key(Key::ArrowDown, M::SHIFT)]);
    assert_eq!(app.selection_range(), Some((0, 1)));
    app_frame(&mut app, &ctx, vec![egui::Event::Copy]);
    let copied = app.clip.as_ref().map(|c| c.text.clone());
    assert_eq!(copied.as_deref(), Some("• pan negro\n• café"));
    // Pasted into the empty paragraph: two list items, as they were.
    app.selection = None;
    focus_at(&mut app, &ctx, 2, 0);
    app_frame(&mut app, &ctx, vec![paste("• pan negro\r\n• café")]);
    let pasted: Vec<_> = app.blocks[2..]
        .iter()
        .map(|b| (b.kind, b.content.as_str(), b.marks.clone()))
        .collect();
    assert_eq!(
        pasted,
        vec![
            (Bul, "pan negro", vec![bold(4, 9)]),
            (Bul, "café", vec![italic(0, 4)]),
        ]
    );
    Ok(())
}

#[test]
fn copying_elsewhere_forgets_the_formatting() -> anyhow::Result<()> {
    let (mut app, ctx) = editor_app(&[(Par, 0, "pan negro"), (Par, 0, "abc")], 0, 0)?;
    app.blocks[0].marks = vec![bold(0, 9)];
    app_frame(&mut app, &ctx, vec![]);
    select_text(&mut app, &ctx, 0, 0, 3);
    app_frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert!(app.clip.is_some());
    // Copy with no text selected in a block (another field, say).
    focus_at(&mut app, &ctx, 1, 1);
    app_frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert!(app.clip.is_none());
    app_frame(&mut app, &ctx, vec![paste("pan")]);
    assert!(app.blocks[1].marks.is_empty());
    Ok(())
}
