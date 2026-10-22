//! Undo in a database's table (H6.4, ADR 0006): each change is already
//! saved in its own transaction, so undoing applies its inverse operation
//! (the previous value of a cell, a row back from the trash…), never a
//! snapshot of the whole database. The history belongs to the open table
//! and starts empty each time the database is opened.
//!
//! Undoable: cell values, row names, adding a row (undone by sending it to
//! the trash), sending a row to the trash, adding and deleting properties,
//! and the actions on several rows at once (H11.2), each as one step.
//! Not undoable here: renaming or moving properties, options and views.
//!
//! Rows sent to the trash from the table or the board (menu, «⋯», Supr,
//! the selection bar) show a notice with «Deshacer» (H11.1). It is the same step as Cmd+Z,
//! so the notice leaves as soon as another change is recorded or anything
//! is undone: its button never undoes something else.
use anyhow::Result;
use silex_desktop::property::CellValue;

use super::*;

/// The most changes kept; the oldest go first.
const LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TableOp {
    Cell {
        row: String,
        property: String,
        before: Option<CellValue>,
        after: Option<CellValue>,
    },
    Name {
        row: String,
        before: String,
        after: String,
    },
    AddRow(String),
    TrashRow(String),
    AddProperty(String),
    DeleteProperty(String),
    /// Several rows sent to the trash together (H11.2).
    TrashRows(Vec<String>),
    /// Several rows added together: the copies of the chosen rows (H11.4).
    AddRows(Vec<String>),
    /// A value put in several cells at once (H11.2).
    Cells(Vec<CellChange>),
}

/// One cell changed by an action on several rows (H11.2).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CellChange {
    pub(super) row: String,
    pub(super) property: String,
    pub(super) before: Option<CellValue>,
    pub(super) after: Option<CellValue>,
}

/// The notice of rows just sent to the trash (H11.1): what it says, and
/// the step «Deshacer» undoes.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TrashNotice {
    pub(super) database: String,
    pub(super) op: TableOp,
    pub(super) text: String,
}

#[derive(Debug, Default)]
pub(super) struct TableHistory {
    undo: Vec<TableOp>,
    redo: Vec<TableOp>,
}

impl TableHistory {
    /// A new change: it can be undone, and what was undone can no longer
    /// be redone.
    pub(super) fn record(&mut self, op: TableOp) {
        self.undo.push(op);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The change Cmd+Z would undo now.
    pub(super) fn last(&self) -> Option<&TableOp> {
        self.undo.last()
    }
}

/// Which way an operation is applied.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Undo,
    Redo,
}

impl SilexApp {
    /// Records a change made in the open table.
    pub(super) fn record_table(&mut self, op: TableOp) {
        self.trash_notice = None;
        if let Some(state) = self.database.as_mut() {
            state.history.record(op);
        }
    }

    pub(super) fn undo_table(&mut self) {
        self.step_table(Way::Undo);
    }

    pub(super) fn redo_table(&mut self) {
        self.step_table(Way::Redo);
    }

    /// Takes the last change of one stack, applies it the other way and
    /// moves it to the other stack. If it cannot be applied (its row was
    /// moved out of the database meanwhile…) it is dropped and the reason
    /// is shown.
    fn step_table(&mut self, way: Way) {
        // Pending names and cells are saved (and recorded) first.
        let saved = self.save_table_edits();
        self.report(saved);
        self.trash_notice = None;
        let Some(state) = self.database.as_mut() else {
            return;
        };
        let op = match way {
            Way::Undo => state.history.undo.pop(),
            Way::Redo => state.history.redo.pop(),
        };
        let Some(op) = op else {
            return;
        };
        match self.apply_table_op(&op, way) {
            Ok(()) => {
                if let Some(state) = self.database.as_mut() {
                    match way {
                        Way::Undo => state.history.redo.push(op),
                        Way::Redo => state.history.undo.push(op),
                    }
                }
            }
            Err(err) => {
                self.notice = Some(format!("No se pudo deshacer el cambio: {err:#}"));
            }
        }
    }

