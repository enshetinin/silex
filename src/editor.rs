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
        attachment: None,
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
    // A file block has no text to split (ADR 0007).
    if !block.kind.is_text() {
        return None;
    }
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
/// Text never merges with a file block (ADR 0007): those are deleted as
/// blocks (selection or their menu).
pub fn backspace_at_start(blocks: &mut Vec<Block>, index: usize) -> Option<Caret> {
    let block = blocks.get_mut(index)?;
    if !block.kind.is_text() {
        return None;
    }
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
    if index == 0 || !blocks[index - 1].kind.is_text() {
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
    let mergeable = |i: usize| blocks.get(i).is_some_and(|b| b.kind.is_text());
    if !mergeable(index) || !mergeable(index + 1) {
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

// ---------------------------------------------------------------------------
// Slash menu
// ---------------------------------------------------------------------------

/// Longest query after "/" that still counts as a command being typed.
const MAX_SLASH_QUERY: usize = 24;

/// If the block starts with "/" and the cursor is in the word right after it,
/// returns that word (the menu filter). A space ends the command.
pub fn slash_query(content: &str, cursor: usize) -> Option<&str> {
    let rest = content.strip_prefix('/')?;
    // The command word runs from after "/" to the first whitespace.
    let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let word = &rest[..word_end];
    let word_len = char_len(word);
    (cursor == word_len + 1 && word_len <= MAX_SLASH_QUERY).then_some(word)
}

/// Lowercase without Spanish accents, so "codigo" finds "Código" and
/// "pequeno" finds "pequeño" (slash menu and command palette).
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Extra words each type answers to in the slash menu.
fn aliases(kind: BlockKind) -> &'static [&'static str] {
    match kind {
        BlockKind::Paragraph => &["texto", "parrafo", "p", "text"],
        BlockKind::Heading1 => &["h1", "titulo1", "heading1"],
        BlockKind::Heading2 => &["h2", "titulo2", "heading2"],
        BlockKind::Heading3 => &["h3", "titulo3", "heading3"],
        BlockKind::Todo => &["tarea", "todo", "check", "casilla"],
        BlockKind::Bulleted => &["lista", "vinetas", "ul", "bullet", "-"],
        BlockKind::Numbered => &["numerada", "ol", "1.", "numero"],
        BlockKind::Quote => &["cita", "quote", ">"],
        BlockKind::Code => &["codigo", "code", "```"],
        // Not offered: a file block is made by attaching a file.
        BlockKind::File => &[],
    }
}

/// Types matching the slash query, in menu order. Empty query: all types.
pub fn slash_matches(query: &str) -> Vec<BlockKind> {
    let query = fold(query);
    BlockKind::ALL
        .into_iter()
        .filter(|kind| {
            let label = fold(kind.label()).replace(' ', "");
            label.contains(&query) || aliases(*kind).iter().any(|a| a.starts_with(&query))
        })
        .collect()
}

/// Applies a slash command: removes "/query" (everything before the cursor)
/// and changes the block type. Returns where the cursor goes.
pub fn apply_slash(
    blocks: &mut [Block],
    index: usize,
    cursor: usize,
    kind: BlockKind,
) -> Option<Caret> {
    let block = blocks.get_mut(index)?;
    if !block.kind.is_text() || !kind.is_text() {
        return None;
    }
    let cut = byte_index(&block.content, cursor);
    block.content.replace_range(..cut, "");
    block.kind = kind;
    if kind != BlockKind::Todo {
        block.checked = false;
    }
    Some(Caret {
        block: index,
        char: 0,
    })
}

// ---------------------------------------------------------------------------
// Block selection
// ---------------------------------------------------------------------------

/// Blocks removed by a selection delete, with their original positions, so
/// the deletion can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedBlocks {
    pub removed: Vec<(usize, Block)>,
}

/// Deletes blocks `first..=last` (clamped). Returns what was removed.
pub fn delete_range(blocks: &mut Vec<Block>, first: usize, last: usize) -> DeletedBlocks {
    let (first, last) = (first.min(last), first.max(last));
    if first >= blocks.len() {
        return DeletedBlocks {
            removed: Vec::new(),
        };
    }
    let last = last.min(blocks.len() - 1);
    let removed = blocks
        .drain(first..=last)
        .enumerate()
        .map(|(offset, block)| (first + offset, block))
        .collect();
    DeletedBlocks { removed }
}

/// Puts deleted blocks back at their original positions (clamped if the
/// document got shorter meanwhile).
pub fn restore_deleted(blocks: &mut Vec<Block>, deleted: DeletedBlocks) {
    for (index, block) in deleted.removed {
        let at = index.min(blocks.len());
        blocks.insert(at, block);
    }
}

/// Plain text of blocks `first..=last`: one line per block, two spaces per
/// indent level, list markers and task boxes. (Markdown is milestone 4.)
pub fn selection_text(blocks: &[Block], first: usize, last: usize) -> String {
    let (first, last) = (first.min(last), first.max(last));
    let numbers = crate::model::list_numbers(blocks);
    blocks
        .iter()
        .zip(numbers)
        .enumerate()
        .filter(|(i, _)| (first..=last).contains(i))
        .map(|(_, (block, number))| {
            let indent = "  ".repeat(usize::from(block.indent));
            let marker = match block.kind {
                BlockKind::Bulleted => "• ".to_owned(),
                BlockKind::Numbered => format!("{}. ", number.unwrap_or(1)),
                BlockKind::Todo if block.checked => "[x] ".to_owned(),
                BlockKind::Todo => "[ ] ".to_owned(),
                _ => String::new(),
            };
            format!("{indent}{marker}{}", block.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Moving and duplicating blocks
// ---------------------------------------------------------------------------

/// Moves blocks `first..=last` one place up or down (keeping their indent).
/// Returns false at the edges.
pub fn move_range(blocks: &mut [Block], first: usize, last: usize, up: bool) -> bool {
    let (first, last) = (first.min(last), first.max(last));
    if last >= blocks.len() {
        return false;
    }
    if up {
        if first == 0 {
            return false;
        }
        blocks[first - 1..=last].rotate_left(1);
    } else {
        if last + 1 >= blocks.len() {
            return false;
        }
        blocks[first..=last + 1].rotate_right(1);
    }
    true
}

/// Inserts copies of blocks `first..=last` right after them, with new ids.
/// Returns the index range of the copies.
pub fn duplicate_range(
    blocks: &mut Vec<Block>,
    first: usize,
    last: usize,
    mut new_id: impl FnMut() -> String,
) -> Option<(usize, usize)> {
    let (first, last) = (first.min(last), first.max(last));
    if last >= blocks.len() {
        return None;
    }
    let copies: Vec<Block> = blocks[first..=last]
        .iter()
        .map(|b| Block {
            id: new_id(),
            ..b.clone()
        })
        .collect();
    let count = copies.len();
    blocks.splice(last + 1..last + 1, copies);
    Some((last + 1, last + count))
}

// ---------------------------------------------------------------------------
// Pasting text
// ---------------------------------------------------------------------------

/// One pasted line, with the structure recognized from Silex's own copy
/// format (see [`selection_text`]): two spaces per indent level, "• ",
/// "N. ", "[ ] " / "[x] ". Anything else is plain text. Full Markdown is
/// milestone 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedLine {
    pub kind: BlockKind,
    pub indent: u8,
    pub checked: bool,
    pub content: String,
}

/// Line endings to `\n`, and invisible control characters removed (tabs kept).
pub fn normalize_pasted(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

pub fn parse_pasted_line(line: &str) -> PastedLine {
    let spaces = line.chars().take_while(|c| *c == ' ').count();
    let indent = u8::try_from(spaces / 2)
        .unwrap_or(MAX_INDENT)
        .min(MAX_INDENT);
    let rest = &line[spaces / 2 * 2..];
    let (kind, checked, content) = if let Some(text) = rest.strip_prefix("• ") {
        (BlockKind::Bulleted, false, text)
    } else if let Some(text) = rest.strip_prefix("[ ] ") {
        (BlockKind::Todo, false, text)
    } else if let Some(text) = rest.strip_prefix("[x] ") {
        (BlockKind::Todo, true, text)
    } else if let Some(text) = numbered_text(rest) {
        (BlockKind::Numbered, false, text)
    } else {
        // Plain text keeps its leading spaces only beyond the indent we took.
        return PastedLine {
            kind: BlockKind::Paragraph,
            indent: 0,
            checked: false,
            content: line.to_owned(),
        };
    };
    PastedLine {
        kind,
        indent,
        checked,
        content: content.to_owned(),
    }
}

/// "12. texto" → "texto".
fn numbered_text(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    (digits > 0)
        .then(|| line[digits..].strip_prefix(". "))
        .flatten()
}

/// Pastes multi-line `text` at `cursor` in block `index`: the first line
/// joins the text before the cursor, each further line becomes a block, and
/// the text after the cursor ends up after the last pasted line. If the
/// block was empty before the cursor, the first line's structure applies to
/// it. Returns where the cursor goes (end of the pasted text).
pub fn paste_lines(
    blocks: &mut Vec<Block>,
    index: usize,
    cursor: usize,
    text: &str,
    mut new_id: impl FnMut() -> String,
) -> Option<Caret> {
    let text = normalize_pasted(text);
    let mut lines = text.split('\n').map(parse_pasted_line);
    let first = lines.next()?;
    let block = blocks.get_mut(index)?;
    let split_at = byte_index(&block.content, cursor);
    let tail = block.content.split_off(split_at);
    let base_indent = block.indent;
    if block.content.is_empty() && first.kind != BlockKind::Paragraph {
        block.kind = first.kind;
        block.checked = first.checked;
        block.indent = base_indent.saturating_add(first.indent).min(MAX_INDENT);
    }
    block.content.push_str(&first.content);

    let template = block.clone();
    let mut last = index;
    for line in lines {
        last += 1;
        blocks.insert(
            last,
            Block {
                id: new_id(),
                kind: line.kind,
                checked: line.checked,
                indent: base_indent.saturating_add(line.indent).min(MAX_INDENT),
                content: line.content,
                ..template.clone()
            },
        );
    }
    let target = &mut blocks[last];
    let char = char_len(&target.content);
    target.content.push_str(&tail);
    Some(Caret { block: last, char })
}
