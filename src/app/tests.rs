use super::*;

use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    db::Database,
    model::{Block, BlockKind},
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
