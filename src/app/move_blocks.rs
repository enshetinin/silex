//! «Mover a…» (H12.6a, ADR 0017): blocks of the open page go to the end of
//! another page, in one transaction, and Cmd+Z brings them back with what
//! was written in them there.
use std::time::Instant;

use anyhow::{Result, bail};
use silex_desktop::editor;
use silex_desktop::history::{Moved, Snapshot};
use silex_desktop::model::{BlockKind, Page, PageKind};

use super::*;

impl SilexApp {
    /// Opens the page picker to move blocks `first..=last` (with the
    /// content of their toggles, ADR 0016).
    pub(super) fn open_move_picker(&mut self, first: usize, last: usize) {
        if self.blocks.get(first).is_none() {
            return;
        }
        let last = editor::with_content(&self.blocks, first, last.min(self.blocks.len() - 1));
        let ids: Vec<String> = self.blocks[first..=last]
            .iter()
            .map(|b| b.id.clone())
            .collect();
        self.links.picker = Some(LinkPicker {
            moving: Some(ids),
            opening: true,
            ..LinkPicker::default()
        });
        self.slash = None;
    }

    /// Moves blocks `ids` of the open page to the end of `page`: one undo
    /// step that remembers where they went.
    pub(super) fn move_blocks_to(&mut self, ids: &[String], page: &Page) -> Result<()> {
        let Some(open) = self.selected.clone() else {
            return Ok(());
        };
        if page.id == open || page.kind != PageKind::Page {
            bail!("Los bloques solo se pueden mover a otra página");
        }
        let Some(first) = self.blocks.iter().position(|b| ids.contains(&b.id)) else {
            return Ok(());
        };
        // What is moved is what was last written.
        self.save_now()?;
        let before = self.snapshot();
        self.db.move_blocks(ids, &page.id)?;
        self.blocks.retain(|b| !ids.contains(&b.id));
        self.shadow = self.snapshot();
        self.history.record_move(
            before,
            Moved {
                ids: ids.to_vec(),
                to: page.id.clone(),
                created: false,
            },
        );
        self.save.mark_dirty(Instant::now());
        self.forget_history_of(&page.id);
        self.selection = None;
        // The cursor stays where the blocks were.
        if let Some(block) = self
            .blocks
            .get(first)
            .or_else(|| self.blocks.get(first.saturating_sub(1)))
        {
            self.pending_focus = Some((block.id.clone(), 0));
        }
        self.notice = Some(format!(
            "{} a «{}». {}+Z para deshacer.",
            match ids.len() {
                1 => "Se movió 1 bloque".to_owned(),
                n => format!("Se movieron {n} bloques"),
            },
            short_title(&page.title),
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Ctrl"
            },
        ));
        self.save_now()
    }

    /// Undoes a move (ADR 0017): the blocks come back from wherever they
    /// are now, with what was written in them there, to where `previous`
    /// had them.
    pub(super) fn undo_move(
        &mut self,
        previous: Snapshot,
        current: &Snapshot,
        moved: &Moved,
    ) -> Result<()> {
        let Some(open) = self.selected.clone() else {
            return Ok(());
        };
        self.save_now()?;
        let now = self.db.blocks_by_ids(&moved.ids)?;
        let found: Vec<String> = now.iter().map(|b| b.id.clone()).collect();
        let from = self.db.move_blocks(&found, &open)?;
        let mut target = previous;
        for block in &mut target.blocks {
            // What was written there, at the indent it had here.
            if let Some(latest) = now.iter().find(|b| b.id == block.id) {
                *block = Block {
                    page_id: open.clone(),
                    indent: block.indent,
                    ..latest.clone()
                };
            }
        }
        self.apply_snapshot(target, current);
        for page in from {
            self.forget_history_of(&page);
        }
        self.save_now()?;
        // A subpage made by «Convertir en subpágina», left empty: to the
        // trash (it can be restored from there).
        if moved.created
            && self.db.list_blocks(&moved.to)?.is_empty()
            && !self
                .pages
                .iter()
                .any(|p| p.parent_id.as_deref() == Some(moved.to.as_str()))
        {
            self.db.trash_page(&moved.to)?;
            self.reload_pages()?;
            self.close_tabs_not_live();
        }
        Ok(())
    }

    /// Redoes a move: the blocks go to the end of their page again.
    pub(super) fn redo_move(
        &mut self,
        next: Snapshot,
        current: &Snapshot,
        moved: &Moved,
    ) -> Result<()> {
        self.save_now()?;
        // The subpage it made, back from the trash.
        if moved.created && !self.pages.iter().any(|p| p.id == moved.to) {
            self.db.restore_page(&moved.to)?;
            self.reload_pages()?;
        }
        let present: Vec<String> = moved
            .ids
            .iter()
            .filter(|id| self.blocks.iter().any(|b| &b.id == *id))
            .cloned()
            .collect();
        self.db.move_blocks(&present, &moved.to)?;
        // `next` was taken before: its other blocks are as they were then.
        let mut target = next;
        target.blocks.retain(|b| !moved.ids.contains(&b.id));
        self.apply_snapshot(target, current);
        self.forget_history_of(&moved.to);
        self.save_now()
    }

    /// «Convertir en subpágina» (H12.6b, ADR 0017): blocks `first..=last`
    /// (with the content of their toggles) go to a new subpage of the open
    /// page, «Sin título» like any new page, and a link to it takes their
    /// place.
    pub(super) fn convert_to_subpage(&mut self, first: usize, last: usize) -> Result<()> {
        let Some(open) = self.selected.clone() else {
            return Ok(());
        };
        let Some(head) = self.blocks.get(first) else {
            return Ok(());
        };
        let indent = head.indent;
        let last = editor::with_content(&self.blocks, first, last.min(self.blocks.len() - 1));
        let ids: Vec<String> = self.blocks[first..=last]
            .iter()
            .map(|b| b.id.clone())
            .collect();
        // What goes is what was last written.
        self.save_now()?;
        let title = "Sin título";
        let page = self.db.create_page(title, Some(&open))?;
        let before = self.snapshot();
        self.db.move_blocks(&ids, &page)?;
        self.blocks.retain(|b| !ids.contains(&b.id));
        let link = Block {
            id: uuid::Uuid::new_v4().to_string(),
            page_id: open.clone(),
            kind: BlockKind::PageLink,
            content: title.to_owned(),
            checked: false,
            position: 0,
            indent,
            attachment: None,
            link: Some(page.clone()),
            marks: Vec::new(),
        };
        let link_id = link.id.clone();
        self.blocks.insert(first.min(self.blocks.len()), link);
        self.shadow = self.snapshot();
        let count = ids.len();
        self.history.record_move(
            before,
            Moved {
                ids,
                to: page,
                created: true,
            },
        );
        self.save.mark_dirty(Instant::now());
        self.selection = None;
        self.expanded.insert(open);
        self.reload_pages()?;
        self.pending_focus = Some((link_id, 0));
        self.notice = Some(format!(
            "{} a una subpágina nueva. {}+Z para deshacer.",
            match count {
                1 => "Se pasó 1 bloque".to_owned(),
                n => format!("Se pasaron {n} bloques"),
            },
            if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Ctrl"
            },
        ));
        self.save_now()
    }

    /// Blocks came to or left page `id` behind its back: the undo its tab
    /// kept no longer matches it and is dropped, and the page beside is
    /// read again if it shows it.
    pub(super) fn forget_history_of(&mut self, id: &str) {
        if let Some(parked) = self.parked.get_mut(id) {
            parked.history.clear();
        }
        if self.side.as_ref().is_some_and(|s| s.page == id) {
            self.refresh_side();
        }
    }
}
