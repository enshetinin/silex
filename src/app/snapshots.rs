//! UI snapshots with `egui_kittest`: the real app rendered headless to PNG
//! (`tests/snapshots/*.png`). They are for reviewing the design and catching
//! unintended visual changes.
//!
//! Rendering differs slightly between operating systems and GPUs, and CI
//! runners may have no GPU adapter, so these only run when asked:
//!
//! ```text
//! SILEX_SNAPSHOTS=1 cargo test --bin silex-desktop snapshots   # compare
//! SILEX_SNAPSHOTS=1 UPDATE_SNAPSHOTS=1 cargo test --bin silex-desktop snapshots   # accept
//! ```
//!
//! Reference images are produced on macOS.
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use silex_desktop::db::Database;
use silex_desktop::model::BlockKind;

use super::{SilexApp, theme};

fn enabled() -> bool {
    let on = std::env::var_os("SILEX_SNAPSHOTS").is_some();
    if !on {
        eprintln!("snapshot tests skipped (set SILEX_SNAPSHOTS=1 to run them)");
    }
    on
}

/// A small workspace with nested pages, a favorite and every block type.
fn demo_app() -> anyhow::Result<SilexApp> {
    let mut db = Database::open_in_memory()?;
    let proyecto = db.create_page("Proyecto Silex", None)?;
    let diseno = db.create_page("Diseño de la interfaz", Some(&proyecto))?;
    db.create_page("Ideas sueltas", Some(&diseno))?;
    db.create_page("Notas de lectura", None)?;
    db.create_page("Recetas", None)?;
    db.set_favorite(&diseno, true)?;
    let blocks = [
        (BlockKind::Heading1, 0, false, "Objetivo"),
        (
            BlockKind::Paragraph,
            0,
            false,
            "Una interfaz limpia, con mucho aire y un único color de acento.",
        ),
        (BlockKind::Heading2, 0, false, "Tareas"),
        (BlockKind::Todo, 0, true, "Elegir tipografía"),
        (BlockKind::Todo, 0, false, "Diseñar la barra lateral"),
        (BlockKind::Heading3, 0, false, "Principios"),
        (BlockKind::Bulleted, 0, false, "Menos bordes, más espacio"),
        (BlockKind::Bulleted, 1, false, "Acciones al pasar el ratón"),
        (
            BlockKind::Numbered,
            0,
            false,
            "Contraste AA en claro y oscuro",
        ),
        (BlockKind::Numbered, 0, false, "Todo accesible con teclado"),
        (
            BlockKind::Quote,
            0,
            false,
            "La simplicidad es la máxima sofisticación.",
        ),
        (BlockKind::Code, 0, false, "theme::install(&ctx);"),
    ];
    let new: Vec<_> = blocks
        .iter()
        .map(
            |(kind, indent, checked, content)| silex_desktop::model::NewBlock {
                kind: *kind,
                indent: *indent,
                checked: *checked,
                content: (*content).to_owned(),
                marks: Vec::new(),
            },
        )
        .collect();
    // Replace the page with a populated one and open it.
    let page = db.create_page_with_blocks("Guía de estilo", Some(&proyecto), &new)?;
    let mut app = SilexApp::new(db)?;
    app.expanded.insert(proyecto);
    app.expanded.insert(diseno);
    app.reload_pages()?;
    app.open_page(page)?;
    Ok(app)
}

fn snapshot(name: &str, theme: eframe::egui::Theme) -> anyhow::Result<()> {
    snapshot_with(name, theme, |_| {})
}

/// Renders the demo app, lets `interact` act on it (hover, click…), and
/// compares with the reference image.
fn snapshot_with(
    name: &str,
    theme: eframe::egui::Theme,
    interact: impl FnOnce(&mut Harness<'_, SilexApp>),
) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let app = demo_app()?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    interact(&mut harness);
    harness.run();
    harness.snapshot(name);
    Ok(())
}

#[test]
fn main_window_light() -> anyhow::Result<()> {
    snapshot("main_window_light", eframe::egui::Theme::Light)
}

#[test]
fn main_window_dark() -> anyhow::Result<()> {
    snapshot("main_window_dark", eframe::egui::Theme::Dark)
}

#[test]
fn sidebar_row_hover_shows_actions() -> anyhow::Result<()> {
    snapshot_with(
        "sidebar_hover_light",
        eframe::egui::Theme::Light,
        |harness| {
            // The row (a button with the page title as its accessible name).
            if let Some(row) = harness
                .get_all_by_label("Notas de lectura")
                .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            {
                row.hover();
            }
        },
    )
}

#[test]
fn block_handle_menu() -> anyhow::Result<()> {
    snapshot_with("block_menu_light", eframe::egui::Theme::Light, |harness| {
        // Open the handle menu of the paragraph block (its id is stable).
        let paragraph = harness.state().blocks[1].id.clone();
        egui::Popup::open_id(&harness.ctx, super::blocks::handle_menu_id(&paragraph));
    })
}

/// H12.1c: «Formato» open in the paragraph's menu, with each format's
/// shortcut.
#[test]
fn block_format_menu() -> anyhow::Result<()> {
    snapshot_with(
        "block_format_menu_light",
        eframe::egui::Theme::Light,
        |harness| {
            let paragraph = harness.state().blocks[1].id.clone();
            egui::Popup::open_id(&harness.ctx, super::blocks::handle_menu_id(&paragraph));
            harness.run();
            harness.get_by_label_contains("Formato").hover();
            harness.run();
        },
    )
}

/// H11.7: the style guide after Cmd+Opción+F, «in» searched and «IN»
/// typed as its replacement. The bar sits above the page, with
/// «Reemplazar»; every match is painted, the one shown stronger (also in
/// the code block, which has its own background).
fn find_bar_harness(theme: egui::Theme) -> anyhow::Result<Harness<'static, SilexApp>> {
    use eframe::egui::{Key, Modifiers};
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), demo_app()?);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::F);
    harness.run();
    harness.get_by_label("Buscar en la página").type_text("in");
    harness.run();
    harness.get_by_label("Reemplazar por").focus();
    harness.run();
    harness.get_by_label("Reemplazar por").type_text("IN");
    harness.run();
    Ok(harness)
}

fn contents_of(harness: &Harness<'_, SilexApp>) -> Vec<String> {
    harness
        .state()
        .blocks
        .iter()
        .map(|b| b.content.clone())
        .collect()
}

/// Each control has a name and the counter is read out; «Reemplazar todo»
/// from its button is one Cmd+Z; «×» closes the bar.
#[test]
fn the_find_bar_highlights_and_replaces() -> anyhow::Result<()> {
    use eframe::egui::{Key, Modifiers};
    let mut harness = find_bar_harness(egui::Theme::Light)?;
    // «interfaz», «Principios», «install».
    assert_eq!(
        harness
            .state()
            .find
            .as_ref()
            .map(|f| f.counter())
            .as_deref(),
        Some("1 de 3")
    );
    assert!(
        harness.query_by_label("1 de 3").is_some(),
        "the counter is read out"
    );
    assert_accessible(&harness, "find bar");
    if enabled() {
        harness.snapshot("find_bar_light");
    }
    let before = contents_of(&harness);
    click_button(&mut harness, "Reemplazar todo");
    let after = contents_of(&harness);
    assert!(
        after.iter().any(|t| t == "theme::INstall(&ctx);"),
        "{after:?}"
    );
    assert!(after.iter().any(|t| t.contains("PrINcipios")), "{after:?}");
    assert!(harness.query_by_label("3 reemplazadas").is_some());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(contents_of(&harness), before, "one Cmd+Z undoes them all");
    click_button(&mut harness, "Cerrar la búsqueda");
    assert!(harness.state().find.is_none());
    Ok(())
}

#[test]
fn the_find_bar_dark() -> anyhow::Result<()> {
    let mut harness = find_bar_harness(egui::Theme::Dark)?;
    if enabled() {
        harness.snapshot("find_bar_dark");
    }
    Ok(())
}

// ---- Accessibility: no rendering needed, so these always run (also in CI). ----

/// The demo app in a harness, settled.
fn a11y_harness(app: SilexApp) -> Harness<'static, SilexApp> {
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness
}

/// Every control and text field has a name, and no name or text is only an
/// icon glyph (Private Use Area characters mean nothing to a screen reader).
#[track_caller]
fn assert_accessible(harness: &Harness<'_, SilexApp>, state: &str) {
    let unnamed = harness.unnamed_widgets();
    assert!(
        unnamed.is_empty(),
        "{state}: unnamed widgets:\n{unnamed:#?}"
    );
    let nodes: Vec<_> = harness
        .root()
        .children_recursive()
        .map(|node| node.accesskit_node())
        .filter(|node| !node.is_hidden())
        .collect();
    // A text field's value is what was typed: it needs a name of its own
    // («Título de la página», «Nombre, Rayuela»…).
    let unnamed_fields: Vec<String> = nodes
        .iter()
        .filter(|node| node.is_text_input() && !node.label().is_some_and(|l| !l.trim().is_empty()))
        .map(|node| {
            format!(
                "{:?} {:?} at {:?}",
                node.role(),
                node.value(),
                node.raw_bounds()
            )
        })
        .collect();
    assert!(
        unnamed_fields.is_empty(),
        "{state}: text fields without a name:\n{unnamed_fields:#?}"
    );
    let is_icon = |c: char| ('\u{E000}'..='\u{F8FF}').contains(&c);
    let only_icons = |text: &str| {
        !text.trim().is_empty() && !text.chars().any(|c| !c.is_whitespace() && !is_icon(c))
    };
    let icon_only: Vec<String> = nodes
        .iter()
        .filter_map(|node| {
            let text = node.label().filter(|l| only_icons(l)).or_else(|| {
                (!node.is_text_input())
                    .then(|| node.value())
                    .flatten()
                    .filter(|v| only_icons(v))
            })?;
            Some(format!(
                "{:?} {text:?} at {:?}",
                node.role(),
                node.raw_bounds()
            ))
        })
        .collect();
    assert!(
        icon_only.is_empty(),
        "{state}: icon-only names or text:\n{icon_only:#?}"
    );
}

/// The sidebar's "Buscar" button (its name also holds the shortcut).
fn search_button<'h>(harness: &'h Harness<'_, SilexApp>) -> egui_kittest::Node<'h> {
    harness
        .get_all_by_label_contains("Buscar")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("search button")
}

/// Opens the search window from the sidebar and types `query`.
fn open_search(harness: &mut Harness<'_, SilexApp>, query: &str) {
    search_button(harness).click();
    harness.run();
    harness.event(egui::Event::Text(query.into()));
    harness.run();
}

/// A button or cell clicked by its accessibility action, not the pointer:
/// in the test window the table is wider than its room, and «Estado» is
/// partly scrolled away.
fn click_action(harness: &mut Harness<'_, SilexApp>, label: &str) {
    harness
        .get_all_by_label(label)
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap_or_else(|| panic!("control {label:?}"))
        .click_accesskit();
    harness.run();
}

fn click_button(harness: &mut Harness<'_, SilexApp>, label: &str) {
    harness
        .get_all_by_label(label)
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap_or_else(|| panic!("button {label:?}"))
        .click();
    harness.run();
}

#[test]
fn every_control_has_a_name() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    assert_accessible(&harness, "at rest");

    // Row actions appear on hover.
    harness
        .get_all_by_label("Notas de lectura")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("row")
        .hover();
    harness.run();
    assert_accessible(&harness, "row hover");
    assert!(harness.query_by_label("Más acciones").is_some());
    assert!(harness.query_by_label("Nueva subpágina").is_some());

    let paragraph = harness.state().blocks[1].id.clone();
    egui::Popup::open_id(&harness.ctx, super::blocks::handle_menu_id(&paragraph));
    harness.run();
    assert_accessible(&harness, "block menu");
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    for button in [
        "Más acciones de la página",
        "Ajustes",
        "Copias y exportación",
        "Atajos de teclado",
    ] {
        click_button(&mut harness, button);
        assert_accessible(&harness, button);
        egui::Popup::close_all(&harness.ctx);
        harness.run();
    }

    search_button(&harness).click();
    harness.run();
    assert_accessible(&harness, "command palette");
    let command = harness
        .get_all_by_label("Nueva página (Cmd+N)")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button);
    assert!(command.is_some(), "command row is named with its shortcut");
    harness.key_press(egui::Key::Escape);
    harness.run();

    harness.state_mut().shortcuts_dialog = true;
    harness.run();
    assert_accessible(&harness, "shortcuts dialog");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shortcuts_dialog);

    open_search(&mut harness, "dise");
    assert_accessible(&harness, "search results");
    // Each result is a button named with its title, path and snippet.
    let result = harness
        .get_all_by_label_contains("Diseño de la interfaz, en Proyecto Silex")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button);
    assert!(result.is_some(), "result row is named");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().search.is_none());

    harness.state_mut().selected = None;
    harness.run();
    assert_accessible(&harness, "no page open");
    Ok(())
}

/// The names Tab visits, `steps` times from what has the focus.
fn tab_names(harness: &mut Harness<'_, SilexApp>, steps: usize) -> Vec<String> {
    (0..steps)
        .map(|_| {
            harness.key_press(egui::Key::Tab);
            harness.run();
            harness
                .root()
                .children_recursive()
                .map(|node| node.accesskit_node())
                .find(|node| node.is_focused())
                .and_then(|node| node.label())
                .unwrap_or_default()
        })
        .collect()
}

/// Positions of `names` in `visited`, which must exist and go up.
#[track_caller]
fn assert_visited_in_order(visited: &[String], names: &[&str]) {
    let at: Vec<Option<usize>> = names
        .iter()
        .map(|name| visited.iter().position(|v| v == name))
        .collect();
    assert!(
        at.iter().all(Option::is_some) && at.windows(2).all(|w| w[0] < w[1]),
        "expected {names:?} in this order, Tab visited {visited:#?}"
    );
}

/// Tab goes through the window in the order it is seen (H9.3a): the
/// sidebar top to bottom, footer last; the page's buttons and the
/// table's toolbar left to right.
#[test]
fn tab_follows_the_order_on_screen() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    search_button(&harness).focus();
    harness.run();
    let visited = tab_names(&mut harness, 24);
    assert_visited_in_order(
        &visited,
        &[
            "Diseño de la interfaz",
            "Plantillas",
            "Añadir",
            "Proyecto Silex",
            "Recetas",
            "Papelera",
            "Copias y exportación",
            "Atajos de teclado",
            "Añadir a favoritos",
            "Más acciones de la página",
            "Título de la página",
        ],
    );
    // The path in the top bar is a link to the page above.
    let link = harness.get_by_label("Abrir «Proyecto Silex»");
    assert_eq!(link.accesskit_node().role(), egui::accesskit::Role::Link);

    let (app, _, _) = database_app()?;
    let mut harness = a11y_harness(app);
    harness.get_by_label("Título de la página").focus();
    harness.run();
    let visited = tab_names(&mut harness, 8);
    assert_visited_in_order(
        &visited,
        &[
            "Vista «Tabla»",
            "Nueva vista",
            "Filtrar",
            "Ordenar",
            "Columnas",
            "Añadir una propiedad",
        ],
    );
    Ok(())
}

#[test]
fn settings_menu_changes_and_stores_the_theme_and_size() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    click_button(&mut harness, "Ajustes");
    harness.get_by_label("Oscuro").click();
    harness.run();
    harness.get_by_label("125 %").click();
    harness.run();
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    assert!((harness.ctx.zoom_factor() - 1.25).abs() < 1e-6);
    let stored = harness.state().db.load_settings()?;
    assert_eq!(stored.theme, silex_desktop::settings::ThemeMode::Dark);
    // ADR 0014: the version and «Buscar actualizaciones» in the same menu.
    let version = format!("Silex {}", silex_desktop::about::VERSION);
    assert!(harness.query_by_label(&version).is_some(), "{version}");
    assert!(menu_item(&harness, "Buscar actualizaciones…").is_some());
    assert_eq!(stored.zoom_percent, 125);
    Ok(())
}

