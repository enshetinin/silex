use silex_desktop::model::{Block, BlockKind, word_count};

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

#[test]
fn counts_words_across_text_blocks() {
    let blocks = [
        b(BlockKind::Heading1, "Plan de lanzamiento"),
        b(
            BlockKind::Paragraph,
            "Una  primera versión,\npequeña y sólida.",
        ),
        b(BlockKind::Todo, "Probar deshacer"),
    ];
    assert_eq!(word_count(&blocks), 3 + 6 + 2);
}

#[test]
fn ignores_marks_cards_and_empty_blocks() {
    let blocks = [
        b(BlockKind::Paragraph, "— · 🙂 … 42"),
        b(BlockKind::Paragraph, "   "),
        b(BlockKind::File, "informe.pdf"),
        b(BlockKind::PageLink, "Otra página"),
    ];
    assert_eq!(word_count(&blocks), 1);
    assert_eq!(word_count(&[]), 0);
}
