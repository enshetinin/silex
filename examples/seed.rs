//! Fills a NEW database with 100 pages × 10 blocks for manual UX checks.
//! Usage: cargo run --example seed -- /tmp/silex-dev.sqlite3
use anyhow::{Context, Result, bail};
use silex_desktop::db::Database;
use silex_desktop::model::BlockKind;

const PAGES: usize = 100;
const BLOCKS_PER_PAGE: usize = 10;
const KINDS: [BlockKind; 9] = BlockKind::ALL;

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("Uso: cargo run --example seed -- <ruta-nueva.sqlite3>")?;
    if std::path::Path::new(&path).exists() {
        bail!("{} ya existe; usa una ruta nueva", path.to_string_lossy());
    }
    let mut db = Database::open(&path)?;
    for p in 0..PAGES {
        let title = format!("Página de prueba {p:03}");
        let page_id = db.create_page(&title, None)?;
        for b in 0..BLOCKS_PER_PAGE {
            db.add_block(&page_id, KINDS[b % KINDS.len()])?;
        }
        let mut blocks = db.list_blocks(&page_id)?;
        for (b, block) in blocks.iter_mut().enumerate() {
            block.content = format!(
                "Bloque {b} de la página {p}: texto de ejemplo número {}",
                p * 10 + b
            );
            block.checked = block.is_todo() && b % 2 == 0;
        }
        db.save_document(&page_id, &title, &blocks)?;
    }
    println!(
        "Creadas {PAGES} páginas y {} bloques en {}",
        PAGES * BLOCKS_PER_PAGE,
        path.to_string_lossy()
    );
    Ok(())
}