#[test]
fn stored_settings_apply_at_startup_and_keyboard_zoom_is_remembered() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.save_settings(&silex_desktop::settings::Settings {
        theme: silex_desktop::settings::ThemeMode::Dark,
        zoom_percent: 125,
        ..Default::default()
    })?;
    let mut harness = a11y_harness(SilexApp::new(db)?);
    assert_eq!(harness.ctx.theme(), egui::Theme::Dark);
    assert!((harness.ctx.zoom_factor() - 1.25).abs() < 1e-6);

    // What egui does on Cmd+− (outside our code), and an out-of-range zoom.
    harness.ctx.set_zoom_factor(1.1);
    harness.run();
    assert_eq!(harness.state().db.load_settings()?.zoom_percent, 110);
    harness.ctx.set_zoom_factor(4.0);
    harness.run();
    assert_eq!(harness.state().db.load_settings()?.zoom_percent, 200);
    assert!((harness.ctx.zoom_factor() - 2.0).abs() < 1e-6, "clamped");
    Ok(())
}

#[test]
fn settings_menu() -> anyhow::Result<()> {
    snapshot_with(
        "settings_menu_light",
        eframe::egui::Theme::Light,
        |harness| {
            click_button(harness, "Ajustes");
        },
    )
}

#[test]
fn keyboard_focus_is_visible() -> anyhow::Result<()> {
    snapshot_with(
        "keyboard_focus_light",
        eframe::egui::Theme::Light,
        |harness| {
            // Tab from the search button: favorites, «Plantillas», then
            // «Páginas +», as they are seen (H9.3a).
            search_button(harness).focus();
            harness.run();
            for _ in 0..3 {
                harness.key_press(egui::Key::Tab);
                harness.run();
            }
        },
    )
}

#[test]
fn row_menu_stays_open_while_the_pointer_moves_into_it() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    let row = |h: &Harness<'_, SilexApp>| {
        h.get_all_by_label("Notas de lectura")
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .expect("row")
            .rect()
    };
    let row_rect = row(&harness);
    harness.hover_at(row_rect.center());
    harness.run();
    click_button(&mut harness, "Más acciones");
    assert!(harness.query_by_label("Abrir").is_some(), "menu opened");

    // Leave the row and go over a menu item, as a user does.
    let item = harness.get_by_label("Abrir").rect();
    assert!(!row_rect.contains(item.center()));
    harness.hover_at(item.center());
    harness.run();
    assert!(
        harness.query_by_label("Abrir").is_some(),
        "menu closed on hover"
    );
    harness.get_by_label("Abrir").click();
    harness.run();
    assert_eq!(
        harness.state().title,
        "Notas de lectura",
        "the menu action ran"
    );
    Ok(())
}

#[test]
fn slash_menu_lists_options_in_a_column() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    click_button(&mut harness, "Añadir bloque");
    harness.event(egui::Event::Text("/".into()));
    harness.run();
    assert!(harness.state().slash.is_some(), "slash menu opened");
    assert_accessible(&harness, "slash menu");
    let option = |label: &str| {
        harness
            .get_all_by_label_contains(label)
            .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap_or_else(|| panic!("option {label:?}"))
            .rect()
    };
    let (first, second) = (option("Título 1"), option("Título 2"));
    assert!(
        second.top() >= first.bottom() - 0.5,
        "{first:?} / {second:?}"
    );
    assert!((second.left() - first.left()).abs() < 0.5, "same column");
    Ok(())
}

#[test]
fn slash_menu() -> anyhow::Result<()> {
    snapshot_with("slash_menu_light", eframe::egui::Theme::Light, |harness| {
        click_button(harness, "Añadir bloque");
        harness.event(egui::Event::Text("/".into()));
    })
}

#[test]
fn search_window() -> anyhow::Result<()> {
    snapshot_with(
        "search_window_light",
        eframe::egui::Theme::Light,
        |harness| {
            open_search(harness, "dise");
        },
    )
}

#[test]
fn search_window_dark() -> anyhow::Result<()> {
    snapshot_with("search_window_dark", eframe::egui::Theme::Dark, |harness| {
        open_search(harness, "dise");
    })
}

#[test]
fn command_palette() -> anyhow::Result<()> {
    snapshot_with(
        "command_palette_light",
        eframe::egui::Theme::Light,
        |harness| {
            search_button(harness).click();
        },
    )
}

// ---- Startup error window (shown when the app cannot open its data). ----

fn startup_error_harness() -> Harness<'static, super::StartupError> {
    let err = anyhow::anyhow!("La base de datos usa un esquema más reciente (9)");
    let path = std::path::Path::new(
        "/Users/ana/Library/Application Support/dev.silex.Silex/silex.sqlite3",
    );
    let mut harness = Harness::builder()
        .with_size([760.0, 420.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .build_ui_state(
            |ui, window: &mut super::StartupError| window.update_ui(ui),
            super::StartupError::new(&err, Some(path)),
        );
    harness.run();
    harness
}

#[test]
fn startup_error_shows_the_error_and_copies_it() {
    let mut harness = startup_error_harness();
    let unnamed = harness.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed widgets:\n{unnamed:#?}");
    assert!(
        harness
            .query_by_label_contains("esquema más reciente")
            .is_some()
    );
    assert!(harness.query_by_label_contains("dev.silex.Silex").is_some());
    harness.get_by_label("Copiar el error").click();
    // One frame: its output holds the copy command.
    harness.step();
    let copied = harness
        .output()
        .platform_output
        .commands
        .iter()
        .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("esquema") && t.contains("silex.sqlite3")));
    assert!(copied, "the error and the file are copied");
    assert!(harness.query_by_label("Cerrar").is_some());
}

#[test]
fn startup_error() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = startup_error_harness();
    harness.run();
    harness.snapshot("startup_error_light");
    Ok(())
}

// ---- Databases (H6.1) ----

/// The demo app with a database «Lecturas» open: three rows with an
/// author, a page count, a «Leído» box, a date and a state (H6.2).
fn database_app() -> anyhow::Result<(SilexApp, String, Vec<String>)> {
    use silex_desktop::property::{CellValue, Date, OptionColor, PropertyKind};
    let mut app = demo_app()?;
    let db = &mut app.db;
    let base = db.create_database("Lecturas", None)?;
    let rows = ["Rayuela", "Pedro Páramo", "Ficciones"]
        .into_iter()
        .map(|title| db.create_row(&base, title))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let paginas = db.add_property(&base, "Páginas", PropertyKind::Number)?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let fecha = db.add_property(&base, "Terminado", PropertyKind::Date)?;
    let terminado = db.add_select_option(&estado, "Terminado", OptionColor::Green)?;
    let leyendo = db.add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
    let text = |t: &str| CellValue::Text(t.into());
    let cells = [
        (0, &autor, text("Julio Cortázar")),
        (1, &autor, text("Juan Rulfo")),
        (2, &autor, text("Jorge Luis Borges")),
        (0, &paginas, CellValue::Number(736.0)),
        (1, &paginas, CellValue::Number(124.0)),
        (0, &leido, CellValue::Checkbox(true)),
        (0, &estado, CellValue::Select(terminado)),
        (2, &estado, CellValue::Select(leyendo)),
    ];
    for (row, property, value) in cells {
        db.set_value(&rows[row], property, Some(&value))?;
    }
    if let Some(date) = Date::parse("2026-09-14") {
        db.set_value(&rows[0], &fecha, Some(&CellValue::Date(date)))?;
    }
    app.reload_pages()?;
    app.open_page(base.clone())?;
    Ok((app, base, rows))
}

#[test]
fn database_rows_open_as_pages_and_every_control_has_a_name() -> anyhow::Result<()> {
    let (app, base, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    assert_accessible(&harness, "database table");
    assert!(harness.query_by_label("3 filas").is_some());
    click_button(&mut harness, "Abrir «Pedro Páramo»");
    assert_eq!(harness.state().selected.as_deref(), Some(rows[1].as_str()));
    assert!(harness.state().database.is_none(), "a row is a page");
    // Its breadcrumb leads back to the database.
    click_button(&mut harness, "Lecturas");
    assert_eq!(harness.state().selected.as_deref(), Some(base.as_str()));
    Ok(())
}

/// H11.5: «Más acciones» offers both CSV exports; the view one leaves out
/// what the view hides, the other writes everything. Written on the export
/// thread with a notice while it runs.
#[test]
fn a_database_exports_its_view_or_everything_to_csv() -> anyhow::Result<()> {
    let (app, _, _) = database_app()?;
    let mut harness = a11y_harness(app);
    click_button(&mut harness, "Más acciones de la página");
    assert!(menu_item(&harness, "Exportar la vista a CSV").is_some());
    assert!(menu_item(&harness, "Exportar toda la base a CSV").is_some());
    assert_accessible(&harness, "database page menu");
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    // The view hides «Autor» and shows only rows with a page count.
    {
        let state = harness.state_mut().database.as_mut().expect("database");
        let autor = state
            .properties
            .iter()
            .find(|p| p.name == "Autor")
            .map(|p| p.id.clone())
            .expect("Autor");
        let paginas = state
            .properties
            .iter()
            .find(|p| p.name == "Páginas")
            .map(|p| p.id.clone())
            .expect("Páginas");
        let view = state.view;
        let config = &mut state.views[view].config;
        config.set_hidden(&autor, true);
        config.filters.push(silex_desktop::view::Filter {
            property: paginas,
            op: silex_desktop::view::FilterOp::NotEmpty,
            value: String::new(),
        });
    }
    let dir = std::env::temp_dir().join(format!("silex-app-csv-{}", uuid::Uuid::new_v4()));
    let result = (|| -> anyhow::Result<()> {
        let ctx = harness.ctx.clone();
        let export = |harness: &mut Harness<'_, SilexApp>, all: bool| {
            harness
                .state_mut()
                .export_database_csv_to(&ctx, &dir, all)?;
            assert!(harness.state().csv_export.is_some(), "on the worker");
            // `step`, not `run`: while the worker writes, the app keeps
            // repainting (the spinner, and the worker's wake-up from its
            // thread), so it never settles.
            let start = std::time::Instant::now();
            while harness.state().csv_export.is_some()
                && start.elapsed() < std::time::Duration::from_secs(30)
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
                harness.step();
            }
            assert!(harness.state().csv_export.is_none(), "finished in 30 s");
            harness.run();
            let path = harness.state().last_export.clone().expect("exported");
            anyhow::Ok((path.clone(), std::fs::read_to_string(path)?))
        };
        let (path, view) = export(&mut harness, false)?;
        assert_eq!(path, dir.join("Lecturas.csv"));
        assert_eq!(
            view,
            "\u{feff}Nombre,Páginas,Leído,Estado,Terminado\r\n\
             Rayuela,736,true,Terminado,2026-09-14\r\n\
             Pedro Páramo,124,false,,\r\n"
        );
        let (path, all) = export(&mut harness, true)?;
        assert_eq!(path, dir.join("Lecturas (2).csv"), "never overwrites");
        let lines: Vec<&str> = all.split("\r\n").collect();
        assert_eq!(
            lines[0],
            "\u{feff}Nombre,Autor,Páginas,Leído,Estado,Terminado"
        );
        assert_eq!(lines[3], "Ficciones,Jorge Luis Borges,,false,Leyendo,");
        assert!(
            harness.query_by_label_contains("Exportado a").is_some(),
            "the notice offers the folder"
        );
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A menu item: a button whose label is `text` after its icon glyph.
fn menu_item<'h>(
    harness: &'h Harness<'_, SilexApp>,
    text: &'h str,
) -> Option<egui_kittest::Node<'h>> {
    let is_icon = |c: char| ('\u{E000}'..='\u{F8FF}').contains(&c);
    harness.query_all_by_label_contains(text).find(|node| {
        let node = node.accesskit_node();
        node.role() == egui::accesskit::Role::Button
            && node
                .label()
                .is_some_and(|l| l.trim_matches(|c: char| c.is_whitespace() || is_icon(c)) == text)
    })
}

#[test]
fn table_cells_and_property_menus_have_names() -> anyhow::Result<()> {
    let (app, _, _) = database_app()?;
    let mut harness = a11y_harness(app);
    // Each cell says its property, row and value.
    for label in [
        "Autor, Rayuela: Julio Cortázar",
        "Páginas, Pedro Páramo: 124",
        "Páginas, Ficciones: vacío",
        "Estado, Rayuela: Terminado",
        "Terminado, Rayuela: 2026-09-14",
        "Leído, Rayuela",
    ] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    // A checkbox cell is a checkbox, ticked or not.
    let leido = harness.get_by_label("Leído, Pedro Páramo");
    assert_eq!(
        leido.accesskit_node().role(),
        egui::accesskit::Role::CheckBox
    );
    leido.click();
    harness.run();
    assert!(
        harness
            .get_by_label("Leído, Pedro Páramo")
            .accesskit_node()
            .toggled()
            .is_some_and(|t| t == egui::accesskit::Toggled::True)
    );

    click_action(&mut harness, "Estado, Selección");
    assert_accessible(&harness, "property menu");
    assert!(menu_item(&harness, "Mover a la izquierda").is_some());
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    click_action(&mut harness, "Estado, Ficciones: Leyendo");
    assert_accessible(&harness, "select menu");
    assert_steady(&mut harness, "Opción de Estado, Ficciones");
    assert!(harness.query_by_label("Terminado").is_some());
    assert!(menu_item(&harness, "Quitar la opción").is_some());
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    click_button(&mut harness, "Añadir una propiedad");
    assert_accessible(&harness, "add property menu");
    assert_steady(&mut harness, "Nombre de la nueva propiedad");
    menu_item(&harness, "Fecha").expect("type «Fecha»").click();
    harness.run();
    let state = harness.state();
    let added = state.database.as_ref().and_then(|d| d.properties.last());
    assert_eq!(added.map(|p| p.name.as_str()), Some("Fecha"));
    Ok(())
}

#[test]
fn the_pages_plus_menu_creates_a_database() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    click_button(&mut harness, "Añadir");
    assert_accessible(&harness, "add menu");
    harness
        .get_all_by_label_contains("Nueva base de datos")
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("menu item")
        .click();
    harness.run();
    let state = harness.state();
    let open = state.selected.clone().expect("opened");
    assert!(state.database.as_ref().is_some_and(|d| d.id == open));
    Ok(())
}

#[test]
fn database_table() -> anyhow::Result<()> {
    database_snapshot("database_table_light", eframe::egui::Theme::Light, |_| {})
}

#[test]
fn database_table_dark() -> anyhow::Result<()> {
    database_snapshot("database_table_dark", eframe::egui::Theme::Dark, |_| {})
}

/// A select cell's menu: search field, the options as tags, «Quitar».
#[test]
fn database_select_menu() -> anyhow::Result<()> {
    database_snapshot(
        "database_select_menu_light",
        eframe::egui::Theme::Light,
        |harness| click_action(harness, "Estado, Rayuela: Terminado"),
    )
}

// ---- H11.1: sending rows to the trash in sight ----

/// The trash notice with «Deshacer», if it is shown.
fn undo_notice(harness: &Harness<'_, SilexApp>) -> Option<String> {
    text_containing(harness, "a la papelera.")
}

/// The first name or text on screen that contains `part` (a label's text
/// is its value, not its name).
fn text_containing(harness: &Harness<'_, SilexApp>, part: &str) -> Option<String> {
    harness
        .root()
        .children_recursive()
        .filter_map(|node| {
            let node = node.accesskit_node();
            node.label().or_else(|| node.value())
        })
        .find(|text| text.contains(part))
}

