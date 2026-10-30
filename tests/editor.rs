use silex_desktop::editor::{Caret, backspace_at_start, change_indent, delete_at_end, enter};
use silex_desktop::model::{Block, BlockKind, MAX_INDENT};

fn b(id: &str, kind: BlockKind, indent: u8, content: &str) -> Block {
    Block {
        id: id.into(),
        page_id: "p".into(),
        kind,
        content: content.into(),
        checked: kind == BlockKind::Todo,
        position: 0,
        indent,
        attachment: None,
        link: None,
        marks: Vec::new(),
    }
}

fn summary(blocks: &[Block]) -> Vec<(&str, BlockKind, u8, &str)> {
    blocks
        .iter()
        .map(|b| (b.id.as_str(), b.kind, b.indent, b.content.as_str()))
        .collect()
}

fn new_id() -> String {
    "nuevo".into()
}

use BlockKind::{Bulleted, Code, Heading1, Numbered, Paragraph, Quote, Todo};

mod enter_key {
    use super::*;

    #[test]
    fn splits_at_a_unicode_character_boundary() {
        let mut blocks = vec![b("a", Paragraph, 0, "día🙂mundo 日本")];
        let caret = enter(&mut blocks, 0, 4, new_id);
        assert_eq!(caret, Some(Caret { block: 1, char: 0 }));
        assert_eq!(
            summary(&blocks),
            [
                ("a", Paragraph, 0, "día🙂"),
                ("nuevo", Paragraph, 0, "mundo 日本")
            ]
        );
    }

    #[test]
    fn at_the_end_creates_an_empty_block_below() {
        let mut blocks = vec![b("a", Paragraph, 0, "fin")];
        assert_eq!(
            enter(&mut blocks, 0, 3, new_id),
            Some(Caret { block: 1, char: 0 })
        );
        assert_eq!(blocks[1].content, "");
    }

    #[test]
    fn cursor_past_the_end_is_clamped() {
        let mut blocks = vec![b("a", Paragraph, 0, "ñu")];
        enter(&mut blocks, 0, 99, new_id);
        assert_eq!(summary(&blocks)[0].3, "ñu");
        assert_eq!(blocks[1].content, "");
    }

    #[test]
    fn lists_and_tasks_continue_with_same_kind_and_indent() {
        for kind in [Bulleted, Numbered, Todo] {
            let mut blocks = vec![b("a", kind, 2, "uno dos")];
            enter(&mut blocks, 0, 3, new_id);
            assert_eq!(blocks[1].kind, kind);
            assert_eq!(blocks[1].indent, 2);
            assert_eq!(blocks[1].content, " dos");
            assert!(!blocks[1].checked, "a new task starts unchecked");
        }
    }

    #[test]
    fn headings_and_quotes_continue_as_text() {
        for kind in [Heading1, Quote] {
            let mut blocks = vec![b("a", kind, 0, "Título")];
            enter(&mut blocks, 0, 6, new_id);
            assert_eq!(blocks[0].kind, kind);
            assert_eq!(blocks[1].kind, Paragraph);
        }
    }

    #[test]
    fn at_start_of_non_empty_block_inserts_above_and_keeps_cursor() {
        let mut blocks = vec![b("a", Heading1, 0, "Capítulo")];
        let caret = enter(&mut blocks, 0, 0, new_id);
        assert_eq!(caret, Some(Caret { block: 1, char: 0 }));
        assert_eq!(
            summary(&blocks),
            [("nuevo", Paragraph, 0, ""), ("a", Heading1, 0, "Capítulo")]
        );
    }

    #[test]
    fn empty_list_item_outdents_then_becomes_text() {
        let mut blocks = vec![b("a", Bulleted, 1, "")];
        assert_eq!(
            enter(&mut blocks, 0, 0, new_id),
            Some(Caret { block: 0, char: 0 })
        );
        assert_eq!(summary(&blocks), [("a", Bulleted, 0, "")]);
        enter(&mut blocks, 0, 0, new_id);
        assert_eq!(summary(&blocks), [("a", Paragraph, 0, "")]);
        assert_eq!(blocks.len(), 1, "no block was added");
    }

