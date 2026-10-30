//! Fills a NEW database with 100 pages × 10 blocks, plus a database («Base de
//! prueba») with 1000 rows and a property of each type, for manual UX checks.
//! Also the Hito 7 pages («QA · …»): attachments (an image, a PDF, a script,
//! two large files and one no block uses), links and backlinks, and a page
//! with versions from earlier days.
//! Usage: cargo run --example seed -- /tmp/silex-dev.sqlite3
use anyhow::{Context, Result, bail};
use silex_desktop::attachment::{NewAttachment, encode_png};
use silex_desktop::db::Database;
use silex_desktop::model::{Block, BlockKind};
use silex_desktop::property::{CellValue, Date, OptionColor, PropertyKind};

const PAGES: usize = 100;
const BLOCKS_PER_PAGE: usize = 10;
const ROWS: usize = 1000;
const KINDS: [BlockKind; BlockKind::ALL.len()] = BlockKind::ALL;

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
    let first_row = db
        .database_rows(&base)?
        .first()
        .map(|r| r.id.clone())
        .context("la base no tiene filas")?;
    hito7(&mut db, &first_row)?;
    formatting(&mut db)?;
    println!(
        "Creadas {PAGES} páginas, {} bloques, una base de {ROWS} filas con 5 propiedades \
         y las páginas «QA · …» del Hito 7 y del formato en línea en {}",
        PAGES * BLOCKS_PER_PAGE,
        path.to_string_lossy()
    );
    Ok(())
}

/// A block of `page` (its position is the list order when saved).
fn block(page: &str, kind: BlockKind, content: &str) -> Block {
    Block {
        id: uuid::Uuid::new_v4().to_string(),
        page_id: page.into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks: Vec::new(),
    }
}

/// A file block for a new attachment named `name` with `data`.
fn file(db: &mut Database, page: &str, name: &str, data: Vec<u8>) -> Result<Block> {
    let attachment = db.add_attachment(&NewAttachment::from_bytes(name, data)?)?;
    Ok(Block {
        attachment: Some(attachment.id),
        ..block(page, BlockKind::File, &attachment.name)
    })
}

fn link(page: &str, to: &str) -> Block {
    Block {
        link: Some(to.into()),
        ..block(page, BlockKind::PageLink, "")
    }
}

/// «QA · Formato en línea» (H12.1b, ADR 0015): every kind of inline
/// formatting in the block kinds that take it, plus a code block, a
/// divider, a callout and a toggle (H12.2–H12.4).
fn formatting(db: &mut Database) -> Result<()> {
    use silex_desktop::marks::{Mark, MarkKind};
    let page = db.create_page("QA · Formato en línea", None)?;
    db.set_favorite(&page, true)?;
    // Marks over the first occurrence of each word (in characters).
    let marked = |kind: BlockKind, text: &str, words: &[(&str, MarkKind)]| -> Result<Block> {
        let mut out = block(&page, kind, text);
        for (word, mark) in words {
            let byte = text.find(word).context("palabra que no está en el texto")?;
            let start = text[..byte].chars().count();
            out.marks
                .push(Mark::new(start, start + word.chars().count(), mark.clone()));
        }
        Ok(out)
    };
    let web = || MarkKind::Link("https://github.com/enshetinin/silex".into());
    let mut todo = marked(
        BlockKind::Todo,
        "Tarea hecha con negrita",
        &[("negrita", MarkKind::Bold)],
    )?;
    todo.checked = true;
    let blocks = vec![
        marked(
            BlockKind::Heading2,
            "Título con cursiva y código",
            &[("cursiva", MarkKind::Italic), ("código", MarkKind::Code)],
        )?,
        marked(
            BlockKind::Paragraph,
            "Escribe dentro de esta negrita y al final de esta otra: sigue en negrita. Al final de código o de un enlace, no.",
            &[
                ("esta negrita", MarkKind::Bold),
                ("esta otra", MarkKind::Bold),
                ("código", MarkKind::Code),
                ("un enlace", web()),
            ],
        )?,
        marked(
            BlockKind::Paragraph,
            "Cursiva, tachado y las dos juntas con acentos: camión, pingüino, 日本語 🌾.",
            &[
                ("Cursiva", MarkKind::Italic),
                ("tachado", MarkKind::Strike),
                ("las dos juntas", MarkKind::Bold),
                ("las dos juntas", MarkKind::Italic),
                ("日本語 🌾", MarkKind::Bold),
            ],
        )?,
        marked(
            BlockKind::Bulleted,
            "Pulsa Intro en medio de esta negrita para dividirla",
            &[("esta negrita", MarkKind::Bold)],
        )?,
        todo,
        marked(
            BlockKind::Quote,
            "Una cita con un enlace al repositorio.",
            &[("enlace al repositorio", web())],
        )?,
        block(
            &page,
            BlockKind::Code,
            "// Convertir un bloque en código quita su formato",
        ),
        // H12.2 and H12.3.
        block(&page, BlockKind::Divider, ""),
        marked(
            BlockKind::Callout,
            "Un aviso con negrita y un enlace: Intro sigue en un párrafo normal.",
            &[("negrita", MarkKind::Bold), ("un enlace", web())],
        )?,
        // H12.4 (ADR 0016): a toggle with a block inside.
        marked(
            BlockKind::Toggle,
            "Un desplegable: la flecha o Cmd+Intro lo pliegan",
            &[("Cmd+Intro", MarkKind::Code)],
        )?,
        Block {
            indent: 1,
            ..block(
                &page,
                BlockKind::Paragraph,
                "Esto está dentro del desplegable.",
            )
        },
    ];
    db.save_document(&page, "QA · Formato en línea", &blocks)?;
    Ok(())
}

