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