    #[test]
    fn empty_paragraph_adds_a_new_block() {
        let mut blocks = vec![b("a", Paragraph, 0, "")];
        enter(&mut blocks, 0, 0, new_id);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn out_of_range_block_does_nothing() {
        let mut blocks = vec![b("a", Paragraph, 0, "x")];
        assert_eq!(enter(&mut blocks, 5, 0, new_id), None);
        assert_eq!(blocks.len(), 1);
    }
}

mod backspace_key {
    use super::*;

    #[test]
    fn styled_block_becomes_text_first() {
        let mut blocks = vec![b("a", Paragraph, 0, "x"), b("b", Todo, 1, "tarea")];
        assert_eq!(
            backspace_at_start(&mut blocks, 1),
            Some(Caret { block: 1, char: 0 })
        );
        assert_eq!(summary(&blocks)[1], ("b", Paragraph, 1, "tarea"));
        assert!(!blocks[1].checked);
    }

    #[test]
    fn indented_text_is_outdented_next() {
        let mut blocks = vec![b("a", Paragraph, 0, "x"), b("b", Paragraph, 2, "y")];
        backspace_at_start(&mut blocks, 1);
        assert_eq!(blocks[1].indent, 1);
    }

    #[test]
    fn plain_text_merges_into_previous_with_cursor_at_the_seam() {
        let mut blocks = vec![
            b("a", Heading1, 0, "Café ☕"),
            b("b", Paragraph, 0, " y más"),
        ];
        let caret = backspace_at_start(&mut blocks, 1);
        assert_eq!(caret, Some(Caret { block: 0, char: 6 }), "chars, not bytes");
        assert_eq!(summary(&blocks), [("a", Heading1, 0, "Café ☕ y más")]);
    }

    #[test]
    fn first_plain_block_does_nothing() {
        let mut blocks = vec![b("a", Paragraph, 0, "solo")];
        assert_eq!(backspace_at_start(&mut blocks, 0), None);
        assert_eq!(blocks.len(), 1);
    }
}

mod delete_key {
    use super::*;

    #[test]
    fn pulls_next_block_into_this_one() {
        let mut blocks = vec![b("a", Paragraph, 0, "日本"), b("b", Code, 0, "語")];
        assert_eq!(
            delete_at_end(&mut blocks, 0),
            Some(Caret { block: 0, char: 2 })
        );
        assert_eq!(summary(&blocks), [("a", Paragraph, 0, "日本語")]);
    }

    #[test]
    fn last_block_does_nothing() {
        let mut blocks = vec![b("a", Paragraph, 0, "fin")];
        assert_eq!(delete_at_end(&mut blocks, 0), None);
    }
}

mod tab_key {
    use super::*;

    #[test]
    fn indent_is_limited_to_one_level_below_the_block_above() {
        let mut blocks = vec![b("a", Bulleted, 0, "x"), b("b", Bulleted, 0, "y")];
        assert!(change_indent(&mut blocks, 1, 1));
        assert_eq!(blocks[1].indent, 1);
        assert!(!change_indent(&mut blocks, 1, 1), "already one deeper");
        assert_eq!(blocks[1].indent, 1);
    }

    #[test]
    fn first_block_cannot_be_indented() {
        let mut blocks = vec![b("a", Bulleted, 0, "x")];
        assert!(!change_indent(&mut blocks, 0, 1));
    }

    #[test]
    fn shift_tab_outdents_down_to_zero() {
        let mut blocks = vec![b("a", Paragraph, 0, "x"), b("b", Paragraph, 1, "y")];
        assert!(change_indent(&mut blocks, 1, -1));
        assert!(!change_indent(&mut blocks, 1, -1));
        assert_eq!(blocks[1].indent, 0);
    }

    #[test]
    fn never_deeper_than_max_indent() {
        let mut blocks = vec![
            b("a", Bulleted, MAX_INDENT, "x"),
            b("b", Bulleted, MAX_INDENT, "y"),
        ];
        assert!(!change_indent(&mut blocks, 1, 1));
        assert_eq!(blocks[1].indent, MAX_INDENT);
    }
}

mod slash {
    use super::*;
    use silex_desktop::editor::{apply_slash, slash_matches, slash_query};

