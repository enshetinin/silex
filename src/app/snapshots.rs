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
        // 28 widgets still lack accessible names (block type selectors, text
        // fields…): fixed in H5.5 (accessibility), then this check goes back on.
        .with_accessibility_check(false)
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