#[test]
fn a_rows_menu_button_and_the_delete_key_send_it_to_the_trash() -> anyhow::Result<()> {
    use eframe::egui::{Key, Modifiers};
    let (app, _, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    // «⋯» ends the row: Tab reaches it after the last cell, though it is
    // only seen on hover.
    harness.get_by_label("Nombre, fila 1").focus();
    harness.run();
    assert_visited_in_order(
        &tab_names(&mut harness, 9),
        &[
            "Abrir «Rayuela»",
            "Autor, Rayuela: Julio Cortázar",
            "Terminado, Rayuela: 2026-09-14",
            "Acciones de «Rayuela»",
            "Seleccionar «Pedro Páramo»",
            "Nombre, fila 2",
        ],
    );
    harness.key_press(eframe::egui::Key::Escape);
    harness.run();

    // «⋯» opens the row's menu, the same as a right-click on its name.
    click_button(&mut harness, "Acciones de «Pedro Páramo»");
    assert_accessible(&harness, "row menu");
    menu_item(&harness, "Enviar a la papelera")
        .expect("«Enviar a la papelera»")
        .click();
    harness.run();
    assert_eq!(shown_rows(&harness), ["Rayuela", "Ficciones"]);
    assert!(
        harness.state().db.get_page(&rows[1])?.is_none(),
        "in the trash"
    );
    assert_eq!(
        undo_notice(&harness).as_deref(),
        Some("Fila «Pedro Páramo» enviada a la papelera.")
    );
    assert_accessible(&harness, "trash notice");
    // «Deshacer» brings it back where it was, and the notice goes.
    click_button(&mut harness, "Deshacer el envío a la papelera");
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );
    assert!(undo_notice(&harness).is_none());

    // Delete with the focus on «⋯»; Cmd+Z brings it back.
    harness.get_by_label("Acciones de «Rayuela»").focus();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(shown_rows(&harness), ["Pedro Páramo", "Ficciones"]);
    assert!(undo_notice(&harness).is_some());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );
    assert!(undo_notice(&harness).is_none(), "undone: nothing to offer");

    // Backspace with the focus on a cell that is not being typed in.
    harness.get_by_label("Leído, Ficciones").focus();
    harness.run();
    harness.key_press(Key::Backspace);
    harness.run();
    assert_eq!(shown_rows(&harness), ["Rayuela", "Pedro Páramo"]);
    // Another change to the table: «Deshacer» would undo that one instead.
    harness.get_by_label("Leído, Pedro Páramo").click();
    harness.run();
    assert!(undo_notice(&harness).is_none());

    // In a name, Backspace deletes text, not the row.
    let name = harness.get_by_label("Nombre, fila 1");
    name.focus();
    harness.run();
    harness.key_press(Key::End);
    harness.key_press(Key::Backspace);
    harness.run();
    assert_eq!(shown_rows(&harness), ["Rayuel", "Pedro Páramo"]);
    Ok(())
}

#[test]
fn a_card_goes_to_the_trash_with_the_delete_key_or_its_menu_button() -> anyhow::Result<()> {
    use eframe::egui::Key;
    let (app, rows) = board_app()?;
    let mut harness = a11y_harness(app);
    assert!(harness.query_by_label("Acciones de «Rayuela»").is_some());

    harness
        .get_by_label_contains("Tarjeta «Pedro Páramo»")
        .focus();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert!(
        harness.state().db.get_page(&rows[1])?.is_none(),
        "in the trash"
    );
    assert!(
        harness
            .query_by_label_contains("Tarjeta «Pedro Páramo»")
            .is_none()
    );
    assert!(undo_notice(&harness).is_some());
    click_button(&mut harness, "Deshacer el envío a la papelera");
    assert!(
        harness
            .query_by_label_contains("Tarjeta «Pedro Páramo»")
            .is_some()
    );

    click_button(&mut harness, "Acciones de «Pedro Páramo»");
    assert_accessible(&harness, "card menu from its button");
    assert!(menu_item(&harness, "Leyendo").is_some(), "«Mover a»");
    menu_item(&harness, "Enviar a la papelera")
        .expect("«Enviar a la papelera»")
        .click();
    harness.run();
    assert!(harness.state().db.get_page(&rows[1])?.is_none());
    Ok(())
}

// ---- H11.2: several rows at once ----

/// The rows chosen in the open table, by title, in the view's order.
fn chosen_rows(harness: &Harness<'_, SilexApp>) -> Vec<String> {
    harness
        .state()
        .database
        .as_ref()
        .map(|d| {
            d.order
                .iter()
                .map(|&i| &d.rows[i])
                .filter(|r| d.selection.contains(&r.id))
                .map(|r| r.title.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The stored value of «Leído» in each row, in order.
fn leido(harness: &Harness<'_, SilexApp>, rows: &[String]) -> anyhow::Result<Vec<bool>> {
    use silex_desktop::property::CellValue;
    let state = harness.state();
    let base = &state.database.as_ref().expect("open").id;
    let property = state
        .db
        .database_properties(base)?
        .into_iter()
        .find(|p| p.name == "Leído")
        .expect("Leído")
        .id;
    let values = state.db.database_values(base)?;
    Ok(rows
        .iter()
        .map(|row| {
            values.get(row).and_then(|c| c.get(&property)) == Some(&CellValue::Checkbox(true))
        })
        .collect())
}

#[test]
fn rows_are_chosen_by_their_boxes_and_acted_on_together() -> anyhow::Result<()> {
    use eframe::egui::{Key, Modifiers};
    let (app, _, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    assert!(text_containing(&harness, "seleccionad").is_none());

    // One box, then Shift+click on another: the range between them.
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    assert_eq!(chosen_rows(&harness), ["Rayuela"]);
    assert_eq!(
        text_containing(&harness, "seleccionad").as_deref(),
        Some("1 fila seleccionada")
    );
    harness
        .get_by_label("Seleccionar «Ficciones»")
        .click_modifiers(Modifiers::SHIFT);
    harness.run();
    assert_eq!(
        chosen_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );
    assert_eq!(
        text_containing(&harness, "seleccionad").as_deref(),
        Some("3 filas seleccionadas")
    );
    assert_accessible(&harness, "selection bar");
    // A box clicked again leaves the selection.
    harness.get_by_label("Seleccionar «Pedro Páramo»").click();
    harness.run();
    assert_eq!(chosen_rows(&harness), ["Rayuela", "Ficciones"]);

    // «Poner valor…» → «Leído» → «Marcar»: both, one step of Cmd+Z.
    click_button(&mut harness, "Poner valor…");
    assert_accessible(&harness, "set value menu");
    menu_item(&harness, "Leído").expect("«Leído»").click();
    harness.run();
    menu_item(&harness, "Marcar").expect("«Marcar»").click();
    harness.run();
    assert_eq!(leido(&harness, &rows)?, [true, false, true]);
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(leido(&harness, &rows)?, [true, false, false], "as before");
    assert_eq!(
        chosen_rows(&harness),
        ["Rayuela", "Ficciones"],
        "still chosen"
    );

    // A number typed for all of them.
    click_button(&mut harness, "Poner valor…");
    menu_item(&harness, "Páginas").expect("«Páginas»").click();
    harness.run();
    harness.get_by_label("Valor de «Páginas»").focus();
    harness.run();
    harness.event(egui::Event::Text("200".into()));
    harness.run();
    click_button(&mut harness, "Aplicar");
    {
        use silex_desktop::property::CellValue;
        let state = harness.state();
        let d = state.database.as_ref().expect("open");
        let paginas = &d
            .properties
            .iter()
            .find(|p| p.name == "Páginas")
            .expect("prop")
            .id;
        for row in [&rows[0], &rows[2]] {
            assert_eq!(
                d.values.get(row).and_then(|c| c.get(paginas)),
                Some(&CellValue::Number(200.0))
            );
        }
    }
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    // To the trash together; «Deshacer» brings both back, in their places.
    click_button(&mut harness, "Enviar a la papelera");
    assert_eq!(shown_rows(&harness), ["Pedro Páramo"]);
    assert!(chosen_rows(&harness).is_empty());
    assert_eq!(
        undo_notice(&harness).as_deref(),
        Some("2 filas enviadas a la papelera.")
    );
    click_button(&mut harness, "Deshacer el envío a la papelera");
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );

    // The header's box: every row in sight, and again: none.
    harness
        .get_by_label("Seleccionar todas las filas visibles")
        .click();
    harness.run();
    assert_eq!(chosen_rows(&harness).len(), 3);
    harness
        .get_by_label("Seleccionar todas las filas visibles")
        .click();
    harness.run();
    assert!(chosen_rows(&harness).is_empty());

    // Escape empties the selection, and so does «×».
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(chosen_rows(&harness).is_empty());
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    click_button(&mut harness, "Quitar la selección");
    assert!(chosen_rows(&harness).is_empty());
    Ok(())
}

#[test]
fn the_selection_empties_when_the_view_or_its_filters_change() -> anyhow::Result<()> {
    let (app, _, _) = database_app()?;
    let mut harness = a11y_harness(app);
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    // A filter (or sort, or hidden column) changes the view.
    if let Some(state) = harness.state_mut().database.as_mut() {
        state.view_changed();
    }
    harness.run();
    assert!(chosen_rows(&harness).is_empty());

    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    add_table_view(&mut harness);
    assert!(chosen_rows(&harness).is_empty());
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    click_button(&mut harness, "Vista «Tabla»");
    assert!(chosen_rows(&harness).is_empty());
    Ok(())
}

#[test]
fn database_selection() -> anyhow::Result<()> {
    database_snapshot(
        "database_selection_light",
        eframe::egui::Theme::Light,
        select_two,
    )
}

#[test]
fn database_selection_dark() -> anyhow::Result<()> {
    database_snapshot(
        "database_selection_dark",
        eframe::egui::Theme::Dark,
        select_two,
    )
}

fn select_two(harness: &mut Harness<'_, SilexApp>) {
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    harness.get_by_label("Seleccionar «Ficciones»").click();
    harness.run();
    click_button(harness, "Poner valor…");
}

// ---- H11.4: duplicating ----

/// The sidebar row of `title`, right-clicked: its page menu.
fn sidebar_menu(harness: &mut Harness<'_, SilexApp>, title: &str) {
    harness
        .get_all_by_label(title)
        .find(|node| node.accesskit_node().role() == egui::accesskit::Role::Button)
        .unwrap_or_else(|| panic!("sidebar row {title:?}"))
        .click_secondary();
    harness.run();
}

/// The live pages right below the page `id`.
fn children_of(harness: &Harness<'_, SilexApp>, id: &str) -> Vec<String> {
    harness
        .state()
        .pages
        .iter()
        .filter(|p| p.parent_id.as_deref() == Some(id))
        .map(|p| p.title.clone())
        .collect()
}

#[test]
fn a_page_with_subpages_asks_before_it_is_duplicated() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    sidebar_menu(&mut harness, "Proyecto Silex");
    menu_item(&harness, "Duplicar").expect("«Duplicar»").click();
    harness.run();
    assert_accessible(&harness, "duplicate dialog");
    assert!(text_containing(&harness, "Tiene 2 subpáginas.").is_some());
    // «Cancelar»: nothing.
    click_button(&mut harness, "Cancelar");
    let count = harness.state().pages.len();

    sidebar_menu(&mut harness, "Proyecto Silex");
    menu_item(&harness, "Duplicar").expect("«Duplicar»").click();
    harness.run();
    click_button(&mut harness, "Solo esta página");
    let copy = harness.state().selected.clone().expect("the copy opens");
    assert_eq!(harness.state().title, "Proyecto Silex (copia)");
    assert!(children_of(&harness, &copy).is_empty());
    assert_eq!(harness.state().pages.len(), count + 1);

    sidebar_menu(&mut harness, "Proyecto Silex");
    menu_item(&harness, "Duplicar").expect("«Duplicar»").click();
    harness.run();
    click_button(&mut harness, "Con las subpáginas");
    let copy = harness.state().selected.clone().expect("the copy opens");
    assert_eq!(
        children_of(&harness, &copy),
        ["Diseño de la interfaz", "Guía de estilo"]
    );
    // A page without subpages is copied at once, also from the palette.
    let recetas = harness
        .state()
        .pages
        .iter()
        .find(|p| p.title == "Recetas")
        .map(|p| p.id.clone())
        .expect("Recetas");
    harness.state_mut().open_page(recetas)?;
    harness.run();
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .run_command(&ctx, silex_desktop::palette::Command::DuplicatePage)?;
    harness.run();
    assert!(harness.state().duplicate_dialog.is_none(), "nothing to ask");
    assert_eq!(harness.state().title, "Recetas (copia)");
    Ok(())
}

#[test]
fn rows_are_duplicated_below_themselves_and_cmd_z_takes_them_back() -> anyhow::Result<()> {
    use eframe::egui::{Key, Modifiers};
    use silex_desktop::property::CellValue;
    let (app, _, _) = database_app()?;
    let mut harness = a11y_harness(app);
    click_button(&mut harness, "Acciones de «Rayuela»");
    menu_item(&harness, "Duplicar").expect("«Duplicar»").click();
    harness.run();
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Rayuela (copia)", "Pedro Páramo", "Ficciones"]
    );
    {
        let state = harness.state();
        let d = state.database.as_ref().expect("open");
        let copy = &d.rows[d.order[1]].id;
        let autor = &d
            .properties
            .iter()
            .find(|p| p.name == "Autor")
            .expect("Autor")
            .id;
        assert_eq!(
            d.values.get(copy).and_then(|c| c.get(autor)),
            Some(&CellValue::Text("Julio Cortázar".into()))
        );
    }
    assert!(harness.state().database.is_some(), "the table stays open");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );

    // The chosen rows, together: one step of Cmd+Z.
    harness.get_by_label("Seleccionar «Rayuela»").click();
    harness.run();
    harness.get_by_label("Seleccionar «Ficciones»").click();
    harness.run();
    click_button(&mut harness, "Duplicar");
    assert_eq!(
        shown_rows(&harness),
        [
            "Rayuela",
            "Rayuela (copia)",
            "Pedro Páramo",
            "Ficciones",
            "Ficciones (copia)"
        ]
    );
    assert!(chosen_rows(&harness).is_empty());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );
    Ok(())
}

#[test]
fn duplicate_dialog() -> anyhow::Result<()> {
    snapshot_with(
        "duplicate_dialog_light",
        eframe::egui::Theme::Light,
        |harness| {
            sidebar_menu(harness, "Proyecto Silex");
            if let Some(item) = menu_item(harness, "Duplicar") {
                item.click();
            }
            harness.run();
            harness.event(egui::Event::PointerMoved(egui::pos2(1000.0, 700.0)));
        },
    )
}

// ---- H11.3, ADR 0013: deleting for good from the trash ----

/// The demo app with «Recetas» and «Proyecto Silex» (with its two
/// subpages and «Guía de estilo») in the trash, and the trash open.
fn trash_app() -> anyhow::Result<SilexApp> {
    let mut app = demo_app()?;
    let id = |app: &SilexApp, title: &str| {
        app.pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .expect("page")
    };
    let recetas = id(&app, "Recetas");
    let proyecto = id(&app, "Proyecto Silex");
    app.db.trash_page(&recetas)?;
    app.db.trash_page(&proyecto)?;
    app.reload_pages()?;
    app.open_page(id(&app, "Notas de lectura"))?;
    app.open_trash_dialog();
    Ok(app)
}

