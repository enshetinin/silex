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
