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

/// Every control has a name, and no name is only an icon glyph (Private Use
/// Area characters mean nothing to a screen reader).
#[track_caller]
fn assert_accessible(harness: &Harness<'_, SilexApp>, state: &str) {
    let unnamed = harness.unnamed_widgets();
    assert!(
        unnamed.is_empty(),
        "{state}: unnamed widgets:\n{unnamed:#?}"
    );
    let is_icon = |c: char| ('\u{E000}'..='\u{F8FF}').contains(&c);
    let icon_only: Vec<String> = harness
        .root()
        .children_recursive()
        .map(|node| node.accesskit_node())
        .filter(|node| !node.is_hidden())
        .filter_map(|node| {
            let label = node.label()?;
            let meaningful = label.chars().any(|c| !c.is_whitespace() && !is_icon(c));
            (!label.trim().is_empty() && !meaningful)
                .then(|| format!("{:?} {label:?} at {:?}", node.role(), node.raw_bounds()))
        })
        .collect();
    assert!(
        icon_only.is_empty(),
        "{state}: icon-only names:\n{icon_only:#?}"
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
    assert_eq!(stored.zoom_percent, 125);
    Ok(())
}

#[test]
fn stored_settings_apply_at_startup_and_keyboard_zoom_is_remembered() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.save_settings(&silex_desktop::settings::Settings {
        theme: silex_desktop::settings::ThemeMode::Dark,
        zoom_percent: 125,
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
            // Tab from the search button: the footer comes before the tree
            // (its panel is laid out first).
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

    click_button(&mut harness, "Estado, Selección");
    assert_accessible(&harness, "property menu");
    assert!(menu_item(&harness, "Mover a la izquierda").is_some());
    egui::Popup::close_all(&harness.ctx);
    harness.run();

    click_button(&mut harness, "Estado, Ficciones: Leyendo");
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
        |harness| click_button(harness, "Estado, Rayuela: Terminado"),
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
    click_button(&mut harness, "Estado, Selección");
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
            click_button(harness, "Estado, Selección");
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
    }];
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(1200u32.to_be_bytes());
    png.extend(800u32.to_be_bytes());
    png.resize(245_000, 0);
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