#[test]
fn trash_items_are_deleted_for_good_only_after_confirming() -> anyhow::Result<()> {
    let mut harness = a11y_harness(trash_app()?);
    assert_accessible(&harness, "trash");
    click_button(&mut harness, "Borrar «Recetas» para siempre");
    assert_accessible(&harness, "confirm one");
    assert_eq!(
        text_containing(&harness, "se borrará").as_deref(),
        Some("«Recetas» se borrará para siempre.")
    );
    // «Cancelar» goes back to the list: nothing is deleted.
    click_button(&mut harness, "Cancelar");
    assert_eq!(harness.state().db.list_trash()?.len(), 2);
    assert!(
        harness
            .query_by_label("Borrar «Recetas» para siempre")
            .is_some()
    );

    click_button(&mut harness, "Borrar «Recetas» para siempre");
    click_button(&mut harness, "Confirmar: borrar para siempre");
    let trash = harness.state().db.list_trash()?;
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].page.title, "Proyecto Silex");
    assert!(
        harness
            .query_by_label("Borrar «Recetas» para siempre")
            .is_none()
    );

    // «Vaciar la papelera» counts the pages that go with each entry.
    click_button(&mut harness, "Vaciar la papelera");
    assert_eq!(
        text_containing(&harness, "Se borrar").as_deref(),
        Some("Se borrarán para siempre 4 páginas (con sus subpáginas y filas).")
    );
    click_button(&mut harness, "Confirmar: borrar para siempre");
    assert!(harness.state().db.list_trash()?.is_empty());
    assert_eq!(harness.state().trash_count, 0);
    assert!(text_containing(&harness, "La papelera está vacía.").is_some());
    // An empty trash has nothing to empty.
    let empty = harness
        .get_all_by_label("Vaciar la papelera")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("button");
    assert!(empty.accesskit_node().is_disabled());
    Ok(())
}

#[test]
fn trash_confirm() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), trash_app()?);
    harness.run();
    click_button(&mut harness, "Vaciar la papelera");
    // The pointer out of the way: where the button was is not steady.
    harness.event(egui::Event::PointerMoved(egui::pos2(1000.0, 700.0)));
    harness.run();
    harness.snapshot("trash_confirm_light");
    Ok(())
}

/// A row under the pointer shows its «⋯»; the trash notice offers «Deshacer».
#[test]
fn database_row_trashed() -> anyhow::Result<()> {
    database_snapshot(
        "database_row_trashed_light",
        eframe::egui::Theme::Light,
        |harness| {
            click_button(harness, "Acciones de «Pedro Páramo»");
            if let Some(item) = menu_item(harness, "Enviar a la papelera") {
                item.click();
            }
            harness.run();
            harness.get_by_label("Acciones de «Rayuela»").hover();
        },
    )
}

#[test]
fn database_row_trashed_dark() -> anyhow::Result<()> {
    database_snapshot(
        "database_row_trashed_dark",
        eframe::egui::Theme::Dark,
        |harness| {
            click_button(harness, "Acciones de «Pedro Páramo»");
            if let Some(item) = menu_item(harness, "Enviar a la papelera") {
                item.click();
            }
            harness.run();
            harness.get_by_label("Acciones de «Rayuela»").hover();
        },
    )
}

/// Like [`snapshot_with`], with the database «Lecturas» open.
fn database_snapshot(
    name: &str,
    theme: eframe::egui::Theme,
    interact: impl FnOnce(&mut Harness<'_, SilexApp>),
) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (app, _, _) = database_app()?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    interact(&mut harness);
    harness.run();
    harness.snapshot(name);
    Ok(())
}

// ---- H6.2b: options in the property menu, a row's page ----

/// A menu keeps its size and place from one frame to the next (a widget
/// sized from the menu's own width makes it grow every frame).
#[track_caller]
fn assert_steady(harness: &mut Harness<'_, SilexApp>, label: &str) {
    let bounds =
        |harness: &Harness<'_, SilexApp>| harness.get_by_label(label).accesskit_node().raw_bounds();
    let before = bounds(harness);
    for _ in 0..5 {
        harness.step();
    }
    assert_eq!(bounds(harness), before, "the menu with {label:?} moved");
}

/// The options of «Estado» as stored: (name, color).
fn estado_options(
    app: &SilexApp,
    base: &str,
) -> Vec<(String, silex_desktop::property::OptionColor)> {
    app.db
        .database_properties(base)
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.name == "Estado")
        .map(|p| p.options.into_iter().map(|o| (o.name, o.color)).collect())
        .unwrap_or_default()
}

#[test]
fn options_are_managed_from_the_property_menu() -> anyhow::Result<()> {
    use silex_desktop::property::OptionColor;
    let (app, base, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    click_action(&mut harness, "Estado, Selección");
    assert_accessible(&harness, "options in the property menu");
    assert_steady(&mut harness, "Color de «Leyendo»: Azul");

    // Color: the swatch shows the eight colors.
    click_button(&mut harness, "Color de «Leyendo»: Azul");
    assert_accessible(&harness, "colors");
    click_button(&mut harness, "Rojo");
    assert_eq!(
        estado_options(harness.state(), &base),
        [
            ("Terminado".to_owned(), OptionColor::Green),
            ("Leyendo".to_owned(), OptionColor::Red)
        ]
    );

    // Name: saved when the field is left with Enter.
    let field = harness.get_by_label("Nombre de la opción 2");
    field.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Nombre de la opción 2")
        .type_text("Releyendo");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(estado_options(harness.state(), &base)[1].0, "Releyendo");

    // Delete: asks first and says how many cells it empties.
    click_button(&mut harness, "Borrar la opción «Terminado»");
    assert_accessible(&harness, "confirm delete");
    assert!(
        harness
            .query_by_label_contains("¿Borrar «Terminado»? Se vaciará en 1 fila.")
            .is_some()
    );
    click_button(&mut harness, "Cancelar");
    assert_eq!(estado_options(harness.state(), &base).len(), 2);
    click_button(&mut harness, "Borrar la opción «Terminado»");
    click_button(&mut harness, "Borrar");
    assert_eq!(estado_options(harness.state(), &base).len(), 1);
    let values = harness.state().db.row_values(&rows[0])?;
    assert!(
        values
            .values()
            .all(|v| !matches!(v, silex_desktop::property::CellValue::Select(_))),
        "Rayuela's state is empty: {values:?}"
    );
    Ok(())
}

#[test]
fn a_rows_page_names_its_properties() -> anyhow::Result<()> {
    let (mut app, _, rows) = database_app()?;
    app.open_page(rows[0].clone())?;
    let harness = a11y_harness(app);
    assert_accessible(&harness, "row page");
    for label in [
        "Autor, Rayuela: Julio Cortázar",
        "Páginas, Rayuela: 736",
        "Estado, Rayuela: Terminado",
        "Leído, Rayuela",
    ] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    Ok(())
}

#[test]
fn database_row_page() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (mut app, _, rows) = database_app()?;
    app.open_page(rows[1].clone())?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness.snapshot("database_row_page_light");
    Ok(())
}

#[test]
fn database_property_menu() -> anyhow::Result<()> {
    database_snapshot(
        "database_property_menu_light",
        eframe::egui::Theme::Light,
        |harness| {
            click_action(harness, "Estado, Selección");
            click_button(harness, "Color de «Leyendo»: Azul");
        },
    )
}

// ---- H6.3a: the view's menus ----

/// The titles of the rows in sight, in order.
fn shown_rows(harness: &Harness<'_, SilexApp>) -> Vec<String> {
    harness
        .state()
        .database
        .as_ref()
        .map(|d| d.order.iter().map(|&i| d.rows[i].title.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn the_view_menus_filter_sort_and_hide() -> anyhow::Result<()> {
    let (app, base, _) = database_app()?;
    let mut harness = a11y_harness(app);

    // Filter: Páginas > 150 (Rayuela 736, Pedro Páramo 124, Ficciones none).
    click_button(&mut harness, "Filtrar");
    assert_accessible(&harness, "filter menu");
    click_button(&mut harness, "+ Añadir un filtro");
    menu_item(&harness, "Páginas").expect("Páginas").click();
    harness.run();
    assert_eq!(
        harness
            .state()
            .database
            .as_ref()
            .map(|d| d.views[0].config.filters.len()),
        Some(1),
        "the menu stays open with the new filter"
    );
    click_button(&mut harness, "igual a");
    menu_item(&harness, "mayor que").expect("mayor que").click();
    harness.run();
    harness.get_by_label("Valor del filtro 1").focus();
    harness.run();
    harness.get_by_label("Valor del filtro 1").type_text("150");
    harness.run();
    assert_accessible(&harness, "a filter");
    assert_steady(&mut harness, "Valor del filtro 1");
    assert_eq!(shown_rows(&harness), ["Rayuela"]);
    assert!(harness.query_by_label("Filtrar (1)").is_some());
    assert!(harness.query_by_label_contains("1 de 3 filas").is_some());
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    let stored = harness.state().db.database_views(&base)?[0].config.clone();
    assert_eq!(stored.filters[0].value, "150", "saved once typed");

    // Sort by name, descending; the filter goes.
    click_button(&mut harness, "Filtrar (1)");
    click_button(&mut harness, "Quitar el filtro 1");
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    click_button(&mut harness, "Ordenar");
    assert_accessible(&harness, "sort menu");
    click_button(&mut harness, "+ Añadir un orden");
    menu_item(&harness, "Nombre").expect("Nombre").click();
    harness.run();
    click_button(&mut harness, "Ascendente");
    menu_item(&harness, "Descendente")
        .expect("Descendente")
        .click();
    harness.run();
    assert_steady(&mut harness, "Quitar el orden por Nombre");
    assert_eq!(
        shown_rows(&harness),
        ["Rayuela", "Pedro Páramo", "Ficciones"]
    );
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    // Columns: «Autor» unticked leaves the header.
    click_button(&mut harness, "Columnas");
    assert_accessible(&harness, "columns menu");
    harness
        .get_all_by_label_contains("Autor")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox)
        .expect("Autor check box")
        .click();
    harness.run();
    assert!(harness.query_by_label("Autor, Texto").is_none(), "hidden");
    assert!(harness.query_by_label("Columnas (1 oculta)").is_some());
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    let stored = harness.state().db.database_views(&base)?[0].config.clone();
    assert_eq!(stored.sorts.len(), 1);
    assert_eq!(stored.hidden.len(), 1);
    Ok(())
}

#[test]
fn database_filter_menu() -> anyhow::Result<()> {
    database_snapshot(
        "database_filter_menu_light",
        eframe::egui::Theme::Light,
        |harness| {
            click_button(harness, "Filtrar");
            click_button(harness, "+ Añadir un filtro");
            if let Some(item) = menu_item(harness, "Estado") {
                item.click();
            }
            harness.run();
        },
    )
}

// ---- H6.3b: view tabs ----

/// «+» after the tabs, then «Tabla» in its menu (H6.5 added «Tablero»).
fn add_table_view(harness: &mut Harness<'_, SilexApp>) {
    click_button(harness, "Nueva vista");
    menu_item(harness, "Tabla").expect("«Tabla»").click();
    harness.run();
}

#[test]
fn view_tabs_add_rename_and_delete_views() -> anyhow::Result<()> {
    let (app, base, _) = database_app()?;
    let mut harness = a11y_harness(app);
    add_table_view(&mut harness);
    assert!(harness.query_by_label("Vista «Tabla 2»").is_some());
    let open = |h: &Harness<'_, SilexApp>| {
        h.state()
            .database
            .as_ref()
            .and_then(|d| d.views.get(d.view).map(|v| v.name.clone()))
    };
    assert_eq!(open(&harness).as_deref(), Some("Tabla 2"));

    // A click on the open tab opens its menu, with its name ready.
    click_button(&mut harness, "Vista «Tabla 2»");
    assert_accessible(&harness, "view menu");
    assert_steady(&mut harness, "Nombre de la vista");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness
        .get_by_label("Nombre de la vista")
        .type_text("Pendientes");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    let names: Vec<String> = harness
        .state()
        .db
        .database_views(&base)?
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(names, ["Tabla", "Pendientes"]);

    // Another tab switches; the menu of the open one deletes it, asking first.
    click_button(&mut harness, "Vista «Tabla»");
    assert_eq!(open(&harness).as_deref(), Some("Tabla"));
    click_button(&mut harness, "Vista «Tabla»");
    menu_item(&harness, "Borrar la vista")
        .expect("delete")
        .click();
    harness.run();
    assert_accessible(&harness, "confirm view delete");
    assert!(harness.query_by_label_contains("las filas no").is_some());
    click_button(&mut harness, "Borrar");
    assert_eq!(harness.state().db.database_views(&base)?.len(), 1);
    assert_eq!(open(&harness).as_deref(), Some("Pendientes"));

    // The last view cannot be deleted.
    click_button(&mut harness, "Vista «Pendientes»");
    let delete = menu_item(&harness, "Borrar la vista").expect("delete");
    assert!(delete.accesskit_node().is_disabled());
    Ok(())
}

#[test]
fn database_view_menu() -> anyhow::Result<()> {
    database_snapshot(
        "database_view_menu_light",
        eframe::egui::Theme::Light,
        |harness| {
            add_table_view(harness);
            click_button(harness, "Vista «Tabla 2»");
        },
    )
}

// ---- H6.5: board view ----

/// «Lecturas» open in a new board, grouped by «Estado».
fn board_app() -> anyhow::Result<(SilexApp, Vec<String>)> {
    let (mut app, _, rows) = database_app()?;
    app.add_view(silex_desktop::model::ViewKind::Board)?;
    Ok((app, rows))
}

#[test]
fn board_cards_and_menus_have_names() -> anyhow::Result<()> {
    use silex_desktop::property::CellValue;
    let (app, rows) = board_app()?;
    let mut harness = a11y_harness(app);
    assert_accessible(&harness, "board");
    for label in [
        "Tarjeta «Rayuela», en Terminado; Autor: Julio Cortázar; Páginas: 736; Leído: Sí; \
         Terminado: 2026-09-14",
        "Tarjeta «Pedro Páramo», en Sin Estado; Autor: Juan Rulfo; Páginas: 124",
        "Nueva tarjeta en «Leyendo»",
    ] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }

    // Without a mouse drag: the card's menu moves it.
    harness
        .get_by_label_contains("Tarjeta «Pedro Páramo»")
        .click_secondary();
    harness.run();
    assert_accessible(&harness, "card menu");
    assert!(menu_item(&harness, "Sin Estado").is_none(), "it is there");
    menu_item(&harness, "Leyendo").expect("«Leyendo»").click();
    harness.run();
    let state = harness.state();
    let estado = state
        .database
        .as_ref()
        .and_then(|d| d.properties.iter().find(|p| p.name == "Estado"))
        .expect("Estado");
    let leyendo = estado.options.iter().find(|o| o.name == "Leyendo");
    assert_eq!(
        state
            .db
            .database_values(&state.database.as_ref().expect("open").id)?
            .get(&rows[1])
            .and_then(|cells| cells.get(&estado.id))
            .cloned(),
        leyendo.map(|o| CellValue::Select(o.id.clone()))
    );

    click_button(&mut harness, "Agrupar: Estado");
    assert_accessible(&harness, "group menu");
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    click_button(&mut harness, "Nueva vista");
    assert_accessible(&harness, "new view menu");
    assert!(menu_item(&harness, "Tablero").is_some());
    Ok(())
}

#[test]
fn database_board() -> anyhow::Result<()> {
    board_snapshot("database_board_light", eframe::egui::Theme::Light)
}

#[test]
fn database_board_dark() -> anyhow::Result<()> {
    board_snapshot("database_board_dark", eframe::egui::Theme::Dark)
}

fn board_snapshot(name: &str, theme: eframe::egui::Theme) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (app, _) = board_app()?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness.snapshot(name);
    Ok(())
}

