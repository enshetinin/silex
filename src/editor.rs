//! Block editing operations (Enter, Backspace, Delete, Tab), independent of egui.
//!
//! Cursor positions are **character** indices (Unicode scalar values), the same
//! unit egui uses for text cursors; they are converted to byte offsets here so a
//! split never lands inside a multi-byte character.
use crate::model::{Block, BlockKind, MAX_INDENT};

/// Where the text cursor should go after an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caret {
    pub block: usize,
    pub char: usize,
}

/// Code blocks keep Enter as a newline inside the block.
pub fn enter_inserts_newline(kind: BlockKind) -> bool {
    kind == BlockKind::Code
}

/// Kind of the block created by pressing Enter at the end of `kind`:
/// lists and tasks continue, everything else goes back to plain text.
pub fn continuation_kind(kind: BlockKind) -> BlockKind {
    match kind {
        BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo => kind,
        _ => BlockKind::Paragraph,
    }
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Byte offset of character `char_index`, clamped to the end of the text.
fn byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(byte, _)| byte)
}

fn new_block_like(source: &Block, kind: BlockKind, id: String, content: String) -> Block {
    Block {
        id,
        page_id: source.page_id.clone(),
        kind,
        content,
        checked: false,
        position: 0,
        indent: source.indent,
    }
}

/// Enter at `at_char` in block `index` (no selection).
///
/// - On an empty list item, task or quote: leaves the list (outdent first,
///   then turn into text) instead of adding a block.
/// - At the start of a non-empty block: inserts an empty block above and
///   keeps the cursor where it was.
/// - Otherwise: splits; the text after the cursor moves to a new block below.
///
/// `new_id` is called only if a block is created.
pub fn enter(
    blocks: &mut Vec<Block>,
    index: usize,
    at_char: usize,
    new_id: impl FnOnce() -> String,
) -> Option<Caret> {
    let block = blocks.get(index)?;
    let leaves_list = matches!(
        block.kind,
        BlockKind::Bulleted | BlockKind::Numbered | BlockKind::Todo | BlockKind::Quote
    );
    if block.content.is_empty() && leaves_list {
        let block = &mut blocks[index];
        if block.indent > 0 {
            block.indent -= 1;
        } else {
            block.kind = BlockKind::Paragraph;
            block.checked = false;
        }
        return Some(Caret {
            block: index,
            char: 0,
        });
    }

    let kind = continuation_kind(block.kind);
    if at_char == 0 && !block.content.is_empty() {
        let above = new_block_like(block, kind, new_id(), String::new());
        blocks.insert(index, above);
        return Some(Caret {
            block: index + 1,
            char: 0,
        });
    }

    let split_at = byte_index(&block.content, at_char);
    let tail = blocks[index].content.split_off(split_at);
    let below = new_block_like(&blocks[index], kind, new_id(), tail);
    blocks.insert(index + 1, below);
    Some(Caret {
        block: index + 1,
        char: 0,
    })
}

/// Backspace at the very start of block `index` (no selection).
///
/// Step by step, like Notion: a styled block first becomes plain text, an
/// indented block is then outdented, and only a plain top-level block is
/// merged into the previous one. Returns `None` when nothing happens.
pub fn backspace_at_start(blocks: &mut Vec<Block>, index: usize) -> Option<Caret> {
    let block = blocks.get_mut(index)?;
    if block.kind != BlockKind::Paragraph {
        block.kind = BlockKind::Paragraph;
        block.checked = false;
        return Some(Caret {
            block: index,
            char: 0,
        });
    }
    if block.indent > 0 {
        block.indent -= 1;
        return Some(Caret {
            block: index,
            char: 0,
        });
    }
    if index == 0 {
        return None;
    }
    let removed = blocks.remove(index);
    let previous = &mut blocks[index - 1];
    let char = char_len(&previous.content);
    previous.content.push_str(&removed.content);
    Some(Caret {
        block: index - 1,
        char,
    })
}

/// Delete at the very end of block `index`: pulls the next block's text in.
pub fn delete_at_end(blocks: &mut Vec<Block>, index: usize) -> Option<Caret> {
    if index + 1 >= blocks.len() {
        return None;
    }
    let next = blocks.remove(index + 1);
    let block = &mut blocks[index];
    let char = char_len(&block.content);
    block.content.push_str(&next.content);
    Some(Caret { block: index, char })
}

/// Tab (`delta` = 1) or Shift+Tab (`delta` = -1). A block can be at most one
/// level deeper than the block above it, and never deeper than [`MAX_INDENT`].
/// Returns whether the indent changed.
pub fn change_indent(blocks: &mut [Block], index: usize, delta: i8) -> bool {
    let limit = match index {
        0 => 0,
        _ => blocks
            .get(index - 1)
            .map_or(0, |b| b.indent.saturating_add(1))
            .min(MAX_INDENT),
    };
    let Some(block) = blocks.get_mut(index) else {
        return false;
    };
    let wanted = if delta > 0 {
        block.indent.saturating_add(1).min(limit)
    } else {
        block.indent.saturating_sub(1)
    };
    let changed = wanted != block.indent;
    block.indent = wanted;
    changed
}

/// Number of characters in a block's text (for "cursor at end" checks).
pub fn block_char_len(block: &Block) -> usize {
    char_len(&block.content)
}