    #[test]
    fn query_is_the_word_after_a_leading_slash_up_to_the_cursor() {
        assert_eq!(slash_query("/", 1), Some(""));
        assert_eq!(slash_query("/tit", 4), Some("tit"));
        assert_eq!(slash_query("/código", 7), Some("código"), "char cursor");
        assert_eq!(slash_query("/tit", 2), None, "cursor inside the word");
        assert_eq!(slash_query("/h1 hola", 8), None, "a space ends it");
        assert_eq!(slash_query("hola /h1", 8), None, "only at the start");
        assert_eq!(slash_query("/x", 0), None);
        assert_eq!(slash_query("", 0), None);
    }

    #[test]
    fn matches_by_label_alias_and_without_accents() {
        assert_eq!(slash_matches(""), BlockKind::ALL.to_vec());
        assert_eq!(slash_matches("h2"), [BlockKind::Heading2]);
        assert_eq!(slash_matches("codigo"), [BlockKind::Code]);
        assert_eq!(slash_matches("CÓDIGO"), [BlockKind::Code]);
        assert_eq!(slash_matches("tarea"), [BlockKind::Todo]);
        assert_eq!(
            slash_matches("titulo"),
            [
                BlockKind::Heading1,
                BlockKind::Heading2,
                BlockKind::Heading3
            ]
        );
        assert!(slash_matches("zzz").is_empty());
    }

    #[test]
    fn applying_removes_the_command_and_changes_the_type() {
        let mut blocks = vec![b("a", Todo, 1, "/h1resto")];
        let caret = apply_slash(&mut blocks, 0, 3, Heading1);
        assert_eq!(caret, Some(Caret { block: 0, char: 0 }));
        assert_eq!(summary(&blocks), [("a", Heading1, 1, "resto")]);
        assert!(!blocks[0].checked);
    }
}

mod selection {
    use super::*;
    use silex_desktop::editor::{delete_range, restore_deleted, selection_text};

    fn doc() -> Vec<Block> {
        vec![
            b("a", Heading1, 0, "Compras"),
            b("b", Bulleted, 0, "Pan"),
            b("c", Bulleted, 1, "Integral"),
            b("d", Numbered, 0, "Uno"),
            b("e", Numbered, 0, "Dos"),
            b("f", Todo, 0, "Pagar"),
        ]
    }

    #[test]
    fn delete_and_restore_round_trip() {
        let original = doc();
        let mut blocks = original.clone();
        let deleted = delete_range(&mut blocks, 3, 1); // reversed order is fine
        assert_eq!(
            summary(&blocks)
                .iter()
                .map(|(id, ..)| *id)
                .collect::<Vec<_>>(),
            ["a", "e", "f"]
        );
        restore_deleted(&mut blocks, deleted);
        assert_eq!(blocks, original);
    }

    #[test]
    fn delete_out_of_range_is_clamped_or_empty() {
        let mut blocks = doc();
        assert!(delete_range(&mut blocks, 10, 12).removed.is_empty());
        let deleted = delete_range(&mut blocks, 4, 99);
        assert_eq!(deleted.removed.len(), 2);
        assert_eq!(blocks.len(), 4);
    }

    #[test]
    fn copied_text_keeps_indent_markers_and_numbers() {
        assert_eq!(
            selection_text(&doc(), 0, 5),
            "Compras\n• Pan\n  • Integral\n1. Uno\n2. Dos\n[x] Pagar"
        );
        assert_eq!(
            selection_text(&doc(), 4, 4),
            "2. Dos",
            "number from context"
        );
    }
}

mod move_and_duplicate {
    use super::*;
    use silex_desktop::editor::{duplicate_range, move_range};

    fn ids(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|b| b.id.as_str()).collect()
    }

    fn doc() -> Vec<Block> {
        ["a", "b", "c", "d"]
            .iter()
            .map(|id| b(id, Paragraph, 0, id))
            .collect()
    }

