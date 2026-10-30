//! Global search (H5.7): ranking, prefixes, snippets and safety.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use silex_desktop::db::Database;
use silex_desktop::model::{BlockKind, NewBlock};
use silex_desktop::search::{MAX_RESULTS, SearchHit, SearchWorker, TextPart};

fn paragraphs(texts: &[&str]) -> Vec<NewBlock> {
    texts
        .iter()
        .map(|text| NewBlock {
            kind: BlockKind::Paragraph,
            indent: 0,
            checked: false,
            content: (*text).to_owned(),
            marks: Vec::new(),
        })
        .collect()
}

fn titles(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|h| h.page.title.as_str()).collect()
}

/// The snippet with matches in [brackets].
fn marked(parts: &[TextPart]) -> String {
    parts
        .iter()
        .map(|p| {
            if p.matched {
                format!("[{}]", p.text)
            } else {
                p.text.clone()
            }
        })
        .collect()
}

#[test]
fn title_matches_come_before_content_matches() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.create_page_with_blocks("Notas", None, &paragraphs(&["comprar pan y leche"]))?;
    db.create_page("Pan de centeno", None)?;
    db.create_page_with_blocks("Panadería", None, &paragraphs(&["sin coincidencias aquí"]))?;
    db.create_page("Otra cosa", None)?;

    let hits = db.search("pan")?;
    // "Panadería" starts with "pan", so it is a title match too.
    assert_eq!(titles(&hits)[2], "Notas");
    assert_eq!(hits.len(), 3);
    assert!(hits[0].title_matched() && hits[1].title_matched());
    assert!(!hits[2].title_matched());
    let first_two: Vec<&str> = titles(&hits)[..2].to_vec();
    assert!(first_two.contains(&"Pan de centeno") && first_two.contains(&"Panadería"));
    Ok(())
}

#[test]
fn better_content_matches_rank_first() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    // Filler pages, so "trineo" is a rare word (bm25 needs a corpus).
    for i in 0..10 {
        db.create_page_with_blocks(&format!("Relleno {i}"), None, &paragraphs(&["nada"]))?;
    }
    db.create_page_with_blocks(
        "Una mención",
        None,
        &paragraphs(&["El viaje fue largo y frío; en algún momento se habló de un trineo, pero nadie lo usó al final del día."]),
    )?;
    db.create_page_with_blocks(
        "Muchas menciones",
        None,
        &paragraphs(&["trineo, trineo y trineo"]),
    )?;

    let hits = db.search("trineo")?;
    assert_eq!(titles(&hits), ["Muchas menciones", "Una mención"]);
    Ok(())
}

#[test]
fn words_match_as_prefixes_ignoring_case_and_accents() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.create_page_with_blocks(
        "Viaje",
        None,
        &paragraphs(&["La EXPEDICIÓN al Ártico salió en 1921"]),
    )?;
    for query in [
        "exped",
        "expedicion",
        "ártic",
        "ARTICO",
        "exped artic",
        "192",
    ] {
        assert_eq!(titles(&db.search(query)?), ["Viaje"], "{query:?}");
    }
    // Every word must appear; a word in the middle of another does not count.
    assert!(db.search("exped barco")?.is_empty());
    assert!(db.search("pedicion")?.is_empty());
    Ok(())
}

#[test]
fn snippet_marks_the_matches_of_the_best_block() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let long = "Uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece \
                catorce quince dieciséis harina de centeno diecisiete dieciocho \
                diecinueve veinte veintiuno veintidós veintitrés veinticuatro";
    let page = db.create_page_with_blocks(
        "Pan",
        None,
        &paragraphs(&["primer bloque", long, "línea uno\nlínea dos con centeno"]),
    )?;
    let blocks = db.list_blocks(&page)?;

    let hits = db.search("harina cent")?;
    assert_eq!(hits.len(), 1);
    let block = hits[0].block.as_ref().expect("content match");
    assert_eq!(block.block_id, blocks[1].id);
    let snippet = marked(&block.snippet);
    assert!(snippet.contains("[harina] de [centeno]"), "{snippet}");
    assert!(
        snippet.starts_with('…') && snippet.ends_with('…'),
        "{snippet}"
    );
    // The cursor goes to the first match, counted in characters.
    let byte = blocks[1].content.find("harina").expect("word in block");
    assert_eq!(block.char_offset, blocks[1].content[..byte].chars().count());
    Ok(())
}