/// The options of «Estado» on the open board, in order.
fn board_columns(harness: &Harness<'_, SilexApp>) -> Vec<String> {
    harness
        .state()
        .database
        .as_ref()
        .and_then(|d| d.properties.iter().find(|p| p.name == "Estado"))
        .map(|p| p.options.iter().map(|o| o.name.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn a_board_column_menu_renames_moves_and_deletes_its_option() -> anyhow::Result<()> {
    let (app, rows) = board_app()?;
    let mut harness = a11y_harness(app);
    assert_eq!(board_columns(&harness), ["Terminado", "Leyendo"]);

    click_button(&mut harness, "Grupo «Leyendo»");
    assert_accessible(&harness, "column menu");
    assert_steady(&mut harness, "Nombre del grupo");
    assert!(
        menu_item(&harness, "Mover a la derecha").is_some_and(|b| b.accesskit_node().is_disabled()),
        "already last"
    );
    menu_item(&harness, "Mover a la izquierda")
        .expect("move")
        .click();
    harness.run();
    assert_eq!(board_columns(&harness), ["Leyendo", "Terminado"]);

    // Renamed in its field; saved when the menu closes.
    click_button(&mut harness, "Grupo «Leyendo»");
    harness.get_by_label("Nombre del grupo").type_text(" ya");
    harness.run();
    egui::Popup::close_all(&harness.ctx);
    harness.run();
    harness.run();
    assert_eq!(board_columns(&harness), ["Leyendo ya", "Terminado"]);

    // Deleting asks first and says where its cards go.
    click_button(&mut harness, "Grupo «Leyendo ya»");
    menu_item(&harness, "Borrar el grupo")
        .expect("delete")
        .click();
    harness.run();
    assert!(
        harness
            .query_by_label_contains("1 tarjeta pasará a «Sin Estado»")
            .is_some()
    );
    click_button(&mut harness, "Borrar");
    assert_eq!(board_columns(&harness), ["Terminado"]);
    let state = harness.state();
    let base = state.database.as_ref().expect("open").id.clone();
    let ficciones = state.db.database_values(&base)?.get(&rows[2]).cloned();
    assert!(
        ficciones.is_none_or(|cells| cells
            .values()
            .all(|v| !matches!(v, silex_desktop::property::CellValue::Select(_)))),
        "its card has no option now"
    );
    Ok(())
}

#[test]
fn database_board_column_menu() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (app, _) = board_app()?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    click_button(&mut harness, "Grupo «Leyendo»");
    menu_item(&harness, "Borrar el grupo")
        .expect("delete")
        .click();
    harness.run();
    harness.snapshot("database_board_column_menu_light");
    Ok(())
}

// ---- H6.6: date picker ----

/// Opens a cell from the keyboard: focus (which scrolls it into sight;
/// «Terminado» is past the window's right edge) and Intro.
fn open_cell(harness: &mut Harness<'_, SilexApp>, label: &str) {
    harness.get_by_label(label).focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
}

/// The «Terminado» date of a row of «Lecturas», as stored.
fn terminado(harness: &Harness<'_, SilexApp>, row: &str) -> Option<String> {
    let state = harness.state();
    let database = state.database.as_ref()?;
    let property = database.properties.iter().find(|p| p.name == "Terminado")?;
    let values = state.db.database_values(&database.id).ok()?;
    match values.get(row)?.get(&property.id)? {
        silex_desktop::property::CellValue::Date(date) => Some(date.to_string()),
        _ => None,
    }
}

#[test]
fn the_calendar_picks_moves_and_clears_dates() -> anyhow::Result<()> {
    use silex_desktop::property::{CalendarMonth, Date, spoken_date};
    let (app, _, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    let today = Date::today_utc().expect("a clock after 1970");

    // An empty cell opens on this month; «Mes anterior» goes back one.
    open_cell(&mut harness, "Terminado, Pedro Páramo: vacío");
    assert_accessible(&harness, "calendar");
    assert_steady(&mut harness, "Fecha de Terminado, Pedro Páramo");
    let month = CalendarMonth::of(today);
    assert!(harness.query_by_label(&month.label()).is_some());
    assert!(
        harness
            .query_by_label(&format!("{}, hoy", spoken_date(today)))
            .is_some()
    );
    click_button(&mut harness, "Mes anterior");
    let day = month.previous().weeks()[2]
        .iter()
        .flatten()
        .next()
        .copied()
        .expect("a day in the third week");
    click_button(&mut harness, &spoken_date(day));
    assert_eq!(terminado(&harness, &rows[1]), Some(day.to_string()));

    // A cell with a date opens on its month, with its day selected.
    open_cell(&mut harness, "Terminado, Rayuela: 2026-09-14");
    assert!(harness.query_by_label("septiembre de 2026").is_some());
    let toggled = |label: &str| {
        harness
            .get_by_label(label)
            .accesskit_node()
            .toggled()
            .is_some_and(|t| t == egui::accesskit::Toggled::True)
    };
    assert!(toggled("14 de septiembre de 2026"), "its day is marked");
    assert!(!toggled("15 de septiembre de 2026"));
    menu_item(&harness, "Quitar la fecha")
        .expect("«Quitar la fecha»")
        .click();
    harness.run();
    assert_eq!(terminado(&harness, &rows[0]), None);

    // «Hoy».
    open_cell(&mut harness, "Terminado, Ficciones: vacío");
    menu_item(&harness, "Hoy").expect("«Hoy»").click();
    harness.run();
    assert_eq!(terminado(&harness, &rows[2]), Some(today.to_string()));
    Ok(())
}

#[test]
fn database_date_picker() -> anyhow::Result<()> {
    database_snapshot(
        "database_date_picker_light",
        eframe::egui::Theme::Light,
        |harness| open_cell(harness, "Terminado, Rayuela: 2026-09-14"),
    )
}

// ---- H7.1: attachments ----

/// A page with a paragraph, a PDF, an image and a script attached.
fn attachments_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::attachment::NewAttachment;
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Viaje a Lisboa", None)?;
    let mut blocks = vec![Block {
        id: "intro".into(),
        page_id: page.clone(),
        kind: BlockKind::Paragraph,
        content: "Billetes y reservas del viaje.".into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks: Vec::new(),
    }];
    // A real image (a soft gradient), so its preview shows (H7.2).
    let (w, h) = (480u32, 260u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(60 + x * 120 / w) as u8, (140 + y * 80 / h) as u8, 210, 255]
        })
        .collect();
    let png = silex_desktop::attachment::encode_png(w, h, rgba)?;
    let files = [
        ("billetes de avión.pdf", {
            let mut pdf = b"%PDF-1.7".to_vec();
            pdf.resize(2_516_582, b' ');
            pdf
        }),
        ("mapa del centro.png", png),
        ("instalar.sh", b"#!/bin/sh\necho hola".to_vec()),
    ];
    for (name, data) in files {
        let attachment = db.add_attachment(&NewAttachment::from_bytes(name, data)?)?;
        blocks.push(Block {
            id: format!("file-{name}"),
            page_id: page.clone(),
            kind: BlockKind::File,
            content: attachment.name.clone(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: Some(attachment.id),
            link: None,
            marks: Vec::new(),
        });
    }
    db.save_document(&page, "Viaje a Lisboa", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

#[test]
fn file_blocks_have_names() -> anyhow::Result<()> {
    let mut harness = a11y_harness(attachments_app()?);
    assert_accessible(&harness, "file blocks");
    for label in [
        "Archivo «billetes de avión.pdf», 2,4 MB",
        "Abrir «billetes de avión.pdf»",
        "Guardar una copia de «mapa del centro.png»",
        "Imagen «mapa del centro.png», 480 × 260, 3,4 KB",
        "Mostrar «instalar.sh» en la carpeta",
    ] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    assert!(harness.query_by_label("Abrir «instalar.sh»").is_none());
    // The block menu of a file: no «Convertir en».
    harness
        .get_by_label("Archivo «billetes de avión.pdf», 2,4 MB")
        .hover();
    harness.run();
    click_button(&mut harness, "Acciones del bloque");
    assert_accessible(&harness, "file block menu");
    assert!(menu_item(&harness, "Convertir en").is_none());
    assert!(menu_item(&harness, "Guardar una copia").is_some());
    Ok(())
}

#[test]
fn file_blocks() -> anyhow::Result<()> {
    file_blocks_snapshot("file_blocks_light", eframe::egui::Theme::Light)
}

#[test]
fn file_blocks_dark() -> anyhow::Result<()> {
    file_blocks_snapshot("file_blocks_dark", eframe::egui::Theme::Dark)
}

fn file_blocks_snapshot(name: &str, theme: eframe::egui::Theme) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            attachments_app()?,
        );
    harness.run();
    harness.snapshot(name);
    Ok(())
}

// ---- H7.4: space ----

/// The PDF's block is deleted and saved, but the version saved before it
/// still names its file (ADR 0009). A copy of the PDF that nothing names
/// (as from an attach that was undone) is the unused one.
fn with_unused_pdf(app: &mut SilexApp) -> anyhow::Result<()> {
    use silex_desktop::attachment::NewAttachment;
    app.blocks.retain(|b| !b.content.starts_with("billetes"));
    app.mark_dirty();
    app.save_now()?;
    let mut pdf = b"%PDF-1.7".to_vec();
    pdf.resize(2_516_582, b' ');
    app.db
        .add_attachment(&NewAttachment::from_bytes("billetes (copia).pdf", pdf)?)?;
    Ok(())
}

#[test]
fn freeing_space_shows_what_goes_and_asks_first() -> anyhow::Result<()> {
    let mut app = attachments_app()?;
    with_unused_pdf(&mut app)?;
    app.history.clear();
    let mut harness = a11y_harness(app);
    click_button(&mut harness, "Copias y exportación");
    assert_accessible(&harness, "backups menu");
    assert!(
        harness
            .query_by_label("1 adjunto sin usar, 2,4 MB")
            .is_some()
    );
    menu_item(&harness, "Liberar espacio…")
        .expect("«Liberar espacio…»")
        .click();
    harness.run();
    assert_accessible(&harness, "free space dialog");
    assert!(
        harness
            .query_by_label_contains("Se borrará 1 adjunto que ya no usa ninguna página (2,4 MB)")
            .is_some()
    );
    click_button(&mut harness, "Liberar espacio");
    let state = harness.state();
    let notice = state.notice.clone().unwrap_or_default();
    assert!(notice.starts_with("Se liberaron"), "{notice}");
    assert!(notice.contains("(1 adjunto sin usar)"), "{notice}");
    assert_eq!(state.db.space_usage()?.unused_attachments, 0);
    // The other files are still there, and so is the PDF a version names.
    assert_eq!(state.db.space_usage()?.used_attachments, 3);
    Ok(())
}

/// The «Copias y exportación» menu with its «Espacio» part.
#[test]
fn backups_menu() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut app = attachments_app()?;
    with_unused_pdf(&mut app)?;
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    click_button(&mut harness, "Copias y exportación");
    harness.snapshot("backups_menu_light");
    Ok(())
}

// ---- H9.2: dragging blocks ----

/// A page with four paragraphs: «uno», «dos», «tres» and «cuatro».
fn drag_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Arrastrar", None)?;
    let blocks: Vec<Block> = ["uno", "dos", "tres", "cuatro"]
        .iter()
        .map(|text| Block {
            id: (*text).into(),
            page_id: page.clone(),
            kind: BlockKind::Paragraph,
            content: (*text).into(),
            checked: false,
            position: 0,
            indent: 0,
            attachment: None,
            link: None,
            marks: Vec::new(),
        })
        .collect();
    db.save_document(&page, "Arrastrar", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

/// The rect of the text field of the block that says `text`.
fn block_rect(harness: &Harness<'_, SilexApp>, text: &str) -> egui::Rect {
    harness
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().value().as_deref() == Some(text))
        .map(|n| n.rect())
        .unwrap_or_else(|| panic!("block {text:?}"))
}

/// Presses on the handle of the block that says `text` and moves the
/// pointer to `to` in a few steps, without releasing.
fn start_drag(harness: &mut Harness<'_, SilexApp>, text: &str, to: egui::Pos2) {
    let block = block_rect(harness, text);
    harness.hover_at(block.center());
    harness.run();
    let handle = harness.get_by_label("Acciones del bloque").rect().center();
    harness.hover_at(handle);
    harness.run();
    harness.drag_at(handle);
    harness.run();
    for step in 1..=6 {
        let t = step as f32 / 6.0;
        harness.hover_at(handle + (to - handle) * t);
        harness.run();
    }
}

fn texts(app: &SilexApp) -> Vec<String> {
    app.blocks.iter().map(|b| b.content.clone()).collect()
}

#[test]
fn a_block_dragged_by_its_handle_moves_and_cmd_z_takes_it_back() -> anyhow::Result<()> {
    let mut harness = a11y_harness(drag_app()?);
    // Below «cuatro»: to the end.
    let below = block_rect(&harness, "cuatro").center_bottom() + egui::vec2(0.0, 6.0);
    start_drag(&mut harness, "uno", below);
    harness.drop_at(below);
    harness.run();
    assert_eq!(texts(harness.state()), ["dos", "tres", "cuatro", "uno"]);
    // Saved like any edit, and one Cmd+Z puts it back.
    harness.state_mut().save_now()?;
    let page = harness.state().selected.clone().unwrap_or_default();
    let stored: Vec<String> = harness
        .state()
        .db
        .list_blocks(&page)?
        .into_iter()
        .map(|b| b.content)
        .collect();
    assert_eq!(stored, ["dos", "tres", "cuatro", "uno"]);
    harness.state_mut().undo();
    assert_eq!(texts(harness.state()), ["uno", "dos", "tres", "cuatro"]);
    Ok(())
}

#[test]
fn a_selection_moves_together_and_escape_cancels_a_drag() -> anyhow::Result<()> {
    let mut app = drag_app()?;
    app.selection = Some(super::BlockSelection {
        anchor: "uno".into(),
        head: "dos".into(),
    });
    let mut harness = a11y_harness(app);
    let below = block_rect(&harness, "cuatro").center_bottom() + egui::vec2(0.0, 6.0);
    start_drag(&mut harness, "uno", below);
    harness.drop_at(below);
    harness.run();
    assert_eq!(texts(harness.state()), ["tres", "cuatro", "uno", "dos"]);

    // Esc while dragging: nothing moves.
    let above = block_rect(&harness, "tres").center_top() - egui::vec2(0.0, 2.0);
    start_drag(&mut harness, "dos", above);
    harness.key_press(egui::Key::Escape);
    harness.run();
    harness.drop_at(above);
    harness.run();
    assert_eq!(texts(harness.state()), ["tres", "cuatro", "uno", "dos"]);
    // Dropped where it already is: nothing either.
    let here = block_rect(&harness, "uno").center();
    start_drag(&mut harness, "uno", here);
    harness.drop_at(here);
    harness.run();
    assert_eq!(texts(harness.state()), ["tres", "cuatro", "uno", "dos"]);
    Ok(())
}

#[test]
fn block_drag() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), drag_app()?);
    harness.run();
    // «uno» held between «tres» and «cuatro».
    let between = block_rect(&harness, "cuatro").center_top();
    start_drag(&mut harness, "uno", between);
    harness.snapshot("block_drag_light");
    Ok(())
}

// ---- H7.12: local data kept private ----

#[test]
fn the_synced_folder_warning_has_names_and_is_not_shown_again() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("silex-synced-{}", uuid::Uuid::new_v4()));
    let folder = root.join("OneDrive - Empresa");
    std::fs::create_dir_all(&folder)?;
    let path = folder.join("silex.sqlite3");
    let mut app = SilexApp::new(Database::open(&path)?)?;
    app.startup_backup_pending = false;
    let mut harness = a11y_harness(app);
    harness.run();
    assert_accessible(&harness, "synced folder warning");
    click_button(&mut harness, "No volver a avisar");
    harness.run();
    assert!(harness.state().synced_warning.is_none());
    assert!(harness.state().db.synced_warning_dismissed()?);
    drop(harness);
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

// ---- H8.2: the page beside ----

/// «Notas de lectura» open, «Guía de estilo» beside it.
fn beside_demo() -> anyhow::Result<SilexApp> {
    let mut app = demo_app()?;
    let find = |app: &SilexApp, title: &str| {
        app.pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .unwrap_or_default()
    };
    let notes = find(&app, "Notas de lectura");
    let guide = find(&app, "Guía de estilo");
    app.open_page(notes)?;
    // Wide enough for both (the first frame measures it).
    app.main_width = 1000.0;
    app.open_beside(&guide)?;
    Ok(app)
}

