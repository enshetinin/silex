mod app;

use anyhow::Result;
use silex_desktop::db::Database;

fn main() -> Result<()> {
    let db = Database::open_default()?;
    let app = app::SilexApp::new(db)?;
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1100.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native("Silex", options, Box::new(move |_cc| Ok(Box::new(app))))
        .map_err(|err| anyhow::anyhow!("No se puede iniciar la interfaz: {err}"))?;
    Ok(())
}