    #[test]
    fn single_block_moves_and_stops_at_the_edges() {
        let mut blocks = doc();
        assert!(move_range(&mut blocks, 1, 1, true));
        assert_eq!(ids(&blocks), ["b", "a", "c", "d"]);
        assert!(!move_range(&mut blocks, 0, 0, true));
        assert!(move_range(&mut blocks, 0, 0, false));
        assert_eq!(ids(&blocks), ["a", "b", "c", "d"]);
        assert!(!move_range(&mut blocks, 3, 3, false));
    }

    #[test]
    fn a_range_moves_as_a_unit_keeping_indent() {
        let mut blocks = doc();
        blocks[2].indent = 1;
        assert!(move_range(&mut blocks, 2, 1, false));
        assert_eq!(ids(&blocks), ["a", "d", "b", "c"]);
        assert_eq!(blocks[3].indent, 1);
        assert!(move_range(&mut blocks, 2, 3, true));
        assert_eq!(ids(&blocks), ["a", "b", "c", "d"]);
        assert!(!move_range(&mut blocks, 2, 9, true), "out of range");
    }

    #[test]
    fn duplicate_inserts_copies_with_new_ids_after_the_range() {
        let mut blocks = doc();
        let mut n = 0;
        let range = duplicate_range(&mut blocks, 1, 2, || {
            n += 1;
            format!("copia{n}")
        });
        assert_eq!(range, Some((3, 4)));
        assert_eq!(ids(&blocks), ["a", "b", "c", "copia1", "copia2", "d"]);
        assert_eq!(blocks[3].content, "b");
        assert_eq!(duplicate_range(&mut blocks, 9, 9, || "x".into()), None);
    }
}

mod paste {
    use super::*;
    use silex_desktop::editor::{normalize_pasted, parse_pasted_line, paste_lines, selection_text};

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("p{n}")
        }
    }

    #[test]
    fn multi_line_paste_splits_around_the_cursor() {
        let mut blocks = vec![b("a", Paragraph, 0, "Hola mundo")];
        let caret = paste_lines(&mut blocks, 0, 5, "uno\ndos\ntres ", counter());
        assert_eq!(
            summary(&blocks),
            [
                ("a", Paragraph, 0, "Hola uno"),
                ("p1", Paragraph, 0, "dos"),
                ("p2", Paragraph, 0, "tres mundo"),
            ]
        );
        assert_eq!(
            caret,
            Some(Caret { block: 2, char: 5 }),
            "end of pasted text"
        );
    }

    #[test]
    fn line_endings_and_control_characters_are_normalized() {
        assert_eq!(normalize_pasted("a\r\nb\rc\u{0}d\u{7}\te"), "a\nb\ncd\te");
        let mut blocks = vec![b("a", Paragraph, 0, "")];
        paste_lines(&mut blocks, 0, 0, "x\r\ny", counter());
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].content, "y");
    }

    #[test]
    fn silex_copy_format_is_recognized() {
        assert_eq!(
            parse_pasted_line("  • Integral"),
            silex_desktop::editor::PastedLine {
                kind: Bulleted,
                indent: 1,
                checked: false,
                content: "Integral".into()
            }
        );
        assert_eq!(parse_pasted_line("12. Paso").kind, Numbered);
        assert!(parse_pasted_line("[x] Hecho").checked);
        assert_eq!(parse_pasted_line("[ ] Por hacer").kind, Todo);
        let plain = parse_pasted_line("  sangrado normal");
        assert_eq!((plain.kind, plain.indent), (Paragraph, 0));
        assert_eq!(
            plain.content, "  sangrado normal",
            "plain text is kept as is"
        );
        assert_eq!(parse_pasted_line("1.5 litros").kind, Paragraph);
    }

    #[test]
    fn copy_then_paste_rebuilds_the_blocks() {
        let original = vec![
            b("a", Bulleted, 0, "Pan"),
            b("b", Bulleted, 1, "Integral"),
            b("c", Numbered, 0, "Uno"),
            b("d", Numbered, 0, "Dos"),
            b("e", Todo, 0, "Pagar"),
            b("f", Paragraph, 0, "Fin"),
        ];
        let copied = selection_text(&original, 0, 5);
        let mut blocks = vec![b("x", Paragraph, 0, "")];
        paste_lines(&mut blocks, 0, 0, &copied, counter());
        let rebuilt: Vec<_> = blocks
            .iter()
            .map(|b| (b.kind, b.indent, b.checked, b.content.as_str()))
            .collect();
        let expected: Vec<_> = original
            .iter()
            .map(|b| (b.kind, b.indent, b.checked, b.content.as_str()))
            .collect();
        assert_eq!(rebuilt, expected);
    }

    #[test]
    fn pasted_structure_is_relative_to_the_block_indent() {
        let mut blocks = vec![b("a", Bulleted, 2, "x")];
        paste_lines(&mut blocks, 0, 1, "\n  • hijo", counter());
        assert_eq!(blocks[1].indent, 3);
    }
}