#[test]
fn the_page_beside_has_names_keeps_its_width_and_edits_here() -> anyhow::Result<()> {
    let mut harness = Harness::builder()
        .with_size([1400.0, 800.0])
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), beside_demo()?);
    harness.run();
    assert_accessible(&harness, "page beside");
    assert!(harness.query_by_label("Cerrar el panel lateral").is_some());
    let edit = |h: &Harness<'_, SilexApp>| h.get_by_label("Editar aquí").rect();
    let before = edit(&harness);
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(edit(&harness), before, "the panel keeps its width");
    click_button(&mut harness, "Editar aquí");
    harness.run();
    let state = harness.state();
    assert!(state.side.is_none());
    let open = state.selected.clone().unwrap_or_default();
    assert_eq!(
        state.find_page(&open).map(|p| p.title.as_str()),
        Some("Guía de estilo")
    );
    Ok(())
}

#[test]
fn side_panel() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1400.0, 800.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), beside_demo()?);
    harness.run();
    harness.snapshot("side_panel_light");
    Ok(())
}

// ---- H8.1: tabs ----

/// The demo workspace with three tabs: «Guía de estilo» (active),
/// «Recetas» and «Notas de lectura».
fn tabs_demo() -> anyhow::Result<SilexApp> {
    let mut app = demo_app()?;
    let find = |app: &SilexApp, title: &str| {
        app.pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .unwrap_or_default()
    };
    let guide = find(&app, "Guía de estilo");
    let recipes = find(&app, "Recetas");
    let notes = find(&app, "Notas de lectura");
    app.open_page(guide)?;
    app.open_in_new_tab(&recipes)?;
    app.open_in_new_tab(&notes)?;
    app.select_tab(0)?;
    Ok(app)
}

#[test]
fn tabs_are_named_and_a_click_switches() -> anyhow::Result<()> {
    let mut harness = a11y_harness(tabs_demo()?);
    assert_accessible(&harness, "tabs");
    let tabs: Vec<String> = harness
        .root()
        .children_recursive()
        .map(|n| n.accesskit_node())
        .filter(|n| n.role() == egui::accesskit::Role::Tab)
        .filter_map(|n| n.label())
        .collect();
    assert_eq!(tabs, ["Guía de estilo", "Recetas", "Notas de lectura"]);
    assert!(harness.query_by_label("Cerrar «Guía de estilo»").is_some());
    // The bar keeps its size frame after frame.
    let rect = |h: &Harness<'_, SilexApp>| {
        h.get_all_by_label("Notas de lectura")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Tab)
            .map(|n| n.rect())
    };
    let before = rect(&harness);
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(rect(&harness), before);
    harness
        .get_all_by_label("Recetas")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Tab)
        .expect("the «Recetas» tab")
        .click();
    harness.run();
    assert_eq!(harness.state().tabs.active(), 1);
    // Its «×» closes it.
    harness.get_by_label("Cerrar «Recetas»").click();
    harness.run();
    assert_eq!(harness.state().tabs.len(), 2);
    Ok(())
}

#[test]
fn tabs() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), tabs_demo()?);
    harness.run();
    harness.snapshot("tabs_light");
    Ok(())
}

// ---- H7.11: templates ----

/// The demo workspace with the «Plantillas» section unfolded (so the
/// examples exist) and «Reunión {{fecha}}» open.
fn templates_demo() -> anyhow::Result<SilexApp> {
    let mut app = demo_app()?;
    app.templates.open = true;
    app.db.create_example_templates()?;
    app.reload_pages()?;
    let meeting = app.templates.list[0].id.clone();
    app.open_page(meeting)?;
    Ok(app)
}

#[test]
fn the_templates_section_unfolds_and_everything_has_a_name() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    let count = |h: &Harness<'_, SilexApp>, label: &str| h.query_all_by_label(label).count();
    assert_eq!(count(&harness, "Diario {{fecha}}"), 0, "folded at first");
    click_button(&mut harness, "Plantillas");
    harness.run();
    assert!(harness.state().templates.open);
    for title in ["Reunión {{fecha}}", "Diario {{fecha}}", "Lista de tareas"] {
        assert!(count(&harness, title) > 0, "{title}");
    }
    assert_accessible(&harness, "templates section");
    // A page's menu offers both template actions.
    click_button(&mut harness, "Más acciones de la página");
    harness.run();
    assert!(menu_item(&harness, "Guardar como plantilla").is_some());
    assert!(menu_item(&harness, "Nueva página desde plantilla…").is_some());
    Ok(())
}

#[test]
fn an_open_template_and_the_picker_have_names() -> anyhow::Result<()> {
    let mut harness = a11y_harness(templates_demo()?);
    let count = |h: &Harness<'_, SilexApp>, label: &str| h.query_all_by_label(label).count();
    assert!(count(&harness, "Plantilla") > 0);
    assert!(
        harness
            .query_all_by_label_contains("Usar esta plantilla")
            .count()
            > 0
    );
    assert!(
        harness.query_by_label("Añadir a favoritos").is_none(),
        "no star"
    );
    assert_accessible(&harness, "open template");
    harness.state_mut().open_template_picker(None);
    harness.run();
    assert_accessible(&harness, "template picker");
    assert!(count(&harness, "Plantilla") >= 2, "the picker's field too");
    Ok(())
}

#[test]
fn templates() -> anyhow::Result<()> {
    templates_snapshot("templates_light", false)
}

#[test]
fn template_picker() -> anyhow::Result<()> {
    templates_snapshot("template_picker_light", true)
}

fn templates_snapshot(name: &str, picker: bool) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            templates_demo()?,
        );
    harness.run();
    if picker {
        harness.state_mut().open_template_picker(None);
        harness.run();
    }
    harness.snapshot(name);
    Ok(())
}

// ---- H7.8: page history ----

#[test]
fn the_page_history_has_names_and_opens_from_the_page_menu() -> anyhow::Result<()> {
    let (app, _) = super::page_history::history_fixture()?;
    let mut harness = a11y_harness(app);
    click_button(&mut harness, "Más acciones de la página");
    harness.run();
    menu_item(&harness, "Historial de versiones…")
        .expect("«Historial de versiones…» in the page menu")
        .click();
    harness.run();
    assert!(harness.state().page_history.is_some());
    harness.run();
    assert_accessible(&harness, "page history");
    assert!(harness.query_by_label("Restaurar esta versión").is_some());
    // Each version is a button named by when it was saved.
    let rows = harness
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| l.contains(':') && l.contains(','))
        .count();
    assert_eq!(rows, 3);
    Ok(())
}

/// The window keeps its size frame after frame (it once grew without end).
#[test]
fn the_page_history_window_keeps_its_size() -> anyhow::Result<()> {
    let (mut app, _) = super::page_history::history_fixture()?;
    super::page_history::open_fixed_history(&mut app)?;
    let mut harness = a11y_harness(app);
    let bounds = |h: &Harness<'_, SilexApp>| {
        let close = h.get_by_label("Cerrar").rect();
        let heading = h.get_by_label("Historial de versiones").rect();
        (close.min, heading.max.x)
    };
    harness.run();
    let first = bounds(&harness);
    for _ in 0..60 {
        harness.step();
    }
    let later = bounds(&harness);
    assert_eq!(first, later, "the window moved or grew");
    // Choosing another version does not change it either.
    harness.key_press(egui::Key::ArrowDown);
    harness.run();
    for _ in 0..30 {
        harness.step();
    }
    assert_eq!(bounds(&harness), first);
    Ok(())
}

#[test]
fn page_history() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (mut app, _) = super::page_history::history_fixture()?;
    super::page_history::open_fixed_history(&mut app)?;
    if let Some(history) = &mut app.page_history {
        history.selected = 1;
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness.snapshot("page_history_light");
    Ok(())
}

// ---- H12.11: a database inside a page ----

#[test]
fn the_page_menu_makes_a_database_inside_the_page() -> anyhow::Result<()> {
    let app = demo_app()?;
    let open = app.selected.clone().expect("a page is open");
    let mut harness = a11y_harness(app);
    click_button(&mut harness, "Más acciones de la página");
    harness.run();
    menu_item(&harness, "Nueva base de datos dentro")
        .expect("«Nueva base de datos dentro» in the page menu")
        .click();
    harness.run();
    let app = harness.state();
    let base = app.selected.clone().expect("the database is open");
    let page = app.db.get_page(&base)?.expect("made");
    assert_eq!(page.kind, silex_desktop::model::PageKind::Database);
    assert_eq!(page.parent_id.as_deref(), Some(open.as_str()));
    assert!(app.database.is_some());
    Ok(())
}

/// «Club de lectura», a page with a paragraph and the card of
/// «Lecturas», a database inside it: seven books with a state, an author,
/// a mark and a «Leído» box (five shown).
fn database_card_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    use silex_desktop::property::{CellValue, OptionColor, PropertyKind};
    let mut db = Database::open_in_memory()?;
    let club = db.create_page("Club de lectura", None)?;
    let base = db.create_database("Lecturas", Some(&club))?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let autor = db.add_property(&base, "Autor", PropertyKind::Text)?;
    let nota = db.add_property(&base, "Nota", PropertyKind::Number)?;
    let leido = db.add_property(&base, "Leído", PropertyKind::Checkbox)?;
    let hecho = db.add_select_option(&estado, "Leído", OptionColor::Green)?;
    let leyendo = db.add_select_option(&estado, "Leyendo", OptionColor::Blue)?;
    let pendiente = db.add_select_option(&estado, "Pendiente", OptionColor::Orange)?;
    for (title, state, author, mark) in [
        ("Dune", &hecho, "Frank Herbert", Some(9.0)),
        ("Solaris", &leyendo, "Stanisław Lem", None),
        ("Fundación", &pendiente, "Isaac Asimov", None),
        ("Neuromante", &hecho, "William Gibson", Some(7.0)),
        (
            "La mano izquierda de la oscuridad",
            &hecho,
            "Ursula K. Le Guin",
            Some(10.0),
        ),
        ("Hyperion", &pendiente, "Dan Simmons", None),
        ("Ubik", &leyendo, "Philip K. Dick", None),
    ] {
        let row = db.create_row(&base, title)?;
        db.set_value(&row, &estado, Some(&CellValue::Select(state.clone())))?;
        db.set_value(&row, &autor, Some(&CellValue::Text(author.into())))?;
        if let Some(mark) = mark {
            db.set_value(&row, &nota, Some(&CellValue::Number(mark)))?;
            db.set_value(&row, &leido, Some(&CellValue::Checkbox(true)))?;
        }
    }
    let block = |id: &str, kind: BlockKind, content: &str, link: Option<&str>| Block {
        id: id.into(),
        page_id: club.clone(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: link.map(str::to_owned),
        marks: Vec::new(),
    };
    db.save_document(
        &club,
        "Club de lectura",
        &[
            block(
                "p",
                BlockKind::Paragraph,
                "Lo que leemos este otoño, y lo que nos ha parecido.",
                None,
            ),
            block("l", BlockKind::PageLink, "", Some(&base)),
            block(
                "q",
                BlockKind::Paragraph,
                "Próxima reunión: el jueves.",
                None,
            ),
        ],
    )?;
    let mut app = SilexApp::new(db)?;
    app.open_page(club)?;
    Ok(app)
}

#[test]
fn the_database_card_is_a_link_named_with_its_rows() -> anyhow::Result<()> {
    let app = database_card_app()?;
    let mut harness = a11y_harness(app);
    harness.run();
    assert_accessible(&harness, "database card");
    let card = harness.get_by_label("Base de datos «Lecturas», 7 filas. Abrir");
    card.click();
    harness.run();
    let app = harness.state();
    assert!(app.database.is_some(), "a click opens the database");
    assert_eq!(
        app.selected
            .as_ref()
            .and_then(|id| app.pages.iter().find(|p| &p.id == id))
            .map(|p| p.title.as_str()),
        Some("Lecturas")
    );
    Ok(())
}

fn database_card(theme: eframe::egui::Theme, name: &str) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            database_card_app()?,
        );
    harness.run();
    harness.snapshot(name);
    Ok(())
}

#[test]
fn database_card_light() -> anyhow::Result<()> {
    database_card(eframe::egui::Theme::Light, "database_card_light")
}

#[test]
fn database_card_dark() -> anyhow::Result<()> {
    database_card(eframe::egui::Theme::Dark, "database_card_dark")
}

// ---- H12.10: comparing a version with the page now ----

#[test]
fn comparing_a_version_has_names_for_what_changed() -> anyhow::Result<()> {
    let (mut app, _) = super::page_history::history_fixture()?;
    super::page_history::open_fixed_history(&mut app)?;
    let mut harness = a11y_harness(app);
    harness.run();
    click_button(&mut harness, "Comparar con la actual");
    harness.run();
    assert!(
        harness
            .state()
            .page_history
            .as_ref()
            .is_some_and(|h| h.compare)
    );
    harness.run();
    assert_accessible(&harness, "page history compared");
    for label in [
        "Quitado: Sopas",
        "Quitado: Comprar calabaza",
        "Cambiado: Solo queda la idea de la sopa.",
    ] {
        assert!(has_label(&harness, label), "no «{label}»");
    }
    // A second click goes back to the version alone.
    click_button(&mut harness, "Comparar con la actual");
    harness.run();
    assert!(!has_label(&harness, "Quitado: Sopas"));
    Ok(())
}

/// Whether some node is named `label` (labels included, which
/// `query_by_label` leaves out).
fn has_label(harness: &Harness<'_, SilexApp>, label: &str) -> bool {
    harness
        .root()
        .children_recursive()
        .any(|n| n.accesskit_node().label().as_deref() == Some(label))
}

/// The second version (19 oct) compared with the page now.
fn page_history_compared(theme: eframe::egui::Theme, name: &str) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let (mut app, _) = super::page_history::history_fixture()?;
    super::page_history::open_fixed_history(&mut app)?;
    if let Some(history) = &mut app.page_history {
        history.selected = 1;
        history.compare = true;
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness.snapshot(name);
    Ok(())
}

#[test]
fn page_history_compared_light() -> anyhow::Result<()> {
    page_history_compared(eframe::egui::Theme::Light, "page_history_compared_light")
}

#[test]
fn page_history_compared_dark() -> anyhow::Result<()> {
    page_history_compared(eframe::egui::Theme::Dark, "page_history_compared_dark")
}

// ---- H7.5: links between pages ----

/// «Diario» with a paragraph, a link to «Diseño de la interfaz» (inside
/// «Proyecto Silex») and one to «Recetas», which is in the trash.
fn links_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    let mut app = demo_app()?;
    let find = |app: &SilexApp, title: &str| {
        app.pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .unwrap_or_default()
    };
    let diseno = find(&app, "Diseño de la interfaz");
    let recetas = find(&app, "Recetas");
    let diario = app.db.create_page("Diario", None)?;
    let block = |id: &str, kind: BlockKind, content: &str, link: Option<&str>| Block {
        id: id.into(),
        page_id: diario.clone(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: link.map(str::to_owned),
        marks: Vec::new(),
    };
    let blocks = [
        block("p", BlockKind::Paragraph, "Hoy revisé el diseño:", None),
        block("l1", BlockKind::PageLink, "", Some(&diseno)),
        block("l2", BlockKind::PageLink, "", Some(&recetas)),
    ];
    app.db.save_document(&diario, "Diario", &blocks)?;
    app.db.trash_page(&recetas)?;
    app.reload_pages()?;
    app.open_page(diario)?;
    Ok(app)
}

#[test]
fn link_cards_and_the_picker_have_names() -> anyhow::Result<()> {
    let mut harness = a11y_harness(links_app()?);
    assert_accessible(&harness, "links");
    assert!(
        harness
            .query_by_label("Enlace a «Diseño de la interfaz»")
            .is_some()
    );
    assert!(
        harness
            .query_by_label("Enlace a «Recetas», en la papelera")
            .is_some()
    );
    harness.state_mut().open_link_picker(None, None);
    harness.run();
    assert_accessible(&harness, "link picker");
    assert!(harness.query_by_label("Página a enlazar").is_some());
    // In the sidebar and in the picker.
    assert!(harness.query_all_by_label("Ideas sueltas").count() >= 2);
    Ok(())
}

