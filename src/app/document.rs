//! Open document state: saving and document undo/redo (ADR 0004).
use std::time::Instant;

use anyhow::Result;
use eframe::egui::{self};
use silex_desktop::{
    editor,
    history::{EditKind, Snapshot, first_difference},
};

use super::*;

impl SilexApp {
    /// A structural or non-text change to the open document.
    pub(super) fn mark_dirty(&mut self) {
        self.changed(EditKind::Other);
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            title: self.title.clone(),
            blocks: self.blocks.clone(),
        }
    }

    /// Records the change in the undo history (unless nothing actually
    /// changed) and schedules the autosave.
    pub(super) fn changed(&mut self, kind: EditKind) {
        let now = Instant::now();
        let current = self.snapshot();
        if current != self.shadow {
            let before = std::mem::replace(&mut self.shadow, current);
            self.history.record(before, kind, now);
        }
        self.save.mark_dirty(now);
    }

    pub(super) fn reset_history(&mut self) {
        self.history.clear();
        self.shadow = self.snapshot();
    }

    pub(super) fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(previous) = self.history.undo(current.clone()) {
            self.apply_snapshot(previous, &current);
        }
    }

    pub(super) fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(next) = self.history.redo(current.clone()) {
            self.apply_snapshot(next, &current);
        }
    }

    /// Replaces the document with `target` and puts the cursor at the end of
    /// the first block that changed.
    pub(super) fn apply_snapshot(&mut self, target: Snapshot, from: &Snapshot) {
        let focus = first_difference(&from.blocks, &target.blocks);
        self.title = target.title;
        self.blocks = target.blocks;
        self.shadow = self.snapshot();
        // New field ids: egui must not keep cursors or text state from before.
        self.doc_generation += 1;
        self.selection = None;
        self.slash = None;
        self.pending_focus = focus
            .and_then(|i| self.blocks.get(i.min(self.blocks.len().saturating_sub(1))))
            .map(|b| (b.id.clone(), editor::block_char_len(b)));
        self.save.mark_dirty(Instant::now());
    }

    /// True when Cmd+Z should act on the document: no field has focus (e.g.
    /// blocks are selected) or the title or a block has it. The search and
    /// rename fields keep their own text undo.
    pub(super) fn document_has_focus(&self, ctx: &egui::Context) -> bool {
        let Some(focused) = ctx.memory(|m| m.focused()) else {
            return true;
        };
        let page = self.selected.as_deref().unwrap_or_default();
        focused == field_id(self.doc_generation, "title", page)
            || self
                .blocks
                .iter()
                .any(|b| focused == self.block_field_id(&b.id))
    }

    /// Saves pending edits now. On failure the edits stay in memory, the
    /// error is shown in the status bar and a retry is scheduled.
    pub(super) fn save_now(&mut self) -> Result<()> {
        if !self.save.is_dirty() {
            return Ok(());
        }
        let Some(id) = self.selected.clone() else {
            self.save.record_success();
            return Ok(());
        };
        match self.db.save_document(&id, &self.title, &self.blocks) {
            Ok(()) => {
                self.save.record_success();
                // The save already succeeded; a failed list refresh is only a notice.
                let refreshed = self.reload_pages();
                self.report(refreshed);
                Ok(())
            }
            Err(err) => {
                self.save.record_failure(Instant::now(), format!("{err:#}"));
                Err(err)
            }
        }
    }
}
