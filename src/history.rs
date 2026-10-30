//! Document undo/redo (ADR 0004): snapshots of the open page, independent of egui.
//!
//! Every change records the state *before* it. Consecutive typing in the same
//! field within [`GROUP_GAP`] is merged into one step; any other change
//! (split, merge, move, type change, delete…) is its own step.
use std::time::{Duration, Instant};

use crate::model::Block;

/// Typing pauses longer than this start a new undo step.
pub const GROUP_GAP: Duration = Duration::from_millis(1000);
/// Undo steps kept per page (oldest dropped first).
pub const MAX_STEPS: usize = 100;

/// Everything undo can bring back for the open page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub title: String,
    pub blocks: Vec<Block>,
}

/// What kind of change is being recorded (decides grouping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditKind {
    /// Typing in one text field (block id, or "title").
    Text(String),
    /// Anything structural or non-text.
    Other,
}

/// Blocks a step moved to another page (ADR 0017): undoing it brings them
/// back from wherever they are; redoing it sends them to `to` again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The blocks, in their order.
    pub ids: Vec<String>,
    /// The page they went to.
    pub to: String,
    /// The step created that page («Convertir en subpágina»): undoing it
    /// sends the page, if left empty, to the trash; redoing brings it back.
    pub created: bool,
}

/// A step: the state on one side of it, and what it did to other pages.
type Step = (Snapshot, Option<Moved>);

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    last: Option<(EditKind, Instant)>,
}

impl History {
    /// Records the state before a change. Starts a new step unless this is
    /// more typing in the same field shortly after the previous keystroke.
    pub fn record(&mut self, before: Snapshot, kind: EditKind, now: Instant) {
        self.redo.clear();
        let continues_typing = matches!(kind, EditKind::Text(_))
            && self.last.as_ref().is_some_and(|(k, at)| {
                *k == kind && now.saturating_duration_since(*at) < GROUP_GAP
            });
        if !continues_typing {
            self.push((before, None));
        }
        self.last = Some((kind, now));
    }

    /// Records a step that moved blocks to another page (ADR 0017), always
    /// a step of its own.
    pub fn record_move(&mut self, before: Snapshot, moved: Moved) {
        self.redo.clear();
        self.push((before, Some(moved)));
        self.last = None;
    }

    fn push(&mut self, step: Step) {
        self.undo.push(step);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
    }

    /// Returns the state to go back to; `current` becomes redoable.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        self.undo_step(current).map(|(snapshot, _)| snapshot)
    }

    /// Returns the state to go forward to; `current` becomes undoable.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        self.redo_step(current).map(|(snapshot, _)| snapshot)
    }

    /// [`History::undo`], with the blocks the step moved to another page,
    /// if it did. [`History::redo_step`] with what it returned puts things
    /// back as they were (when undoing it could not be done).
    pub fn undo_step(&mut self, current: Snapshot) -> Option<(Snapshot, Option<Moved>)> {
        let (previous, moved) = self.undo.pop()?;
        self.redo.push((current, moved.clone()));
        self.last = None;
        Some((previous, moved))
    }

    /// [`History::redo`], with the blocks the step moved to another page.
    pub fn redo_step(&mut self, current: Snapshot) -> Option<(Snapshot, Option<Moved>)> {
        let (next, moved) = self.redo.pop()?;
        self.undo.push((current, moved.clone()));
        self.last = None;
        Some((next, moved))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The attachments that undo or redo can bring back (ADR 0007): they
    /// must not be deleted while this history lives.
    pub fn attachments(&self) -> std::collections::HashSet<String> {
        self.undo
            .iter()
            .chain(&self.redo)
            .flat_map(|(snapshot, _)| &snapshot.blocks)
            .filter_map(|block| block.attachment.clone())
            .collect()
    }

    /// Forget everything (page switch, backup restore).
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// First block index where two block lists differ (for placing the cursor
/// after undo/redo). `None` if they are equal.
pub fn first_difference(a: &[Block], b: &[Block]) -> Option<usize> {
    (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i))
}