/// The Hito 7 pages: attachments (H7.1–H7.4, H7.9), links and backlinks
/// (H7.5–H7.6) and history (H7.7–H7.8). `row` is a row of the database.
fn hito7(db: &mut Database, row: &str) -> Result<()> {
    let qa = db.create_page("QA · Hito 7", None)?;
    db.set_favorite(&qa, true)?;

    // Attachments: an image with a preview, a PDF, a script (never opened),
    // two large files (the export runs on a worker) and one no block uses
    // («Liberar espacio» deletes it).
    let files = db.create_page("QA · Adjuntos", Some(&qa))?;
    let (w, h) = (1200u32, 800u32);
    let rgba: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [(40 + x * 180 / w) as u8, (90 + y * 120 / h) as u8, 200, 255]
        })
        .collect();
    let large = |n: u8| -> Vec<u8> {
        let mut data = format!("%PDF-1.7 grande {n}\n").into_bytes();
        data.resize(18_000_000, b' ');
        data
    };
    let blocks = vec![
        block(
            &files,
            BlockKind::Paragraph,
            "Una imagen, un PDF, un script y dos archivos grandes (18 MB cada uno).",
        ),
        file(db, &files, "degradado.png", encode_png(w, h, rgba)?)?,
        file(
            db,
            &files,
            "factura.pdf",
            b"%PDF-1.7\nfactura de prueba".to_vec(),
        )?,
        file(db, &files, "instalar.sh", b"#!/bin/sh\necho hola".to_vec())?,
        file(db, &files, "grande 1.pdf", large(1))?,
        file(db, &files, "grande 2.pdf", large(2))?,
    ];
    db.save_document(&files, "QA · Adjuntos", &blocks)?;
    db.add_attachment(&NewAttachment::from_bytes(
        "sin usar.pdf",
        b"%PDF-1.7\nnadie me usa".to_vec(),
    )?)?;

    // Links: to a page, to the database, to a row and to a page in the
    // trash. «QA · Adjuntos» and a row link back to «QA · Enlaces».
    let links = db.create_page("QA · Enlaces", Some(&qa))?;
    let trashed = db.create_page("QA · En la papelera", Some(&qa))?;
    let pages = db.list_pages()?;
    let find = |title: &str| {
        pages
            .iter()
            .find(|p| p.title == title)
            .map(|p| p.id.clone())
            .with_context(|| format!("falta «{title}»"))
    };
    let base = find("Base de prueba")?;
    let blocks = vec![
        block(
            &links,
            BlockKind::Paragraph,
            "Enlaces a una página, a una base, a una fila y a una página en la papelera:",
        ),
        link(&links, &files),
        link(&links, &base),
        link(&links, row),
        link(&links, &trashed),
    ];
    db.save_document(&links, "QA · Enlaces", &blocks)?;
    let mut back = db.list_blocks(&files)?;
    back.push(link(&files, &links));
    db.save_document(&files, "QA · Adjuntos", &back)?;
    db.save_document(row, &title_of(db, row)?, &[link(row, &links)])?;
    db.trash_page(&trashed)?;

    // History: versions from three days ago, yesterday and this morning
    // (local times depend on the time zone), then today's text.
    let history = db.create_page("QA · Historial", Some(&qa))?;
    let mut blocks = vec![
        block(&history, BlockKind::Heading1, "Plan del viaje"),
        block(&history, BlockKind::Paragraph, "Primera idea: ir en tren."),
    ];
    let days = days_since_epoch()?;
    // A page without blocks leaves no version: start four days ago.
    db.set_clock(Some(&format!("{}T09:00:00.000Z", civil(days - 4))));
    db.save_document(&history, "QA · Historial", &blocks)?;
    blocks.push(block(
        &history,
        BlockKind::Todo,
        "Segunda idea: ir en coche.",
    ));
    for (ago, hour, text) in [
        (3, 10, "Tercera idea: ir en avión."),
        (1, 18, "Decidido: en tren, con una parada."),
        (0, 0, "Comprar los billetes."),
    ] {
        db.set_clock(Some(&format!("{}T{hour:02}:00:00.000Z", civil(days - ago))));
        db.save_document(&history, "QA · Historial", &blocks)?;
        blocks.push(block(&history, BlockKind::Todo, text));
    }
    db.set_clock(None);
    db.save_document(&history, "QA · Historial", &blocks)?;
    Ok(())
}

fn title_of(db: &Database, page: &str) -> Result<String> {
    Ok(db.get_page(page)?.map(|p| p.title).unwrap_or_default())
}

/// Days since 1970-01-01 (UTC), today.
fn days_since_epoch() -> Result<i64> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("el reloj del sistema es anterior a 1970")?
        .as_secs();
    Ok((secs / 86_400) as i64)
}

/// `YYYY-MM-DD` of a day counted from 1970-01-01 (proleptic Gregorian).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