#[test]
fn snippets_are_one_line_and_titles_are_marked() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.create_page_with_blocks(
        "Pan de centeno",
        None,
        &paragraphs(&["línea uno\nlínea dos con centeno"]),
    )?;
    let hits = db.search("centeno")?;
    let hit = &hits[0];
    assert_eq!(marked(&hit.title), "Pan de [centeno]");
    let block = hit.block.as_ref().expect("content match");
    assert_eq!(marked(&block.snippet), "línea uno línea dos con [centeno]");
    // "línea uno\nlínea dos con " is 24 characters.
    assert_eq!(block.char_offset, 24);

    // A page found only by its content shows its title unmarked.
    db.create_page_with_blocks("Receta", None, &paragraphs(&["masa madre"]))?;
    let hits = db.search("madre")?;
    assert_eq!(marked(&hits[0].title), "Receta");
    assert!(!hits[0].title_matched());
    Ok(())
}

#[test]
fn unicode_text_is_found_and_offsets_are_in_characters() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let text = "👨‍👩‍👧 año ñandú 日本語のテキスト";
    let page = db.create_page_with_blocks("Unicode", None, &paragraphs(&[text]))?;
    let hits = db.search("ñandu")?;
    assert_eq!(titles(&hits), ["Unicode"]);
    let block = hits[0].block.as_ref().expect("content match");
    let byte = text.find("ñandú").expect("word in text");
    assert_eq!(block.char_offset, text[..byte].chars().count());
    assert_eq!(db.list_blocks(&page)?[0].content, text);
    // CJK runs are one token; a prefix finds them.
    assert_eq!(titles(&db.search("日本")?), ["Unicode"]);
    Ok(())
}

#[test]
fn trashed_pages_are_not_found() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let parent = db.create_page_with_blocks("Islandia", None, &paragraphs(&["géiser"]))?;
    db.create_page_with_blocks("Reikiavik", Some(&parent), &paragraphs(&["géiser"]))?;
    db.create_page_with_blocks("Noruega", None, &paragraphs(&["fiordo y géiser"]))?;
    db.trash_page(&parent)?;
    assert_eq!(titles(&db.search("geiser")?), ["Noruega"]);
    assert!(db.search("islandia")?.is_empty());
    db.restore_page(&parent)?;
    assert_eq!(db.search("geiser")?.len(), 3);
    Ok(())
}

#[test]
fn one_result_per_page_and_results_follow_edits() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    let page = db.create_page_with_blocks(
        "Lista",
        None,
        &paragraphs(&["manzana", "manzana verde", "manzana roja"]),
    )?;
    assert_eq!(db.search("manzana")?.len(), 1);

    let mut blocks = db.list_blocks(&page)?;
    for block in &mut blocks {
        block.content = block.content.replace("manzana", "pera");
    }
    db.save_document(&page, "Lista", &blocks)?;
    assert!(db.search("manzana")?.is_empty());
    assert_eq!(db.search("pera")?.len(), 1);
    Ok(())
}

#[test]
fn fts_syntax_and_empty_input_are_safe() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    db.create_page_with_blocks(
        "Plan NEAR OR mañana",
        None,
        &paragraphs(&["texto con \"comillas\" y * asteriscos"]),
    )?;
    for input in [
        "",
        "   ",
        "\"",
        "*",
        "NEAR(",
        "OR",
        "-x",
        "'; DROP TABLE pages; --",
        "a\"b",
        "(",
        "^",
        "\u{0}",
        "\u{2}",
        "\u{3}",
    ] {
        db.search(input)?;
    }
    assert!(db.search("")?.is_empty());
    // Operators are plain words.
    assert_eq!(titles(&db.search("near or")?), ["Plan NEAR OR mañana"]);
    assert_eq!(titles(&db.search("\"comillas\"")?), ["Plan NEAR OR mañana"]);
    assert_eq!(db.list_pages()?.len(), 1);
    Ok(())
}