#[test]
fn page_links() -> anyhow::Result<()> {
    links_snapshot("page_links_light", false)
}

#[test]
fn link_picker() -> anyhow::Result<()> {
    links_snapshot("link_picker_light", true)
}

/// «Recetas» open, linked from «Diario», from «Pan» (inside it) and from
/// the row «Zumos» of «Lecturas»; «Agenda», in the trash, links too.
fn backlinks_app() -> anyhow::Result<(SilexApp, String)> {
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let recetas = db.create_page("Recetas", None)?;
    let pan = db.create_page("Pan", Some(&recetas))?;
    let diario = db.create_page("Diario", None)?;
    let agenda = db.create_page("Agenda", None)?;
    let base = db.create_database("Lecturas", None)?;
    let zumos = db.create_row(&base, "Zumos")?;
    let link = |page: &str| Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind: BlockKind::PageLink,
        content: String::new(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: Some(recetas.clone()),
        marks: Vec::new(),
    };
    for (page, title) in [
        (&diario, "Diario"),
        (&pan, "Pan"),
        (&agenda, "Agenda"),
        (&zumos, "Zumos"),
    ] {
        db.save_document(page, title, &[link(page)])?;
    }
    db.trash_page(&agenda)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(recetas)?;
    Ok((app, diario))
}

#[test]
fn backlinks_are_listed_named_and_open_their_page() -> anyhow::Result<()> {
    let (app, diario) = backlinks_app()?;
    let mut harness = a11y_harness(app);
    assert_accessible(&harness, "backlinks");
    assert!(harness.query_by_label("Enlazada desde").is_some());
    let links: Vec<String> = harness
        .root()
        .children_recursive()
        .map(|n| n.accesskit_node())
        .filter(|n| n.role() == egui::accesskit::Role::Link)
        .filter_map(|n| n.label())
        .collect();
    // By title; not «Agenda», which is in the trash.
    assert_eq!(links, ["Diario", "Pan", "Zumos"]);

    // A click opens the page; «Diario» is linked from nobody.
    harness
        .get_all_by_label("Diario")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Link)
        .expect("the «Diario» backlink")
        .click();
    harness.run();
    assert_eq!(harness.state().selected.as_deref(), Some(diario.as_str()));
    assert!(harness.query_by_label("Enlazada desde").is_none());
    Ok(())
}

#[test]
fn backlinks_open_with_the_keyboard_and_follow_the_trash() -> anyhow::Result<()> {
    let (app, diario) = backlinks_app()?;
    let mut harness = a11y_harness(app);
    // Trashing a page that links here takes it out of the list at once.
    harness.state_mut().trash_page(&diario)?;
    harness.run();
    let recetas = harness.state().selected.clone();
    assert!(harness.query_by_label("Diario").is_none());
    let zumos = harness
        .get_all_by_label("Zumos")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Link)
        .expect("the «Zumos» backlink");
    zumos.focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    let state = harness.state();
    assert_ne!(state.selected, recetas);
    // A row opens as a row, with its properties.
    assert!(state.row_page.is_some());
    // The row shows its blocks, then nothing linked to it.
    assert!(harness.query_by_label("Enlazada desde").is_none());
    Ok(())
}

#[test]
fn backlinks() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            backlinks_app()?.0,
        );
    harness.run();
    harness.snapshot("backlinks_light");
    Ok(())
}

fn links_snapshot(name: &str, picker: bool) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 760.0])
        .with_theme(eframe::egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), links_app()?);
    harness.run();
    if picker {
        harness.state_mut().open_link_picker(None, None);
        harness.run();
        harness.get_by_label("Página a enlazar").type_text("dise");
        harness.run();
    }
    harness.snapshot(name);
    Ok(())
}

// ---- A database next to the page beside ----

/// A board next to the page beside, with the page at its minimum width:
/// the view toolbar must not overlap or spill under the sidebar, and
/// nothing may be pushed past the window's right edge.
#[test]
fn a_narrow_database_beside_a_page_keeps_its_toolbar_and_the_window() -> anyhow::Result<()> {
    const WIDTH: f32 = 1100.0;
    let (mut app, _) = board_app()?;
    let base = app.selected.clone().unwrap_or_default();
    app.main_width = 1000.0;
    app.open_beside(&base)?;
    let mut harness = Harness::builder()
        .with_size([WIDTH, 760.0])
        .with_theme(eframe::egui::Theme::Dark)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    // The toolbar measures itself on the first frames.
    for _ in 0..5 {
        harness.run();
    }
    if enabled() {
        harness.snapshot("database_beside_dark");
    }

    // Nothing past the window: the top bar's settings and the panel's «×».
    for label in ["Ajustes", "Cerrar el panel lateral"] {
        let rect = harness.get_by_label(label).rect();
        assert!(rect.right() <= WIDTH, "{label}: {rect:?}");
    }
    // The view tabs and the view's menus do not overlap, and the menus
    // stay inside the page (not under the sidebar).
    let tabs: Vec<_> = harness
        .get_all_by_label_contains("Vista «")
        .map(|n| n.rect())
        .collect();
    assert!(!tabs.is_empty());
    for menu in ["Agrupar", "Propiedades", "Ordenar"] {
        let menu = harness.get_by_label_contains(menu).rect();
        assert!(
            tabs.iter().all(|tab| !tab.intersects(menu)),
            "{tabs:?} {menu:?}"
        );
        assert!(menu.left() >= tabs[0].left(), "{menu:?}");
    }
    Ok(())
}

/// A page with every kind of inline formatting (H12.1b, ADR 0015), in
/// paragraphs, a heading, a list, a done task and a quote.
fn formatting_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::marks::{Mark, MarkKind};
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Formato en línea", None)?;
    let link = || MarkKind::Link("https://ejemplo.com".into());
    let specs: Vec<(BlockKind, bool, &str, Vec<Mark>)> = vec![
        (
            BlockKind::Heading2,
            false,
            "Un título con cursiva y código",
            vec![
                Mark::new(14, 21, MarkKind::Italic),
                Mark::new(24, 30, MarkKind::Code),
            ],
        ),
        (
            BlockKind::Paragraph,
            false,
            "Texto en negrita, en cursiva, en las dos, tachado y código en línea.",
            vec![
                Mark::new(9, 16, MarkKind::Bold),
                Mark::new(21, 28, MarkKind::Italic),
                Mark::new(33, 40, MarkKind::Bold),
                Mark::new(33, 40, MarkKind::Italic),
                Mark::new(42, 49, MarkKind::Strike),
                Mark::new(52, 58, MarkKind::Code),
            ],
        ),
        (
            BlockKind::Paragraph,
            false,
            "Un enlace web a la documentación, y otro en negrita.",
            vec![
                Mark::new(3, 13, link()),
                Mark::new(36, 51, link()),
                Mark::new(44, 51, MarkKind::Bold),
            ],
        ),
        (
            BlockKind::Bulleted,
            false,
            "Una lista con una palabra importante",
            vec![Mark::new(26, 36, MarkKind::Bold)],
        ),
        (
            BlockKind::Todo,
            true,
            "Una tarea hecha con negrita",
            vec![Mark::new(20, 27, MarkKind::Bold)],
        ),
        (
            BlockKind::Quote,
            false,
            "Una cita con un enlace dentro.",
            vec![Mark::new(16, 22, link())],
        ),
    ];
    let blocks: Vec<Block> = specs
        .into_iter()
        .enumerate()
        .map(|(position, (kind, checked, content, marks))| Block {
            id: format!("b{position}"),
            page_id: page.clone(),
            kind,
            content: content.to_owned(),
            checked,
            position: position as i64,
            indent: 0,
            attachment: None,
            link: None,
            marks,
        })
        .collect();
    db.save_document(&page, "Formato en línea", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

fn formatting_snapshot(name: &str, theme: egui::Theme) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 560.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            formatting_app()?,
        );
    harness.run();
    harness.snapshot(name);
    Ok(())
}

#[test]
fn inline_formatting_light() -> anyhow::Result<()> {
    formatting_snapshot("inline_formatting_light", egui::Theme::Light)
}

#[test]
fn inline_formatting_dark() -> anyhow::Result<()> {
    formatting_snapshot("inline_formatting_dark", egui::Theme::Dark)
}

/// H12.1c: the «…» menu of a block, opened over text selected in it:
/// «Formato» → «Negrita» makes just that text bold, with real clicks and
/// the focus moving to the menu. Its items have names.
#[test]
fn the_block_menu_formats_the_text_selected() -> anyhow::Result<()> {
    let mut harness = a11y_harness(demo_app()?);
    // «Una interfaz limpia, …»: select «interfaz».
    let paragraph = harness.state().blocks[1].id.clone();
    harness.state_mut().pending_focus = Some((paragraph.clone(), 12));
    harness.state_mut().pending_anchor = Some((paragraph.clone(), 4));
    harness.run();
    egui::Popup::open_id(&harness.ctx, super::blocks::handle_menu_id(&paragraph));
    harness.run();
    harness.get_by_label_contains("Formato").click();
    harness.run();
    assert_accessible(&harness, "format menu");
    harness.get_by_label_contains("Negrita").click();
    harness.run();
    let block = &harness.state().blocks[1];
    let bold: Vec<String> = block
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
        .collect();
    assert_eq!(bold, ["interfaz"]);
    Ok(())
}

/// H12.1d: the formatting page with «documentación» selected, Cmd+K and
/// an address typed: the field under the text, which stays highlighted.
fn link_edit_harness(theme: egui::Theme) -> anyhow::Result<Harness<'static, SilexApp>> {
    use eframe::egui::{Key, Modifiers};
    let mut harness = Harness::builder()
        .with_size([1100.0, 560.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            formatting_app()?,
        );
    harness.run();
    // «Un enlace web a la documentación, …»: select «documentación».
    harness.state_mut().pending_focus = Some(("b2".into(), 32));
    harness.state_mut().pending_anchor = Some(("b2".into(), 19));
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run();
    harness.event(egui::Event::Text("ejemplo.com/docs".into()));
    harness.run();
    Ok(harness)
}

#[test]
fn link_edit_light() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = link_edit_harness(egui::Theme::Light)?;
    harness.snapshot("link_edit_light");
    Ok(())
}

/// H12.1d: the link field and the block's links have names; Intro in the
/// field links the text; Cmd+clic on a link opens it (once, its URL) and
/// a plain click only places the cursor.
#[test]
fn web_links_are_named_set_with_the_field_and_opened_with_cmd_click() -> anyhow::Result<()> {
    use eframe::egui::{Key, Modifiers};
    let mut harness = link_edit_harness(egui::Theme::Light)?;
    assert_accessible(&harness, "link field");
    harness.get_by_label("Dirección del enlace");
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.state().link_edit.is_none());
    assert!(
        harness.state().blocks[2]
            .marks
            .iter()
            .any(|m| m.start == 19 && m.end == 32 && m.url() == Some("https://ejemplo.com/docs"))
    );
    let text = harness.state().blocks[2].content.clone();
    let field = harness.get_by(|node| {
        node.role() == egui::accesskit::Role::MultilineTextInput
            && node.value().as_deref() == Some(text.as_str())
    });
    let description = field.accesskit_node().description();
    assert_eq!(
        description.as_deref(),
        Some(
            "Enlaces: enlace web (https://ejemplo.com); documentación (https://ejemplo.com/docs); \
             otro en negrita (https://ejemplo.com)"
        )
    );
    // «enlace web» starts after «Un »: a little to the right of the
    // field's left edge, on its first line.
    let pos = field.rect().left_top() + egui::vec2(40.0, 10.0);
    let click =
        |harness: &mut Harness<'_, SilexApp>, modifiers: Modifiers| {
            harness.hover_at(pos);
            harness.run();
            // One event per frame (`step` keeps only the last frame's output).
            let mut urls = Vec::new();
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            };
            for event in [
                egui::Event::ModifiersChanged(modifiers),
                button(true),
                button(false),
                egui::Event::ModifiersChanged(Modifiers::NONE),
                egui::Event::PointerMoved(pos),
            ] {
                harness.event(event);
                harness.step();
                urls.extend(harness.output().platform_output.commands.iter().filter_map(
                    |c| match c {
                        egui::OutputCommand::OpenUrl(open) => Some(open.url.clone()),
                        _ => None,
                    },
                ));
            }
            urls
        };
    assert!(click(&mut harness, Modifiers::NONE).is_empty());
    harness.run();
    assert_eq!(
        click(&mut harness, Modifiers::COMMAND),
        ["https://ejemplo.com"]
    );
    Ok(())
}

/// H12.2 and H12.3: a divider between paragraphs and a callout with
/// formatting, next to a quote.
fn divider_callout_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::marks::{Mark, MarkKind};
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Separador y aviso", None)?;
    let specs: Vec<(BlockKind, &str, Vec<Mark>)> = vec![
        (
            BlockKind::Paragraph,
            "Un párrafo antes del separador.",
            vec![],
        ),
        (BlockKind::Divider, "", vec![]),
        (
            BlockKind::Callout,
            "Un aviso con negrita y un enlace, que ocupa el ancho de la columna y \
             sigue en una segunda línea cuando el texto es largo.",
            vec![
                // «negrita» and «y un enlace».
                Mark::new(13, 20, MarkKind::Bold),
                Mark::new(21, 32, MarkKind::Link("https://ejemplo.com".into())),
            ],
        ),
        (BlockKind::Quote, "Una cita, para comparar.", vec![]),
        (BlockKind::Divider, "", vec![]),
        (BlockKind::Paragraph, "Y un párrafo después.", vec![]),
    ];
    let blocks: Vec<Block> = specs
        .into_iter()
        .enumerate()
        .map(|(position, (kind, content, marks))| Block {
            id: format!("b{position}"),
            page_id: page.clone(),
            kind,
            content: content.to_owned(),
            checked: false,
            position: position as i64,
            indent: 0,
            attachment: None,
            link: None,
            marks,
        })
        .collect();
    db.save_document(&page, "Separador y aviso", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

fn divider_callout_snapshot(name: &str, theme: egui::Theme) -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 560.0])
        .with_theme(theme)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(
            |ui, app: &mut SilexApp| app.update_ui(ui),
            divider_callout_app()?,
        );
    harness.run();
    harness.snapshot(name);
    Ok(())
}

#[test]
fn divider_and_callout_light() -> anyhow::Result<()> {
    divider_callout_snapshot("divider_and_callout_light", egui::Theme::Light)
}

#[test]
fn divider_and_callout_dark() -> anyhow::Result<()> {
    divider_callout_snapshot("divider_and_callout_dark", egui::Theme::Dark)
}

/// The divider and the callout have names for screen readers.
#[test]
fn dividers_and_callouts_have_names() -> anyhow::Result<()> {
    let harness = a11y_harness(divider_callout_app()?);
    assert_accessible(&harness, "divider and callout");
    assert_eq!(harness.get_all_by_label("Separador").count(), 2);
    harness.get_by_label("Aviso");
    Ok(())
}

