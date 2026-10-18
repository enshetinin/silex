// Release builds on Windows are GUI apps: no console window. Errors are
// shown in a window instead (`StartupError`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use anyhow::Result;
use silex_desktop::db::Database;

fn main() -> Result<()> {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Silex")
        .with_app_id(app::APP_ID)
        .with_inner_size([1100.0, 760.0]);
    if let Some(icon) = app::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    // A packaged app has no terminal: if it cannot start, say why on screen.
    let app: Box<dyn eframe::App> = match Database::open_default().and_then(app::SilexApp::new) {
        Ok(app) => Box::new(app),
        Err(err) => {
            eprintln!("Silex: {err:#}");
            let path = Database::default_path().ok();
            Box::new(app::StartupError::new(&err, path.as_deref()))
        }
    };
    eframe::run_native("Silex", options, Box::new(move |_cc| Ok(app)))
        .map_err(|err| anyhow::anyhow!("No se puede iniciar la interfaz: {err}"))?;
    Ok(())
}