/// Integrity: splitting at ANY character position and merging back returns
/// the exact original text, for scripts and sequences that are easy to break.
mod unicode_integrity {
    use super::*;
    use silex_desktop::editor::{backspace_at_start, delete_at_end, paste_lines};

    const TRICKY: &[&str] = &[
        "año ñandú",
        "e\u{301}cole",     // e + combining acute accent
        "👨‍👩‍👧‍👦 familia",       // ZWJ sequence
        "🇪🇸🇯🇵 banderas",    // regional indicator pairs
        "مرحبا بالعالم",    // Arabic (RTL)
        "שלום עולם",        // Hebrew (RTL)
        "日本語のテキスト", // CJK
        "tab\tbetween",
        "\u{200B}zero\u{200D}width", // zero-width space / joiner
        "",
    ];

    #[test]
    fn enter_then_backspace_round_trips_at_every_position() {
        for text in TRICKY {
            for cursor in 0..=text.chars().count() {
                let mut blocks = vec![b("a", Paragraph, 0, text)];
                let caret = enter(&mut blocks, 0, cursor, || "n".into()).expect("split");
                let joined: String = blocks.iter().map(|b| b.content.as_str()).collect();
                assert_eq!(joined, *text, "split loses nothing ({text:?} @ {cursor})");
                backspace_at_start(&mut blocks, caret.block);
                let back: String = blocks.iter().map(|b| b.content.as_str()).collect();
                assert_eq!(back, *text, "{text:?} @ {cursor}");
                assert_eq!(blocks.len(), 1, "{text:?} @ {cursor}");
            }
        }
    }

    #[test]
    fn delete_at_end_and_paste_preserve_every_character() {
        for text in TRICKY {
            let (head, tail): (String, String) = {
                let mid = text.chars().count() / 2;
                (
                    text.chars().take(mid).collect(),
                    text.chars().skip(mid).collect(),
                )
            };
            let mut blocks = vec![b("a", Paragraph, 0, &head), b("b", Paragraph, 0, &tail)];
            delete_at_end(&mut blocks, 0);
            assert_eq!(blocks[0].content, *text);

            let mut blocks = vec![b("a", Paragraph, 0, "[]")];
            paste_lines(&mut blocks, 0, 1, &format!("{text}\n{text}"), || "n".into());
            let joined: String = blocks.iter().map(|b| b.content.as_str()).collect();
            assert_eq!(joined, format!("[{text}{text}]"), "{text:?}");
        }
    }
}

// ---- File blocks (ADR 0007) ----

use silex_desktop::editor::{apply_slash, duplicate_range, selection_text};

fn file(id: &str) -> silex_desktop::model::Block {
    silex_desktop::model::Block {
        kind: BlockKind::File,
        attachment: Some(format!("adjunto-{id}")),
        ..b(id, BlockKind::Paragraph, 0, "informe.pdf")
    }
}

