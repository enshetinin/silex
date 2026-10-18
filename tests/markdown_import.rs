//! Import: golden files from other apps, round trips and stability.
use silex_desktop::markdown::{ImportedPage, markdown_to_page, page_to_markdown};
use silex_desktop::model::{BlockKind, NewBlock};

/// One line per block: `kind indent [x]| content` (newlines as ⏎).
fn dump(page: &ImportedPage) -> String {
    let mut out = format!("título: {}\n", page.title);
    for b in &page.blocks {
        let check = match (b.kind, b.checked) {
            (BlockKind::Todo, true) => " [x]",
            (BlockKind::Todo, false) => " [ ]",
            _ => "",
        };
        out.push_str(&format!(
            "{} {}{}| {}\n",
            b.kind.as_str(),
            b.indent,
            check,
            b.content.replace('\n', "⏎")
        ));
    }
    out
}

const FIXTURES: &[(&str, &str)] = &[
    ("obsidian", include_str!("golden/import/obsidian.md")),
    ("github", include_str!("golden/import/github.md")),
    ("edge", include_str!("golden/import/edge.md")),
];

/// Golden: each fixture's blocks must match `<name>.expected` (reviewed by hand).
#[test]
fn fixtures_match_their_golden_dumps() {
    let expected = [
        include_str!("golden/import/obsidian.expected"),
        include_str!("golden/import/github.expected"),
        include_str!("golden/import/edge.expected"),
    ];
    for ((name, source), expected) in FIXTURES.iter().zip(expected) {
        let actual = dump(&markdown_to_page(source, name));
        if actual != expected {
            let out = std::env::temp_dir().join(format!("silex_import_{name}.actual"));
            let _ = std::fs::write(&out, &actual);
            panic!(
                "{name}: import changed; actual written to {}",
                out.display()
            );
        }
    }
}

/// Exporting what was imported, then importing and exporting again, gives
/// the same file: one round trip normalises, after that nothing drifts.
#[test]
fn export_after_import_is_stable() {
    for (name, source) in FIXTURES {
        let first = markdown_to_page(source, name);
        let md1 = page_to_markdown(&first.title, &to_blocks(&first.blocks));
        let second = markdown_to_page(&md1, "otro");
        assert_eq!(second, first, "{name}: blocks survive the round trip");
        let md2 = page_to_markdown(&second.title, &to_blocks(&second.blocks));
        assert_eq!(md2, md1, "{name}: export is stable");
    }
}

fn to_blocks(blocks: &[NewBlock]) -> Vec<silex_desktop::model::Block> {
    blocks
        .iter()
        .map(|b| silex_desktop::model::Block {
            id: String::new(),
            page_id: String::new(),
            kind: b.kind,
            content: b.content.clone(),
            checked: b.checked,
            position: 0,
            indent: b.indent,
        })
        .collect()
}

fn nb(kind: BlockKind, indent: u8, checked: bool, content: &str) -> NewBlock {
    NewBlock {
        kind,
        indent,
        checked,
        content: content.into(),
    }
}

/// Silex → Markdown → Silex gives back exactly the same blocks.
#[test]
fn silex_blocks_round_trip_exactly() {
    use BlockKind::*;
    let blocks = vec![
        nb(Heading2, 0, false, "Sección *con* #símbolos"),
        nb(Paragraph, 0, false, "  espacios al inicio y fin  "),
        nb(Paragraph, 0, false, "dos\nlíneas\n\ny una vacía"),
        nb(Bulleted, 0, false, "nivel 0"),
        nb(Bulleted, 1, false, "nivel 1"),
        nb(Numbered, 2, false, "nivel 2 numerado"),
        nb(Todo, 3, true, "tarea profunda"),
        nb(Bulleted, 0, false, ""),
        nb(Numbered, 0, false, "1. parece lista"),
        nb(Paragraph, 1, false, "párrafo dentro del elemento"),
        nb(Quote, 0, false, "> cita\ncon # cosas"),
        nb(Code, 0, false, "```\n  sangrado\n\tTab"),
        nb(Code, 0, false, ""),
        nb(Heading3, 0, false, ""),
        nb(Paragraph, 0, false, "日本語 🙂 e\u{301} مرحبا"),
    ];
    let markdown = page_to_markdown("Título: *raro* #1", &to_blocks(&blocks));
    let back = markdown_to_page(&markdown, "x");
    assert_eq!(back.title, "Título: *raro* #1");
    assert_eq!(back.blocks, blocks, "markdown was:\n{markdown}");
}

#[test]
fn every_ascii_punctuation_round_trips_in_every_text_kind() {
    use BlockKind::*;
    let punctuation: String = (33u8..127)
        .map(char::from)
        .filter(char::is_ascii_punctuation)
        .collect();
    for kind in [
        Paragraph, Heading1, Heading2, Heading3, Bulleted, Numbered, Todo, Quote, Code,
    ] {
        for text in [
            punctuation.clone(),
            format!(" {punctuation} x {punctuation} "),
        ] {
            let blocks = vec![nb(Paragraph, 0, false, "antes"), nb(kind, 0, false, &text)];
            let markdown = page_to_markdown("t", &to_blocks(&blocks));
            let back = markdown_to_page(&markdown, "x");
            assert_eq!(back.blocks, blocks, "{kind:?}:\n{markdown}");
        }
    }
}

#[test]
fn title_comes_from_a_leading_h1_or_the_file_name() {
    assert_eq!(markdown_to_page("# Hola\n\ntexto", "archivo").title, "Hola");
    let page = markdown_to_page("texto\n\n# Luego", "archivo");
    assert_eq!(page.title, "archivo");
    assert_eq!(
        page.blocks[1].kind,
        BlockKind::Heading1,
        "later H1 stays a block"
    );
    assert_eq!(markdown_to_page("", "vacío").blocks, []);
}
