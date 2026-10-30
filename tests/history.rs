use std::time::{Duration, Instant};

use silex_desktop::history::{EditKind, GROUP_GAP, History, MAX_STEPS, Snapshot, first_difference};
use silex_desktop::model::{Block, BlockKind};

fn snap(title: &str) -> Snapshot {
    Snapshot {
        title: title.into(),
        blocks: Vec::new(),
    }
}

fn text(id: &str) -> EditKind {
    EditKind::Text(id.into())
}

#[test]
fn typing_in_one_field_is_one_step() {
    let t0 = Instant::now();
    let mut h = History::default();
    h.record(snap("v0"), text("a"), t0);
    h.record(snap("v1"), text("a"), t0 + Duration::from_millis(200));
    h.record(snap("v2"), text("a"), t0 + Duration::from_millis(400));
    assert_eq!(h.undo(snap("now")), Some(snap("v0")));
    assert!(!h.can_undo());
}

#[test]
fn pause_other_field_or_structural_change_start_new_steps() {
    let t0 = Instant::now();
    let mut h = History::default();
    h.record(snap("v0"), text("a"), t0);
    h.record(
        snap("v1"),
        text("a"),
        t0 + GROUP_GAP + Duration::from_millis(1),
    );
    h.record(
        snap("v2"),
        text("b"),
        t0 + GROUP_GAP + Duration::from_millis(2),
    );
    h.record(
        snap("v3"),
        EditKind::Other,
        t0 + GROUP_GAP + Duration::from_millis(3),
    );
    h.record(
        snap("v4"),
        EditKind::Other,
        t0 + GROUP_GAP + Duration::from_millis(4),
    );
    for expected in ["v4", "v3", "v2", "v1", "v0"] {
        assert_eq!(h.undo(snap("x")).map(|s| s.title), Some(expected.into()));
    }
}

#[test]
fn redo_walks_forward_and_a_new_edit_clears_it() {
    let t0 = Instant::now();
    let mut h = History::default();
    h.record(snap("v0"), EditKind::Other, t0);
    h.record(snap("v1"), EditKind::Other, t0);
    assert_eq!(h.undo(snap("v2")), Some(snap("v1")));
    assert_eq!(h.undo(snap("v1")), Some(snap("v0")));
    assert_eq!(h.redo(snap("v0")), Some(snap("v1")));
    assert!(h.can_redo());
    h.record(snap("v1"), EditKind::Other, t0);
    assert!(!h.can_redo(), "a new edit drops the redo branch");
}

#[test]
fn typing_right_after_undo_is_a_new_step() {
    let t0 = Instant::now();
    let mut h = History::default();
    h.record(snap("v0"), text("a"), t0);
    assert_eq!(h.undo(snap("v1")), Some(snap("v0")));
    h.record(snap("v0"), text("a"), t0 + Duration::from_millis(10));
    assert!(h.can_undo());
}

#[test]
fn history_is_capped() {
    let mut h = History::default();
    let t0 = Instant::now();
    for i in 0..MAX_STEPS + 10 {
        h.record(snap(&format!("v{i}")), EditKind::Other, t0);
    }
    let mut count = 0;
    while h.undo(snap("x")).is_some() {
        count += 1;
    }
    assert_eq!(count, MAX_STEPS);
}

#[test]
fn first_difference_finds_the_changed_block() {
    let b = |id: &str, text: &str| Block {
        id: id.into(),
        page_id: "p".into(),
        kind: BlockKind::Paragraph,
        content: text.into(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks: Vec::new(),
    };
    let a = vec![b("1", "x"), b("2", "y")];
    assert_eq!(first_difference(&a, &a), None);
    assert_eq!(first_difference(&a, &[b("1", "x"), b("2", "z")]), Some(1));
    assert_eq!(first_difference(&a, &[b("1", "x")]), Some(1));
}

/// ADR 0017: a step that moved blocks to another page says so, both ways,
/// and undoing it then redoing what came back restores the stacks.
#[test]
fn a_move_step_carries_its_blocks_both_ways() {
    use silex_desktop::history::Moved;
    let t0 = Instant::now();
    let moved = Moved {
        ids: vec!["a".into(), "b".into()],
        to: "p2".into(),
    };
    let mut h = History::default();
    h.record(snap("v0"), text("x"), t0);
    h.record_move(snap("v1"), moved.clone());
    // Typing right after is a step of its own.
    h.record(snap("v2"), text("x"), t0 + Duration::from_millis(10));
    assert_eq!(h.undo_step(snap("v3")), Some((snap("v2"), None)));
    assert_eq!(
        h.undo_step(snap("v2")),
        Some((snap("v1"), Some(moved.clone())))
    );
    // Undoing it failed: give it back.
    assert_eq!(
        h.redo_step(snap("v1")),
        Some((snap("v2"), Some(moved.clone())))
    );
    assert_eq!(
        h.undo_step(snap("v2")),
        Some((snap("v1"), Some(moved.clone())))
    );
    assert_eq!(h.redo_step(snap("v1")), Some((snap("v2"), Some(moved))));
    assert_eq!(h.redo_step(snap("v2")), Some((snap("v3"), None)));
    // The plain API still works on move steps.
    assert_eq!(h.undo(snap("v3")), Some(snap("v2")));
}