#[test]
fn text_never_merges_with_a_file_block() {
    let mut blocks = vec![
        b("a", BlockKind::Paragraph, 0, "antes"),
        file("f"),
        b("c", BlockKind::Paragraph, 0, "después"),
    ];
    let before = blocks.clone();
    // Backspace at the start of the text below a file: nothing.
    assert_eq!(backspace_at_start(&mut blocks, 2), None);
    // Delete at the end of the text above a file: nothing.
    assert_eq!(delete_at_end(&mut blocks, 0), None);
    // On the file block itself: nothing either.
    assert_eq!(backspace_at_start(&mut blocks, 1), None);
    assert_eq!(delete_at_end(&mut blocks, 1), None);
    assert_eq!(enter(&mut blocks, 1, 0, || "nuevo".into()), None);
    assert_eq!(apply_slash(&mut blocks, 1, 0, BlockKind::Heading1), None);
    assert_eq!(apply_slash(&mut blocks, 0, 0, BlockKind::File), None);
    assert_eq!(blocks, before);

    // The steps before merging still apply: a heading below a file becomes text.
    blocks[2].kind = BlockKind::Heading2;
    assert!(backspace_at_start(&mut blocks, 2).is_some());
    assert_eq!(blocks[2].kind, BlockKind::Paragraph);
}

#[test]
fn a_duplicated_file_block_shares_its_attachment() {
    let mut blocks = vec![file("f")];
    let mut n = 0;
    duplicate_range(&mut blocks, 0, 0, || {
        n += 1;
        format!("copia-{n}")
    });
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[1].kind, BlockKind::File);
    assert_eq!(blocks[1].attachment, blocks[0].attachment);
    assert_ne!(blocks[1].id, blocks[0].id);
    // Copied as text, it is its name.
    assert_eq!(selection_text(&blocks, 0, 0), "informe.pdf");
}

// ---- Slash menu entries (H7.3) ----

#[test]
fn the_slash_menu_offers_attaching_files() {
    use silex_desktop::editor::{SlashItem, slash_items};
    let all = slash_items("");
    assert_eq!(all.len(), BlockKind::ALL.len() + 2);
    assert_eq!(
        all[BlockKind::ALL.len()..],
        [SlashItem::LinkPage, SlashItem::AttachFiles]
    );
    for query in ["enlace", "pag", "LINK", "vinc"] {
        assert_eq!(slash_items(query), [SlashItem::LinkPage], "{query}");
    }
    for query in ["arch", "ARCHIVO", "adj", "imagen", "pdf", "foto"] {
        assert_eq!(slash_items(query), [SlashItem::AttachFiles], "{query}");
    }
    // «Título 1» and the like are not drowned by it.
    assert_eq!(slash_items("h1"), [SlashItem::Kind(BlockKind::Heading1)]);
    assert!(slash_items("zzz").is_empty());
}

#[test]
fn text_never_merges_with_a_link_block() {
    let mut blocks = vec![
        b("a", BlockKind::Paragraph, 0, "antes"),
        silex_desktop::model::Block {
            kind: BlockKind::PageLink,
            link: Some("otra".into()),
            ..b("l", BlockKind::Paragraph, 0, "Otra página")
        },
        b("c", BlockKind::Paragraph, 0, "después"),
    ];
    let before = blocks.clone();
    assert_eq!(backspace_at_start(&mut blocks, 2), None);
    assert_eq!(delete_at_end(&mut blocks, 0), None);
    assert_eq!(enter(&mut blocks, 1, 0, || "x".into()), None);
    assert_eq!(apply_slash(&mut blocks, 1, 0, BlockKind::Heading1), None);
    assert_eq!(blocks, before);
}

#[test]
fn dropping_blocks_moves_them_to_the_gap() {
    use silex_desktop::editor::move_range_to;
    let ids = |blocks: &[Block]| blocks.iter().map(|b| b.id.clone()).collect::<String>();
    let start = || -> Vec<Block> {
        ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| b(id, Paragraph, 0, id))
            .collect()
    };
    // One block, down and up, to the ends too.
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 1, 1, 4), Some((3, 3)));
    assert_eq!(ids(&blocks), "acdbe");
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 3, 3, 0), Some((0, 0)));
    assert_eq!(ids(&blocks), "dabce");
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 0, 0, 5), Some((4, 4)));
    assert_eq!(ids(&blocks), "bcdea");
    // A selection keeps its order, given either way round.
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 2, 1, 5), Some((3, 4)));
    assert_eq!(ids(&blocks), "adebc");
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 3, 4, 1), Some((1, 2)));
    assert_eq!(ids(&blocks), "adebc");
    // Dropped on itself (either edge of the range, or inside): nothing.
    for to in [1, 2, 3, 4] {
        let mut blocks = start();
        assert_eq!(move_range_to(&mut blocks, 1, 3, to), None, "{to}");
        assert_eq!(ids(&blocks), "abcde");
    }
    // Out of bounds.
    let mut blocks = start();
    assert_eq!(move_range_to(&mut blocks, 4, 5, 0), None);
    assert_eq!(move_range_to(&mut blocks, 0, 0, 6), None);
    // Indents and kinds travel with the blocks.
    let mut blocks = vec![b("a", Todo, 1, "a"), b("b", Paragraph, 0, "b")];
    move_range_to(&mut blocks, 0, 0, 2);
    assert_eq!(
        summary(&blocks),
        [("b", Paragraph, 0, "b"), ("a", Todo, 1, "a")]
    );
}