#[test]
fn results_are_capped() -> anyhow::Result<()> {
    let mut db = Database::open_in_memory()?;
    for i in 0..MAX_RESULTS + 5 {
        db.create_page_with_blocks(&format!("Nota {i}"), None, &paragraphs(&["común"]))?;
    }
    assert_eq!(db.search("comun")?.len(), MAX_RESULTS);
    Ok(())
}

/// Temporary on-disk database; removes the file and its WAL/SHM even if the test fails.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("silex-search-{}.sqlite3", uuid::Uuid::new_v4())))
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

/// Polls until the worker answers (or 30 s pass).
fn wait(worker: &mut SearchWorker) -> anyhow::Result<Vec<SearchHit>> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if let Some(result) = worker.poll() {
            return result;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    anyhow::bail!("the search worker did not answer")
}

#[test]
fn worker_answers_the_latest_request_from_its_own_connection() -> anyhow::Result<()> {
    let tmp = TempDb::new();
    let mut db = Database::open(&tmp.0)?;
    db.create_page_with_blocks("Compra", None, &paragraphs(&["pan y leche"]))?;
    db.create_page("Pan de centeno", None)?;
    let answered = Arc::new(AtomicUsize::new(0));
    let counter = answered.clone();
    let mut worker = SearchWorker::spawn(tmp.0.clone(), move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })?;

    worker.request("pan".into());
    assert!(worker.is_pending());
    assert_eq!(titles(&wait(&mut worker)?).len(), 2);
    assert!(!worker.is_pending());
    assert!(answered.load(Ordering::SeqCst) >= 1, "callback ran");

    // Two requests in a row: only the second one's result comes back.
    worker.request("pan".into());
    worker.request("leche".into());
    assert_eq!(titles(&wait(&mut worker)?), ["Compra"]);
    assert!(worker.poll().is_none(), "the older result is dropped");

    // Writes from the main connection are seen by the worker.
    db.create_page("Leche de avena", None)?;
    worker.request("leche".into());
    assert_eq!(titles(&wait(&mut worker)?).len(), 2);

    // A cancelled request never answers.
    worker.request("pan".into());
    worker.cancel();
    std::thread::sleep(Duration::from_millis(50));
    assert!(worker.poll().is_none());
    drop(worker);
    drop(db);
    Ok(())
}

#[test]
fn worker_reports_a_database_it_cannot_open() -> anyhow::Result<()> {
    let missing_dir = std::env::temp_dir()
        .join(format!("silex-missing-{}", uuid::Uuid::new_v4()))
        .join("silex.sqlite3");
    let mut worker = SearchWorker::spawn(missing_dir, || {})?;
    worker.request("pan".into());
    let err = wait(&mut worker).expect_err("cannot open");
    assert!(format!("{err:#}").contains("No se puede abrir"), "{err:#}");
    Ok(())
}

/// Manual: `cargo test --test search -- --ignored --nocapture`. Prints timings, never fails on speed.
#[test]
#[ignore = "scale smoke test; run manually"]
fn search_scale_smoke() -> anyhow::Result<()> {
    for (pages, per_page) in [(100, 10), (1000, 10), (1000, 50)] {
        let mut db = Database::open_in_memory()?;
        for p in 0..pages {
            let texts: Vec<String> = (0..per_page)
                .map(|b| {
                    format!(
                        "Bloque {b} de la página {p}: texto de ejemplo número {} con algo de contenido",
                        p * per_page + b
                    )
                })
                .collect();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            db.create_page_with_blocks(&format!("Página {p}"), None, &paragraphs(&refs))?;
        }
        for query in ["e", "texto", "ejemplo número 5", "inexistente"] {
            let t = Instant::now();
            let hits = db.search(query)?;
            println!(
                "{pages} páginas / {} bloques, {query:?}: {} resultados en {:?}",
                pages * per_page,
                hits.len(),
                t.elapsed()
            );
        }
    }
    Ok(())
}
