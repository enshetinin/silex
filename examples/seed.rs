//! Fills a NEW database with 100 pages × 10 blocks, plus a database («Base de
//! prueba») with 1000 rows and a property of each type, for manual UX checks.
//! Usage: cargo run --example seed -- /tmp/silex-dev.sqlite3
use anyhow::{Context, Result, bail};
use silex_desktop::db::Database;
use silex_desktop::model::BlockKind;
use silex_desktop::property::{CellValue, Date, OptionColor, PropertyKind};

const PAGES: usize = 100;
const BLOCKS_PER_PAGE: usize = 10;
const ROWS: usize = 1000;
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
    let base = db.create_database("Base de prueba", None)?;
    let notas = db.add_property(&base, "Notas", PropertyKind::Text)?;
    let numero = db.add_property(&base, "Número", PropertyKind::Number)?;
    let hecho = db.add_property(&base, "Hecho", PropertyKind::Checkbox)?;
    let fecha = db.add_property(&base, "Fecha", PropertyKind::Date)?;
    let estado = db.add_property(&base, "Estado", PropertyKind::Select)?;
    let options = ["Pendiente", "En curso", "Hecho"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| db.add_select_option(&estado, name, OptionColor::for_new_option(i)))
        .collect::<Result<Vec<_>>>()?;
    for r in 0..ROWS {
        let row = db.create_row(&base, &format!("Fila de prueba {r:04}"))?;
        let day = Date::new(2026, (r % 12 + 1) as u8, (r % 28 + 1) as u8).map(CellValue::Date);
        let cells = [
            (
                &notas,
                Some(CellValue::Text(format!("Nota de la fila {r}"))),
            ),
            (&numero, Some(CellValue::Number(r as f64 * 1.5))),
            (&hecho, Some(CellValue::Checkbox(r % 3 == 0))),
            (&fecha, day),
            (
                &estado,
                Some(CellValue::Select(options[r % options.len()].clone())),
            ),
        ];
        for (property, value) in cells {
            db.set_value(&row, property, value.as_ref())?;
        }
    }
    println!(
        "Creadas {PAGES} páginas, {} bloques y una base de {ROWS} filas con 5 propiedades en {}",
        PAGES * BLOCKS_PER_PAGE,
        path.to_string_lossy()
    );
    Ok(())
}
