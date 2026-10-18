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

#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
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
            self.undo.push(before);
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
        }
        self.last = Some((kind, now));
    }

    /// Returns the state to go back to; `current` becomes redoable.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.last = None;
        Some(previous)
    }

    /// Returns the state to go forward to; `current` becomes undoable.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.last = None;
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
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
