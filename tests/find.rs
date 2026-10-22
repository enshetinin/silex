//! Find and replace in the page (H11.7): pure functions over the blocks,
//! with the Unicode cases that break naive code.
use silex_desktop::editor::{
    Match, find_in_blocks, find_in_text, fold, match_from, replace_matches,
};
use silex_desktop::model::{Block, BlockKind};

fn b(kind: BlockKind, content: &str) -> Block {
    Block {
        id: content.into(),
        page_id: "p".into(),
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

/// The text of each match, to read the tests at a glance.
fn found(text: &str, query: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    find_in_text(text, query)
        .into_iter()
        .map(|r| chars[r].iter().collect())
        .collect()
}

#[test]
fn case_and_accents_do_not_matter_either_way() {
    assert_eq!(
        found("Código y CODIGO y código", "codigo"),
        ["Código", "CODIGO", "código"]
    );
    assert_eq!(found("codigo", "CÓDIGO"), ["codigo"]);
    assert_eq!(found("Año, ano, AÑO", "año"), ["Año", "ano", "AÑO"]);
    assert_eq!(
        found("Façade à São Paulo", "facade a sao"),
        ["Façade à São"]
    );
    assert_eq!(found("Pingüino", "pinguino"), ["Pingüino"]);
}

#[test]
fn positions_are_characters_not_bytes() {
    // Emoji (4 bytes, 1 char) and ZWJ sequences before the match.
    let text = "👨‍👩‍👧 🇪🇸 mañana";
    let ranges = find_in_text(text, "manana");
    let prefix = "👨‍👩‍👧 🇪🇸 ".chars().count();
    assert_eq!(ranges, vec![prefix..prefix + 6]);
    assert_eq!(
        find_in_text("日本語のテキスト日本語", "日本語"),
        [0..3, 8..11]
    );
    assert_eq!(found("مرحبا שלום", "שלום"), ["שלום"]);
}

#[test]
fn decomposed_accents_match_and_stay_with_their_letter() {
    let decomposed = "cafe\u{301} cafe";
    assert_eq!(find_in_text(decomposed, "café"), [0..5, 6..10]);
    assert_eq!(
        find_in_text(decomposed, "cafe"),
        [0..5, 6..10],
        "the accent goes along"
    );
    // A query of only a combining accent finds nothing.
    assert!(find_in_text(decomposed, "\u{301}").is_empty());
}

#[test]
fn letters_that_lowercase_to_more_than_one_char() {
    // «İ» lowercases to «i» + a combining dot, which folds away.
    assert_eq!(found("İstanbul", "istanbul"), ["İstanbul"]);
    assert_eq!(found("STRASSE straße", "straße"), ["straße"]);
}

#[test]
fn matches_do_not_overlap_and_empty_finds_nothing() {
    assert_eq!(find_in_text("aaaa", "aa"), [0..2, 2..4]);
    assert_eq!(find_in_text("aaa", "aa"), vec![0..2]);
    assert!(find_in_text("algo", "").is_empty());
    assert!(find_in_text("", "a").is_empty());
    assert_eq!(find_in_text("a  b", " "), [1..2, 2..3], "spaces count");
    assert_eq!(
        found("uno\ndos", "o\nd"),
        ["o\nd"],
        "across lines of a block"
    );
}

#[test]
fn fold_keeps_its_old_behaviour() {
    assert_eq!(fold("Código Pequeño"), "codigo pequeno");
    assert_eq!(fold("ÁÉÍÓÚÜÑ"), "aeiouun");
    assert_eq!(fold("e\u{301}"), "e");
}

#[test]
fn blocks_are_searched_in_order_skipping_files_and_links() {
    let blocks = vec![
        b(BlockKind::Heading1, "Plan"),
        b(BlockKind::File, "plan.pdf"),
        b(BlockKind::PageLink, "plan"),
        b(BlockKind::Paragraph, "un plan y otro PLAN"),
        b(BlockKind::Code, "let plan = 1;"),
    ];
    let matches = find_in_blocks(&blocks, "plan");
    let at: Vec<(usize, usize)> = matches.iter().map(|m| (m.block, m.start)).collect();
    assert_eq!(at, [(0, 0), (3, 3), (3, 15), (4, 4)]);
}

#[test]
fn replace_one_or_all_keeps_the_rest_intact() {
    let mut blocks = vec![
        b(BlockKind::Paragraph, "Árbol, arbol y ÁRBOL 🌳"),
        b(BlockKind::Todo, "regar el árbol"),
    ];
    let matches = find_in_blocks(&blocks, "arbol");
    assert_eq!(matches.len(), 4);
    // One: the second.
    assert_eq!(replace_matches(&mut blocks, &matches[1..2], "pino"), 1);
    assert_eq!(blocks[0].content, "Árbol, pino y ÁRBOL 🌳");
    // All that are left, longer and shorter than the match.
    let matches = find_in_blocks(&blocks, "arbol");
    assert_eq!(replace_matches(&mut blocks, &matches, "roble grande"), 3);
    assert_eq!(blocks[0].content, "roble grande, pino y roble grande 🌳");
    assert_eq!(blocks[1].content, "regar el roble grande");
    // Replacing with nothing deletes; with the query itself, nothing changes.
    let matches = find_in_blocks(&blocks, "grande");
    assert_eq!(replace_matches(&mut blocks, &matches, ""), 3);
    assert_eq!(blocks[1].content, "regar el roble ");
}

#[test]
fn replace_skips_stale_matches_instead_of_panicking() {
    let mut blocks = vec![b(BlockKind::Paragraph, "corto")];
    let stale = [
        Match {
            block: 0,
            start: 3,
            end: 40,
        },
        Match {
            block: 7,
            start: 0,
            end: 1,
        },
        Match {
            block: 0,
            start: 2,
            end: 2,
        },
    ];
    assert_eq!(replace_matches(&mut blocks, &stale, "x"), 0);
    assert_eq!(blocks[0].content, "corto");
    // A decomposed accent is replaced with its letter.
    let mut blocks = vec![b(BlockKind::Paragraph, "cafe\u{301}!")];
    let matches = find_in_blocks(&blocks, "cafe");
    replace_matches(&mut blocks, &matches, "té");
    assert_eq!(blocks[0].content, "té!");
}

#[test]
fn the_first_match_shown_is_at_or_after_the_caret() {
    let blocks = vec![
        b(BlockKind::Paragraph, "uno uno"),
        b(BlockKind::Paragraph, "dos uno"),
    ];
    let matches = find_in_blocks(&blocks, "uno");
    assert_eq!(match_from(&matches, None), 0);
    assert_eq!(match_from(&matches, Some((0, 2))), 0, "inside the first");
    assert_eq!(match_from(&matches, Some((0, 3))), 0, "at its end");
    assert_eq!(match_from(&matches, Some((0, 5))), 1);
    assert_eq!(match_from(&matches, Some((1, 0))), 2);
    assert_eq!(
        match_from(&matches, Some((1, 9))),
        0,
        "after the last: wraps"
    );
    assert_eq!(match_from(&[], Some((0, 0))), 0);
}

/// The bar searches again every frame while it is open: on a long page
/// (1000 blocks, ~150 KB) that must stay well under a frame.
#[test]
#[ignore = "scale smoke test; run manually"]
fn find_scale_smoke() {
    use std::time::Instant;
    let text = "Párrafo de prueba con algo de texto, acentos y la palabra aguja. ";
    let blocks: Vec<Block> = (0..1000)
        .map(|_| b(BlockKind::Paragraph, &text.repeat(2)))
        .collect();
    let bytes: usize = blocks.iter().map(|b| b.content.len()).sum();
    let start = Instant::now();
    let mut found = 0;
    for _ in 0..20 {
        found = find_in_blocks(&blocks, "aguja").len();
    }
    let each = start.elapsed() / 20;
    eprintln!("{} KB, {found} matches: {each:?} per search", bytes / 1024);
    assert_eq!(found, 2000);
}