/// Inline formatting follows the text through block operations (ADR 0015).
mod formatting {
    use super::*;
    use silex_desktop::editor::{apply_slash, paste_lines};
    use silex_desktop::marks::{Mark, MarkKind};

    fn bold(start: usize, end: usize) -> Mark {
        Mark::new(start, end, MarkKind::Bold)
    }

    fn with_marks(mut block: Block, marks: Vec<Mark>) -> Block {
        block.marks = marks;
        block
    }

    #[test]
    fn delete_at_the_end_joins_the_formatting() {
        let mut blocks = vec![
            with_marks(b("a", Paragraph, 0, "uno"), vec![bold(0, 3)]),
            with_marks(b("b", Paragraph, 0, "dos"), vec![bold(1, 3)]),
        ];
        delete_at_end(&mut blocks, 0);
        assert_eq!(blocks[0].content, "unodos");
        assert_eq!(blocks[0].marks, vec![bold(0, 3), bold(4, 6)]);
    }

    #[test]
    fn joining_into_a_code_block_drops_the_formatting() {
        let mut blocks = vec![
            b("a", Code, 0, "let x;"),
            with_marks(b("b", Paragraph, 0, "texto"), vec![bold(0, 5)]),
        ];
        backspace_at_start(&mut blocks, 1);
        assert_eq!(blocks[0].content, "let x;texto");
        assert!(blocks[0].marks.is_empty());
    }

    #[test]
    fn enter_in_a_list_gives_each_half_its_formatting() {
        let mut blocks = vec![with_marks(
            b("a", Bulleted, 0, "pan🌾 rico"),
            vec![bold(0, 4)],
        )];
        enter(&mut blocks, 0, 2, new_id);
        assert_eq!(blocks[0].marks, vec![bold(0, 2)]);
        assert_eq!(blocks[1].kind, Bulleted);
        assert_eq!(blocks[1].marks, vec![bold(0, 2)]);
        assert_eq!(blocks[1].content, "n🌾 rico");
    }

    #[test]
    fn a_slash_command_removes_its_text_and_moves_the_formatting() {
        // «/tit» typed before bold «hola».
        let mut blocks = vec![with_marks(
            b("a", Paragraph, 0, "/tithola"),
            vec![bold(4, 8)],
        )];
        apply_slash(&mut blocks, 0, 4, Heading1);
        assert_eq!(blocks[0].content, "hola");
        assert_eq!(blocks[0].marks, vec![bold(0, 4)]);
        apply_slash(&mut blocks, 0, 0, Code);
        assert!(blocks[0].marks.is_empty(), "code takes no formatting");
    }

    #[test]
    fn pasted_lines_are_plain_and_the_tail_keeps_its_formatting() {
        // Caret between bold «ab» and bold «cd».
        let mut blocks = vec![with_marks(b("a", Paragraph, 0, "abcd"), vec![bold(0, 4)])];
        let mut n = 0;
        paste_lines(&mut blocks, 0, 2, "uno\ndos", || {
            n += 1;
            format!("n{n}")
        });
        assert_eq!(
            summary(&blocks),
            [("a", Paragraph, 0, "abuno"), ("n1", Paragraph, 0, "doscd")]
        );
        assert_eq!(blocks[0].marks, vec![bold(0, 2)]);
        assert_eq!(blocks[1].marks, vec![bold(3, 5)]);
    }
}

