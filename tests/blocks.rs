use silex_desktop::model::{Block, BlockKind, list_numbers};

fn block(kind: BlockKind, indent: u8) -> Block {
    Block {
        id: String::new(),
        page_id: "p".into(),
        kind,
        content: String::new(),
        checked: false,
        position: 0,
        indent,
        attachment: None,
        link: None,
    }
}

use BlockKind::{Numbered as N, Paragraph as P};

#[test]
fn consecutive_numbered_blocks_count_up() {
    let blocks = [block(N, 0), block(N, 0), block(N, 0)];
    assert_eq!(list_numbers(&blocks), [Some(1), Some(2), Some(3)]);
}

#[test]
fn other_block_at_same_level_restarts_the_list() {
    let blocks = [block(N, 0), block(N, 0), block(P, 0), block(N, 0)];
    assert_eq!(list_numbers(&blocks), [Some(1), Some(2), None, Some(1)]);
}

#[test]
fn nested_lists_count_separately_and_do_not_break_the_parent() {
    // 1. / a) a) / 2. — a deeper block does not end the outer list.
    let blocks = [
        block(N, 0),
        block(N, 1),
        block(N, 1),
        block(N, 0),
        block(N, 1),
    ];
    assert_eq!(
        list_numbers(&blocks),
        [Some(1), Some(1), Some(2), Some(2), Some(1)],
        "returning to level 0 restarts level 1"
    );
}

#[test]
fn deeper_non_numbered_block_keeps_outer_count() {
    let blocks = [block(N, 0), block(P, 1), block(N, 0)];
    assert_eq!(list_numbers(&blocks), [Some(1), None, Some(2)]);
}

#[test]
fn out_of_range_indent_is_clamped_not_a_panic() {
    let blocks = [block(N, 200), block(N, 200)];
    assert_eq!(list_numbers(&blocks), [Some(1), Some(2)]);
}

#[test]
fn kind_names_and_labels_are_unique() {
    let names: std::collections::HashSet<_> = BlockKind::ALL.iter().map(|k| k.as_str()).collect();
    let labels: std::collections::HashSet<_> = BlockKind::ALL.iter().map(|k| k.label()).collect();
    assert_eq!(names.len(), BlockKind::ALL.len());
    assert_eq!(labels.len(), BlockKind::ALL.len());
    assert!(BlockKind::Heading2.is_heading());
    assert!(!BlockKind::Quote.is_heading());
}