    fn apply_table_op(&mut self, op: &TableOp, way: Way) -> Result<()> {
        let undo = way == Way::Undo;
        match op {
            TableOp::Cell {
                row,
                property,
                before,
                after,
            } => {
                let value = if undo { before } else { after };
                self.store_cell(row, property, value.clone())?;
            }
            TableOp::Name { row, before, after } => {
                let title = if undo { before } else { after };
                self.db.rename_page(row, title)?;
                if let Some(state) = self.database.as_mut()
                    && let Some(page) = state.rows.iter_mut().find(|r| r.id == *row)
                {
                    page.title = title.clone();
                }
                self.pages = self.db.list_pages()?;
                self.rebuild_tree();
            }
            // Undoing a new row sends it to the trash; redoing brings it back.
            TableOp::AddRow(row) | TableOp::TrashRow(row) => {
                let trash = matches!(op, TableOp::AddRow(_)) == undo;
                if trash {
                    self.db.trash_page(row)?;
                } else {
                    self.db.restore_page(row)?;
                }
                self.reload_pages()?;
                if let Some(state) = self.database.as_mut() {
                    state.values = self.db.database_values(&state.id)?;
                    // A row back from the trash goes to its place, not to
                    // the end (H11.1).
                    if !trash {
                        state.apply_view();
                    }
                }
            }
            TableOp::TrashRows(rows) | TableOp::AddRows(rows) => {
                // Undoing new rows sends them to the trash; redoing brings
                // them back.
                let restore = matches!(op, TableOp::TrashRows(_)) == undo;
                if restore {
                    self.db.restore_pages(rows)?;
                } else {
                    self.db.trash_pages(rows)?;
                }
                self.reload_pages()?;
                if let Some(state) = self.database.as_mut() {
                    state.values = self.db.database_values(&state.id)?;
                    if restore {
                        state.apply_view();
                    }
                }
            }
            TableOp::Cells(changes) => {
                let cells = changes
                    .iter()
                    .map(|c| {
                        let value = if undo { &c.before } else { &c.after };
                        (c.row.clone(), c.property.clone(), value.clone())
                    })
                    .collect();
                self.store_cells(cells)?;
            }
            TableOp::AddProperty(property) | TableOp::DeleteProperty(property) => {
                let delete = matches!(op, TableOp::AddProperty(_)) == undo;
                if delete {
                    self.db.delete_property(property)?;
                } else {
                    self.db.restore_property(property)?;
                }
                self.reload_properties()?;
            }
        }
        Ok(())
    }

    /// The notice of a row just sent to the trash, while «Deshacer» would
    /// still bring it back: its database is open and its trashing is the
    /// last change. Otherwise it is dropped.
    pub(super) fn current_trash_notice(&mut self) -> Option<TrashNotice> {
        let notice = self.trash_notice.as_ref()?;
        let current = self
            .database
            .as_ref()
            .is_some_and(|d| d.id == notice.database && d.history.last() == Some(&notice.op));
        if !current {
            self.trash_notice = None;
        }
        self.trash_notice.clone()
    }

    /// Whether Cmd+Z is for the table: a database is open and no text is
    /// being typed (nothing has the keyboard, or a cell, a row's «⋯» or a
    /// card does, not as a text field). Text fields keep their own undo.
    pub(super) fn table_has_focus(&self, ctx: &egui::Context) -> bool {
        let Some(state) = &self.database else {
            return false;
        };
        let Some(focused) = ctx.memory(|m| m.focused()) else {
            return true;
        };
        if state.handle_focus == Some(focused) {
            return true;
        }
        let Some((index, column)) = state.cursor else {
            return false;
        };
        let cell = state.order.get(index).and_then(|&i| state.rows.get(i)).zip(
            column
                .checked_sub(1)
                .and_then(|c| state.columns.get(c))
                .and_then(|&p| state.properties.get(p)),
        );
        cell.is_some_and(|(row, property)| focused == cell_id(&row.id, &property.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_change_clears_what_could_be_redone_and_the_oldest_go() {
        let mut history = TableHistory::default();
        history.record(TableOp::AddRow("a".into()));
        history.redo.push(TableOp::AddRow("b".into()));
        history.record(TableOp::AddRow("c".into()));
        assert!(!history.can_redo());
        for i in 0..LIMIT + 10 {
            history.record(TableOp::AddRow(i.to_string()));
        }
        assert_eq!(history.undo.len(), LIMIT);
        assert_eq!(history.undo[0], TableOp::AddRow("10".into()));
    }
}