/// H12.1e: copying keeps the formatting with the text, and pasting that
/// same text gives it back (ADR 0015).
mod clipboard {
    use super::*;
    use silex_desktop::editor::{Clip, paste_lines_marked, selection_clip, selection_text};
    use silex_desktop::marks::{Mark, MarkKind};

    fn bold(start: usize, end: usize) -> Mark {
        Mark::new(start, end, MarkKind::Bold)
    }

    fn italic(start: usize, end: usize) -> Mark {
        Mark::new(start, end, MarkKind::Italic)
    }

    fn marked(mut block: Block, marks: Vec<Mark>) -> Block {
        block.marks = marks;
        block
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("n{n}")
        }
    }

    fn copied_blocks() -> Vec<Block> {
        vec![
            marked(b("a", Bulleted, 0, "pan negro"), vec![bold(0, 3)]),
            marked(b("b", Numbered, 1, "dos ñu"), vec![italic(4, 6)]),
            marked(b("c", Paragraph, 0, "🌾 fin"), vec![bold(2, 5)]),
        ]
    }

    #[test]
    fn a_selection_of_blocks_copies_its_formatting_after_the_markers() {
        let clip = selection_clip(&copied_blocks(), 0, 2);
        assert_eq!(clip.text, selection_text(&copied_blocks(), 0, 2));
        assert_eq!(clip.text, "• pan negro\n  1. dos ñu\n🌾 fin");
        let words: Vec<String> = clip
            .marks
            .iter()
            .map(|m| {
                clip.text
                    .chars()
                    .skip(m.start)
                    .take(m.end - m.start)
                    .collect()
            })
            .collect();
        assert_eq!(words, ["pan", "ñu", "fin"]);
    }

    #[test]
    fn pasting_copied_blocks_brings_back_their_formatting() {
        let original = copied_blocks();
        let clip = selection_clip(&original, 0, 2);
        let mut blocks = vec![marked(b("x", Paragraph, 0, ""), vec![])];
        let marks = clip.marks_for(&clip.text).expect("same text");
        paste_lines_marked(&mut blocks, 0, 0, &clip.text, marks, ids());
        let got: Vec<_> = blocks
            .iter()
            .map(|b| (b.kind, b.content.as_str(), b.marks.clone()))
            .collect();
        let want: Vec<_> = original
            .iter()
            .map(|b| (b.kind, b.content.as_str(), b.marks.clone()))
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn pasting_in_the_middle_keeps_the_text_around_and_its_formatting() {
        let clip = Clip {
            text: "uno\ndos".into(),
            marks: vec![bold(1, 6)],
        };
        let mut blocks = vec![marked(b("a", Paragraph, 0, "ab"), vec![italic(0, 2)])];
        // Windows line endings are the same text.
        let marks = clip.marks_for("uno\r\ndos").expect("same text");
        let caret = paste_lines_marked(&mut blocks, 0, 1, "uno\r\ndos", marks, ids());
        assert_eq!(caret, Some(Caret { block: 1, char: 3 }));
        assert_eq!(blocks[0].content, "auno");
        assert_eq!(blocks[0].marks, vec![italic(0, 1), bold(2, 4)]);
        assert_eq!(blocks[1].content, "dosb");
        assert_eq!(blocks[1].marks, vec![bold(0, 2), italic(3, 4)]);
        assert!(clip.marks_for("otro texto").is_none());
    }

    #[test]
    fn a_code_block_or_a_control_character_takes_no_formatting() {
        let clip = Clip {
            text: "uno\ndos".into(),
            marks: vec![bold(0, 7)],
        };
        let mut blocks = vec![b("a", Code, 0, "")];
        paste_lines_marked(&mut blocks, 0, 0, &clip.text, &clip.marks, ids());
        assert!(blocks[0].marks.is_empty());
        let block = b("a", Paragraph, 0, "a\u{7}b");
        assert!(Clip::of_text(&block, 0, 3).is_none());
        let block = marked(b("a", Paragraph, 0, "pan negro"), vec![bold(0, 3)]);
        assert_eq!(
            Clip::of_text(&block, 2, 6),
            Some(Clip {
                text: "n ne".into(),
                marks: vec![bold(0, 1)]
            })
        );
    }
}