/// H12.4a: an open toggle with blocks inside (one a toggle itself), a
/// folded one and an empty one.
fn toggles_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    let page = db.create_page("Desplegables", None)?;
    let specs: Vec<(BlockKind, u8, &str)> = vec![
        (BlockKind::Toggle, 0, "Abierto, con contenido"),
        (BlockKind::Paragraph, 1, "Un párrafo dentro."),
        (BlockKind::Bulleted, 1, "Y una lista"),
        (BlockKind::Toggle, 1, "Otro desplegable dentro"),
        (BlockKind::Paragraph, 2, "Más adentro todavía."),
        (BlockKind::Toggle, 0, "Plegado: esconde lo que tiene"),
        (BlockKind::Paragraph, 1, "No se ve."),
        (BlockKind::Toggle, 0, "Vacío"),
        (BlockKind::Paragraph, 0, "Fuera de los desplegables."),
    ];
    let blocks: Vec<Block> = specs
        .into_iter()
        .enumerate()
        .map(|(position, (kind, indent, content))| Block {
            id: format!("b{position}"),
            page_id: page.clone(),
            kind,
            content: content.to_owned(),
            checked: false,
            position: position as i64,
            indent,
            attachment: None,
            link: None,
            marks: Vec::new(),
        })
        .collect();
    db.save_document(&page, "Desplegables", &blocks)?;
    db.save_folded_blocks(r#"["b5"]"#)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

#[test]
fn toggles_light() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1100.0, 560.0])
        .with_theme(egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), toggles_app()?);
    harness.run();
    harness.snapshot("toggles_light");
    Ok(())
}

/// The arrows have names that say what they do; a real click folds.
#[test]
fn toggle_arrows_are_named_and_fold_with_a_click() -> anyhow::Result<()> {
    let mut harness = a11y_harness(toggles_app()?);
    assert_accessible(&harness, "toggles");
    harness.get_by_label("Desplegar «Plegado: esconde lo que tiene»");
    harness
        .get_by_label("Plegar «Abierto, con contenido»")
        .click();
    harness.run();
    assert!(harness.state().folds.is_folded("b0"));
    harness.get_by_label("Desplegar «Abierto, con contenido»");
    assert!(
        harness
            .query_by_label("Plegar «Otro desplegable dentro»")
            .is_none()
    );
    Ok(())
}

/// H12.4b: dragging a folded toggle by its handle takes what it hides.
#[test]
fn a_dragged_toggle_takes_its_content() -> anyhow::Result<()> {
    let mut harness = a11y_harness(toggles_app()?);
    let below =
        block_rect(&harness, "Fuera de los desplegables.").center_bottom() + egui::vec2(0.0, 6.0);
    start_drag(&mut harness, "Plegado: esconde lo que tiene", below);
    harness.drop_at(below);
    harness.run();
    let ids: Vec<String> = harness
        .state()
        .blocks
        .iter()
        .map(|b| b.id.clone())
        .collect();
    assert_eq!(
        ids,
        ["b0", "b1", "b2", "b3", "b4", "b7", "b8", "b5", "b6"],
        "the toggle and the paragraph it hides, at the end"
    );
    assert!(harness.state().folds.is_folded("b5"), "still folded");
    Ok(())
}

/// H12.5: the outline beside a page with H1–H3, the heading being read
/// marked.
fn outline_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    db.save_settings(&silex_desktop::settings::Settings {
        outline: true,
        ..Default::default()
    })?;
    let page = db.create_page("Guía de uso", None)?;
    let specs: Vec<(BlockKind, &str)> = vec![
        (BlockKind::Heading1, "Primeros pasos"),
        (BlockKind::Paragraph, "Crea una página con Cmd+N."),
        (BlockKind::Heading2, "Escribir"),
        (
            BlockKind::Paragraph,
            "Escribe / para cambiar el tipo de bloque.",
        ),
        (BlockKind::Heading3, "Formato en línea"),
        (BlockKind::Paragraph, "Cmd+B, Cmd+I, Cmd+E."),
        (
            BlockKind::Heading2,
            "Organizar las páginas con un título bastante largo",
        ),
        (
            BlockKind::Paragraph,
            "Arrastra las páginas en la barra lateral.",
        ),
        (BlockKind::Heading1, "Copias de seguridad"),
        (BlockKind::Paragraph, "Se hacen solas cada día."),
    ];
    let blocks: Vec<Block> = specs
        .into_iter()
        .enumerate()
        .map(|(position, (kind, content))| Block {
            id: format!("b{position}"),
            page_id: page.clone(),
            kind,
            content: content.to_owned(),
            checked: false,
            position: position as i64,
            indent: 0,
            attachment: None,
            link: None,
            marks: Vec::new(),
        })
        .collect();
    db.save_document(&page, "Guía de uso", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

#[test]
fn outline_light() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = Harness::builder()
        .with_size([1280.0, 620.0])
        .with_theme(egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), outline_app()?);
    harness.run();
    harness.snapshot("outline_light");
    Ok(())
}

/// The outline's entries are named buttons; clicking one goes to it.
#[test]
fn outline_entries_are_named_and_go_to_their_heading() -> anyhow::Result<()> {
    let mut harness = a11y_harness(outline_app()?);
    assert_accessible(&harness, "outline");
    harness.get_by_label("Ocultar el índice");
    harness.get_by_label("Cerrar el índice");
    let entries: Vec<_> = harness
        .get_all_by_label("Copias de seguridad")
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .collect();
    assert_eq!(entries.len(), 1);
    entries[0].click();
    harness.run();
    let focused = harness.state().caret.as_ref().map(|c| c.block_id.clone());
    assert_eq!(focused.as_deref(), Some("b8"));
    Ok(())
}

/// A long page: five sections of twenty paragraphs, with the outline.
fn long_outline_app() -> anyhow::Result<SilexApp> {
    use silex_desktop::model::Block;
    let mut db = Database::open_in_memory()?;
    db.save_settings(&silex_desktop::settings::Settings {
        outline: true,
        ..Default::default()
    })?;
    let page = db.create_page("Larga", None)?;
    let mut blocks = Vec::new();
    for section in 1..=5 {
        let (kind, title) = if section % 2 == 1 {
            (BlockKind::Heading1, format!("Sección {section}"))
        } else {
            (BlockKind::Heading2, format!("Apartado {section}"))
        };
        blocks.push((kind, title));
        for n in 1..=20 {
            blocks.push((
                BlockKind::Paragraph,
                format!("Párrafo {n} de la sección {section}."),
            ));
        }
    }
    let blocks: Vec<Block> = blocks
        .into_iter()
        .enumerate()
        .map(|(position, (kind, content))| Block {
            id: format!("b{position}"),
            page_id: page.clone(),
            kind,
            content,
            checked: false,
            position: position as i64,
            indent: 0,
            attachment: None,
            link: None,
            marks: Vec::new(),
        })
        .collect();
    db.save_document(&page, "Larga", &blocks)?;
    let mut app = SilexApp::new(db)?;
    app.open_page(page)?;
    Ok(app)
}

/// H12.5: a heading chosen in the outline goes to the top and stays
/// marked (also when it cannot reach the top); scrolling by hand marks
/// the section being read again.
#[test]
fn the_outline_marks_the_heading_chosen_and_then_follows_the_scroll() -> anyhow::Result<()> {
    let mut harness = a11y_harness(long_outline_app()?);
    assert_eq!(harness.state().outline_current.as_deref(), Some("b0"));
    for (target, id) in [
        ("Sección 3", "b42"),
        ("Apartado 4", "b63"),
        ("Apartado 2", "b21"),
        ("Sección 5", "b84"),
    ] {
        harness
            .get_all_by_label(target)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .expect("entry")
            .click();
        harness.run();
        assert_eq!(
            harness.state().outline_current.as_deref(),
            Some(id),
            "{target}"
        );
        let caret = harness.state().caret.as_ref().map(|c| c.block_id.clone());
        assert_eq!(caret.as_deref(), Some(id), "{target}: the cursor is there");
        // At the top of the page's view.
        let top = block_rect(&harness, target).top();
        let first = harness
            .get_by_label("Más acciones de la página")
            .rect()
            .bottom();
        assert!(
            top - first < 80.0,
            "{target}: at the top ({top} vs {first})"
        );
    }
    // Scrolling up by hand: the section being read is marked again.
    let page = block_rect(&harness, "Sección 5").center();
    harness.hover_at(page);
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 700.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Wheel,
    });
    harness.run();
    let current = harness.state().outline_current.clone();
    assert!(
        current.as_deref() != Some("b84") && current.is_some(),
        "{current:?}"
    );
    Ok(())
}

/// A page with a very, very long title in the trash: the window stays on
/// screen, the title is cut, and every button can be reached, also in the
/// question «¿Borrar para siempre?».
#[test]
fn a_huge_title_never_pushes_the_trash_buttons_off_screen() -> anyhow::Result<()> {
    let db = Database::open_in_memory()?;
    let huge = format!("Título enorme {} fin", "sinespacios".repeat(300));
    let page = db.create_page(&huge, None)?;
    db.create_page("Otra", None)?;
    db.trash_page(&page)?;
    let mut app = SilexApp::new(db)?;
    app.open_trash_dialog();
    let mut harness = Harness::builder()
        .with_size([900.0, 600.0])
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .with_context_setup(theme::install)
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let on_screen = |harness: &Harness<'_, SilexApp>, label: &str| {
        let rect = harness
            .get_all_by_label_contains(label)
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .unwrap_or_else(|| panic!("button {label:?}"))
            .rect();
        assert!(screen.contains_rect(rect), "{label}: {rect:?}");
    };
    for label in ["Restaurar", "Borrar «", "Cerrar", "Vaciar la papelera"] {
        on_screen(&harness, label);
    }
    click_button(&mut harness, "Vaciar la papelera");
    for label in ["Confirmar: borrar para siempre", "Cancelar"] {
        on_screen(&harness, label);
    }
    harness
        .get_all_by_label("Cancelar")
        .next()
        .expect("cancel")
        .click();
    harness.run();
    // Deleting the item asks with its title cut short.
    harness
        .get_all_by_label_contains("Borrar «")
        .next()
        .expect("item")
        .click();
    harness.run();
    for label in ["Confirmar: borrar para siempre", "Cancelar"] {
        on_screen(&harness, label);
    }
    Ok(())
}

/// H12.8: the demo with page icons: in the sidebar, favorites, tabs and
/// above the title. Emoji come from the system's font, as in the app.
fn icons_app() -> anyhow::Result<SilexApp> {
    let mut app = demo_app()?;
    let id = |app: &SilexApp, title: &str| {
        app.pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .expect("page")
    };
    for (title, icon) in [
        ("Proyecto Silex", "🚀"),
        ("Diseño de la interfaz", "🎨"),
        ("Recetas", "🍞"),
        ("Guía de estilo", "📒"),
    ] {
        let page = id(&app, title);
        app.set_page_icon(&page, Some(icon))?;
    }
    app.reload_pages()?;
    Ok(app)
}

/// A harness that draws emoji: the system's emoji font, as eframe adds it
/// (`system_fonts`). Where there is none (a CI runner), missing glyphs are
/// boxes, not a failure.
fn emoji_harness(app: SilexApp, size: [f32; 2]) -> Harness<'static, SilexApp> {
    let mut harness = Harness::builder()
        .with_size(size)
        .with_theme(egui::Theme::Light)
        .with_os(eframe::egui::os::OperatingSystem::Mac)
        .allow_missing_glyphs()
        .with_context_setup(|ctx| {
            theme::install(ctx);
            ctx.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(|ui, app: &mut SilexApp| app.update_ui(ui), app);
    harness.run();
    harness
}

#[test]
fn page_icons_light() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut harness = emoji_harness(icons_app()?, [1100.0, 620.0]);
    harness.snapshot("page_icons_light");
    Ok(())
}

#[test]
fn icon_picker_light() -> anyhow::Result<()> {
    if !enabled() {
        return Ok(());
    }
    let mut app = icons_app()?;
    let open = app.selected.clone().expect("page");
    app.open_icon_picker(&open);
    let mut harness = emoji_harness(app, [1100.0, 700.0]);
    harness.snapshot("icon_picker_light");
    Ok(())
}

/// The picker's emoji have names; a click on one sets the icon; the icon
/// above the title opens the picker again, which can remove it.
#[test]
fn the_icon_picker_works_with_real_clicks() -> anyhow::Result<()> {
    let mut harness = emoji_harness(icons_app()?, [1100.0, 700.0]);
    let open = harness.state().selected.clone().expect("page");
    harness.state_mut().set_page_icon(&open, None)?;
    harness.run();
    harness.get_by_label("Añadir un icono a la página").click();
    harness.run();
    assert_accessible(&harness, "icon picker");
    // Found by a word: it is far down the list.
    harness.event(egui::Event::Text("espiga".into()));
    harness.run();
    harness.get_by_label("Icono: espiga").click();
    harness.run();
    assert_eq!(
        harness.state().page_icons.get(&open).map(String::as_str),
        Some("🌾")
    );
    harness
        .get_by_label("Cambiar el icono de la página")
        .click();
    harness.run();
    harness.get_by_label("Quitar icono").click();
    harness.run();
    assert!(!harness.state().page_icons.contains_key(&open));
    Ok(())
}

/// H12.9: «Hoy» in the sidebar opens today's note; Ajustes chooses the
/// template new days come from.
#[test]
fn hoy_and_the_journal_template_work_with_real_clicks() -> anyhow::Result<()> {
    let mut app = demo_app()?;
    let open = app.selected.clone().expect("page");
    app.db.save_as_template(&open)?;
    app.reload_pages()?;
    let mut harness = emoji_harness(app, [1100.0, 760.0]);
    harness.get_by_label("Ajustes").click();
    harness.run();
    harness
        .get_by_label_contains("Plantilla del diario")
        .hover();
    harness.run();
    menu_item(&harness, "Guía de estilo")
        .expect("the template in the list")
        .click();
    harness.run();
    let (_, template) = harness.state().db.journal_settings()?;
    assert!(template.is_some());
    harness.get_by_label("Abrir la nota de hoy").click();
    harness.run();
    let today = harness.state().db.local_today()?;
    assert_eq!(harness.state().title, today);
    assert!(!harness.state().blocks.is_empty(), "from the template");
    assert_accessible(&harness, "journal");
    Ok(())
}

// ---- H13.1: moving rows by hand ----

/// Rayuela dragged by its handle, held just above the end of the table.
fn drag_rayuela(harness: &mut Harness<'_, SilexApp>) {
    let row = harness.get_by_label("Seleccionar «Rayuela»").rect();
    harness.hover_at(row.center());
    harness.run();
    let handle = harness.get_by_label("Mover «Rayuela»").rect().center();
    harness.hover_at(handle);
    harness.run();
    harness.drag_at(handle);
    harness.run();
    harness.hover_at(handle + egui::vec2(0.0, 24.0));
    harness.run();
    let last = harness.get_by_label("Seleccionar «Ficciones»").rect();
    harness.hover_at(egui::pos2(last.center().x + 240.0, last.bottom() - 4.0));
    harness.run();
}

#[test]
fn database_row_drag() -> anyhow::Result<()> {
    database_snapshot(
        "database_row_drag_light",
        eframe::egui::Theme::Light,
        drag_rayuela,
    )
}

#[test]
fn database_row_drag_dark() -> anyhow::Result<()> {
    database_snapshot(
        "database_row_drag_dark",
        eframe::egui::Theme::Dark,
        drag_rayuela,
    )
}

#[test]
fn a_row_handle_has_a_name_and_is_not_a_tab_stop() -> anyhow::Result<()> {
    let (app, _, rows) = database_app()?;
    let mut harness = a11y_harness(app);
    let row = harness.get_by_label("Seleccionar «Rayuela»").rect();
    harness.hover_at(row.center());
    harness.run();
    assert!(harness.query_by_label("Mover «Rayuela»").is_some());
    assert_accessible(&harness, "row handle");
    // Not in the Tab order: «Subir» and «Bajar» in the row's menu are.
    let handle = harness
        .ctx
        .read_response(egui::Id::unique(("silex-row-handle", rows[0].as_str())))
        .expect("handle drawn");
    assert!(!handle.sense.is_focusable());
    Ok(())
}

#[test]
fn relationship_graph_dark() -> anyhow::Result<()> {
    snapshot_with("relationship_graph_dark", egui::Theme::Dark, |harness| {
        *harness.state_mut() = links_app().expect("graph fixture");
        harness.run();
        harness.get_by_label("Grafo").click();
        harness.run();
    })
}
