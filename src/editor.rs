//! Block editing operations (Enter, Backspace, Delete, Tab), independent of egui.
//!
//! Cursor positions are **character** indices (Unicode scalar values), the same
//! unit egui uses for text cursors; they are converted to byte offsets here so a
//! split never lands inside a multi-byte character.
use crate::marks;
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
        link: None,
        marks: Vec::new(),
    }
}

/// Drops the formatting of a block whose kind does not take it (code,
/// ADR 0015), after its kind changed.
pub fn fit_marks(block: &mut Block) {
    if !block.kind.takes_marks() {
        block.marks.clear();
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
        BlockKind::Bulleted
            | BlockKind::Numbered
            | BlockKind::Todo
            | BlockKind::Quote
            | BlockKind::Callout
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
    let (before, after) = marks::split(&block.marks, at_char);
    let tail = blocks[index].content.split_off(split_at);
    blocks[index].marks = before;
    let mut below = new_block_like(&blocks[index], kind, new_id(), tail);
    below.marks = after;
    fit_marks(&mut below);
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
    // A divider above goes, as in Notion; the text stays where it is.
    if index > 0 && blocks[index - 1].kind == BlockKind::Divider {
        blocks.remove(index - 1);
        return Some(Caret {
            block: index - 1,
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
    previous.marks = marks::join(&previous.marks, char, &removed.marks);
    fit_marks(previous);
    Some(Caret {
        block: index - 1,
        char,
    })
}

/// Delete at the very end of block `index`: pulls the next block's text in.
pub fn delete_at_end(blocks: &mut Vec<Block>, index: usize) -> Option<Caret> {
    // A divider below goes (H12.2).
    if blocks.get(index).is_some_and(|b| b.kind.is_text())
        && blocks
            .get(index + 1)
            .is_some_and(|b| b.kind == BlockKind::Divider)
    {
        blocks.remove(index + 1);
        let char = char_len(&blocks[index].content);
        return Some(Caret { block: index, char });
    }
    let mergeable = |i: usize| blocks.get(i).is_some_and(|b| b.kind.is_text());
    if !mergeable(index) || !mergeable(index + 1) {
        return None;
    }
    let next = blocks.remove(index + 1);
    let block = &mut blocks[index];
    let char = char_len(&block.content);
    block.content.push_str(&next.content);
    block.marks = marks::join(&block.marks, char, &next.marks);
    fit_marks(block);
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

/// Lowercase without accents, so "codigo" finds "Código" and "pequeno"
/// finds "pequeño" (slash menu, command palette, filters and find in the
/// page). See [`fold_char`].
pub fn fold(text: &str) -> String {
    text.chars().flat_map(fold_char).collect()
}

/// One character lowercased and without its accent: the precomposed Latin
/// letters lose theirs, and a combining accent (U+0300–U+036F, as in
/// "e\u{301}") folds to nothing. Most characters fold to one; a few
/// lowercase to more (`İ` → `i̇` → `i`), none to more than three.
pub fn fold_char(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().filter_map(|c| match c {
        '\u{300}'..='\u{36f}' => None,
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => Some('a'),
        'é' | 'è' | 'ë' | 'ê' => Some('e'),
        'í' | 'ì' | 'ï' | 'î' => Some('i'),
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => Some('o'),
        'ú' | 'ù' | 'ü' | 'û' => Some('u'),
        'ý' | 'ÿ' => Some('y'),
        'ñ' => Some('n'),
        'ç' => Some('c'),
        other => Some(other),
    })
}

/// One match of a search in the page (H11.7): the block (an index into
/// the page's blocks) and its characters `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

/// Where `query` appears in `text`, ignoring case and accents ([`fold`]),
/// as character ranges, left to right and not overlapping. A match takes
/// in the combining accents right after it, so replacing "e" in
/// "e\u{301}" leaves no accent behind. A query that folds to nothing
/// (empty, or only combining accents) finds nothing; spaces count.
pub fn find_in_text(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let needle: Vec<char> = fold(query).chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // The folded text, each folded char with the text char it came from.
    let chars: Vec<char> = text.chars().collect();
    let mut folded = Vec::with_capacity(chars.len());
    let mut origin = Vec::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        for f in fold_char(c) {
            folded.push(f);
            origin.push(i);
        }
    }
    let combining = |c: char| ('\u{300}'..='\u{36f}').contains(&c);
    let mut found = Vec::new();
    let mut at = 0;
    while at + needle.len() <= folded.len() {
        if folded[at..at + needle.len()] != needle[..] {
            at += 1;
            continue;
        }
        let start = origin[at];
        let mut end = origin[at + needle.len() - 1] + 1;
        while chars.get(end).is_some_and(|&c| combining(c)) {
            end += 1;
        }
        // A match that starts inside a char folded to several (`İ`) and
        // overlaps the previous one is left out.
        if found
            .last()
            .is_none_or(|r: &std::ops::Range<usize>| r.end <= start)
        {
            found.push(start..end);
        }
        // Go on after the text char where this one ends.
        at += needle.len();
        while at < folded.len() && origin[at] < end {
            at += 1;
        }
    }
    found
}

/// Every match of `query` in the text blocks of a page, in page order
/// (files and page links have no text to search).
pub fn find_in_blocks(blocks: &[Block], query: &str) -> Vec<Match> {
    blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind.is_text())
        .flat_map(|(block, b)| {
            find_in_text(&b.content, query)
                .into_iter()
                .map(move |r| Match {
                    block,
                    start: r.start,
                    end: r.end,
                })
        })
        .collect()
}

/// Replaces `matches` (from [`find_in_blocks`] on these blocks) with
/// `replacement`. Matches that no longer fit their block are skipped.
/// Returns how many were replaced.
pub fn replace_matches(blocks: &mut [Block], matches: &[Match], replacement: &str) -> usize {
    let mut sorted: Vec<&Match> = matches.iter().collect();
    // From the end, so the earlier positions stay valid.
    sorted.sort_by_key(|m| std::cmp::Reverse((m.block, m.start)));
    let mut replaced = 0;
    let mut last: Option<(usize, usize)> = None;
    for m in sorted {
        let Some(block) = blocks.get_mut(m.block) else {
            continue;
        };
        let len = char_len(&block.content);
        let overlaps = last.is_some_and(|(b, start)| b == m.block && m.end > start);
        if m.start >= m.end || m.end > len || overlaps || !block.kind.is_text() {
            continue;
        }
        let from = byte_index(&block.content, m.start);
        let to = byte_index(&block.content, m.end);
        block.content.replace_range(from..to, replacement);
        // The new text takes the formatting of the first replaced
        // character (ADR 0015).
        block.marks = marks::replace(&block.marks, m.start, m.end, char_len(replacement));
        last = Some((m.block, m.start));
        replaced += 1;
    }
    replaced
}

/// The match to show first when the search starts or its text changes:
/// the first at or after the caret (block, char), else the first.
pub fn match_from(matches: &[Match], caret: Option<(usize, usize)>) -> usize {
    let Some((block, char)) = caret else {
        return 0;
    };
    matches
        .iter()
        .position(|m| (m.block, m.end) >= (block, char))
        .unwrap_or(0)
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
        BlockKind::Callout => &["aviso", "callout", "nota", "destacado", "info"],
        // Not offered as a type: made by attaching a file, choosing a
        // page or «/separador» (a divider has no text to keep).
        BlockKind::File | BlockKind::PageLink | BlockKind::Divider => &[],
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

/// An entry of the slash menu: a block type, or attaching files (H7.3,
/// ADR 0007), which opens the system's file dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashItem {
    Kind(BlockKind),
    AttachFiles,
    /// A link to another page (H7.5, ADR 0008): opens the page picker.
    LinkPage,
    /// A divider (H12.2): see [`insert_divider`].
    Divider,
}

/// Words «Separador» answers to in the slash menu.
const DIVIDER_WORDS: &[&str] = &["separador", "divisor", "linea", "divider", "hr", "---"];

/// Words «Enlazar a una página» answers to in the slash menu.
const LINK_WORDS: &[&str] = &["enlace", "enlazar", "pagina", "link", "vincular", "mencion"];

/// Words «Adjuntar archivos» answers to in the slash menu.
const ATTACH_WORDS: &[&str] = &[
    "archivo",
    "adjuntar",
    "adjunto",
    "imagen",
    "foto",
    "pdf",
    "documento",
    "file",
    "attach",
];

/// The slash menu for `query`: the matching types (see [`slash_matches`]),
/// then «Separador», «Enlazar a una página» and «Adjuntar archivos» if
/// they match too.
pub fn slash_items(query: &str) -> Vec<SlashItem> {
    let mut items: Vec<SlashItem> = slash_matches(query)
        .into_iter()
        .map(SlashItem::Kind)
        .collect();
    let folded = fold(query);
    if DIVIDER_WORDS.iter().any(|w| w.starts_with(&folded)) {
        items.push(SlashItem::Divider);
    }
    if LINK_WORDS.iter().any(|w| w.starts_with(&folded)) {
        items.push(SlashItem::LinkPage);
    }
    if ATTACH_WORDS.iter().any(|w| w.starts_with(&folded)) {
        items.push(SlashItem::AttachFiles);
    }
    items
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
    block.marks = marks::replace(&block.marks, 0, cursor, 0);
    block.kind = kind;
    fit_marks(block);
    if kind != BlockKind::Todo {
        block.checked = false;
    }
    Some(Caret {
        block: index,
        char: 0,
    })
}

/// A divider at block `index` (H12.2), from «/separador» (the text
/// before `cursor` is the slash query) or `---` typed in a paragraph (all
/// of it). If nothing is left, the block becomes the divider and a new
/// paragraph below takes the cursor; otherwise the divider goes above the
/// text, which keeps the cursor.
pub fn insert_divider(
    blocks: &mut Vec<Block>,
    index: usize,
    cursor: usize,
    new_id: impl FnOnce() -> String,
) -> Option<Caret> {
    let block = blocks.get_mut(index)?;
    if !block.kind.is_text() {
        return None;
    }
    let cut = byte_index(&block.content, cursor);
    block.content.replace_range(..cut, "");
    block.marks = marks::replace(&block.marks, 0, cursor, 0);
    if block.content.is_empty() {
        block.kind = BlockKind::Divider;
        block.checked = false;
        block.marks.clear();
        let below = new_block_like(block, BlockKind::Paragraph, new_id(), String::new());
        blocks.insert(index + 1, below);
        return Some(Caret {
            block: index + 1,
            char: 0,
        });
    }
    let divider = new_block_like(block, BlockKind::Divider, new_id(), String::new());
    blocks.insert(index, divider);
    Some(Caret {
        block: index + 1,
        char: 0,
    })
}

/// Whether a paragraph's text is a divider written as Markdown (`---`):
/// typing it turns the paragraph into one (H12.2).
pub fn is_divider_text(kind: BlockKind, content: &str) -> bool {
    kind == BlockKind::Paragraph && content == "---"
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
    selection_clip(blocks, first, last).text
}

/// Text copied inside Silex, with its formatting over it (ADR 0015).
/// Pasting exactly this text gives the formatting back; other apps only
/// get the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clip {
    pub text: String,
    pub marks: Vec<marks::Mark>,
}

impl Clip {
    /// The characters `[start, end)` of a block, with their formatting.
    /// `None` if the text would change when pasted (control characters).
    pub fn of_text(block: &Block, start: usize, end: usize) -> Option<Clip> {
        let text: String = block
            .content
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        (normalize_pasted(&text) == text).then(|| Clip {
            marks: marks::slice(&block.marks, start, end),
            text,
        })
    }

    /// The formatting of `pasted` if it is this text (line endings aside).
    pub fn marks_for(&self, pasted: &str) -> Option<&[marks::Mark]> {
        (normalize_pasted(pasted) == self.text).then_some(self.marks.as_slice())
    }
}

/// [`selection_text`] with the formatting of each block, moved to where
/// its text is in the copied lines.
pub fn selection_clip(blocks: &[Block], first: usize, last: usize) -> Clip {
    let (first, last) = (first.min(last), first.max(last));
    let numbers = crate::model::list_numbers(blocks);
    let mut clip = Clip::default();
    let mut at = 0;
    for (i, (block, number)) in blocks.iter().zip(numbers).enumerate() {
        if !(first..=last).contains(&i) {
            continue;
        }
        if i > first {
            clip.text.push('\n');
            at += 1;
        }
        let indent = "  ".repeat(usize::from(block.indent));
        let marker = match block.kind {
            BlockKind::Bulleted => "• ".to_owned(),
            BlockKind::Numbered => format!("{}. ", number.unwrap_or(1)),
            BlockKind::Todo if block.checked => "[x] ".to_owned(),
            BlockKind::Todo => "[ ] ".to_owned(),
            _ => String::new(),
        };
        let marker = if block.kind == BlockKind::Divider {
            "---".to_owned()
        } else {
            marker
        };
        let prefix = format!("{indent}{marker}");
        at += char_len(&prefix);
        clip.text.push_str(&prefix);
        clip.text.push_str(&block.content);
        clip.marks.extend(
            block
                .marks
                .iter()
                .map(|m| marks::Mark::new(m.start + at, m.end + at, m.kind.clone())),
        );
        at += char_len(&block.content);
    }
    clip
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

/// Moves blocks `first..=last` to the gap `to` (0 = before the first
/// block, `len` = after the last), as dropping them there with the mouse
/// (H9.2). Returns the new range, or `None` when nothing moves (a gap
/// inside or at either edge of the range, or a range out of bounds).
pub fn move_range_to(
    blocks: &mut [Block],
    first: usize,
    last: usize,
    to: usize,
) -> Option<(usize, usize)> {
    let (first, last) = (first.min(last), first.max(last));
    if last >= blocks.len() || to > blocks.len() || (first..=last + 1).contains(&to) {
        return None;
    }
    let count = last - first + 1;
    if to < first {
        blocks[to..=last].rotate_right(count);
        Some((to, to + count - 1))
    } else {
        blocks[first..to].rotate_left(count);
        Some((to - count, to - 1))
    }
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
    if rest == "---" {
        // A divider, as Silex copies it (and as Markdown writes it).
        return PastedLine {
            kind: BlockKind::Divider,
            indent,
            checked: false,
            content: String::new(),
        };
    }
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
    new_id: impl FnMut() -> String,
) -> Option<Caret> {
    paste_lines_marked(blocks, index, cursor, text, &[], new_id)
}

/// [`paste_lines`] of text copied inside Silex: `pasted` is its formatting
/// (over the text with `\r\n` as `\n`, see [`Clip`]), which each line
/// keeps without its list marker.
pub fn paste_lines_marked(
    blocks: &mut Vec<Block>,
    index: usize,
    cursor: usize,
    text: &str,
    pasted: &[marks::Mark],
    mut new_id: impl FnMut() -> String,
) -> Option<Caret> {
    let text = normalize_pasted(text);
    let mut at = 0;
    let mut lines: Vec<(PastedLine, Vec<marks::Mark>)> = text
        .split('\n')
        .map(|raw| {
            let line = parse_pasted_line(raw);
            let len = char_len(raw);
            // The content is the end of the line, after its marker.
            let start = at + len - char_len(&line.content);
            let line_marks = marks::slice(pasted, start, at + len);
            at += len + 1;
            (line, line_marks)
        })
        .collect();
    let block = blocks.get_mut(index)?;
    // A divider never takes text (H12.2): one pasted after some text goes
    // on a line of its own.
    if lines
        .first()
        .is_some_and(|(l, _)| l.kind == BlockKind::Divider)
        && byte_index(&block.content, cursor) > 0
    {
        let empty = PastedLine {
            kind: BlockKind::Paragraph,
            indent: 0,
            checked: false,
            content: String::new(),
        };
        lines.insert(0, (empty, Vec::new()));
    }
    let mut lines = lines.into_iter();
    let (first, first_marks) = lines.next()?;
    let split_at = byte_index(&block.content, cursor);
    let tail = block.content.split_off(split_at);
    // The text after the cursor keeps its formatting; the pasted text
    // gets its own (none, if it was not copied in Silex).
    let (head_marks, tail_marks) = marks::split(&block.marks, cursor);
    block.marks = marks::join(&head_marks, char_len(&block.content), &first_marks);
    let base_indent = block.indent;
    if block.content.is_empty() && first.kind != BlockKind::Paragraph {
        block.kind = first.kind;
        block.checked = first.checked;
        block.indent = base_indent.saturating_add(first.indent).min(MAX_INDENT);
    }
    block.content.push_str(&first.content);
    fit_marks(block);

    let template = block.clone();
    let mut last = index;
    for (line, line_marks) in lines {
        last += 1;
        let mut new = Block {
            id: new_id(),
            kind: line.kind,
            checked: line.checked,
            indent: base_indent.saturating_add(line.indent).min(MAX_INDENT),
            content: line.content,
            marks: line_marks,
            ..template.clone()
        };
        fit_marks(&mut new);
        blocks.insert(last, new);
    }
    // The text after the cursor never ends up in a divider.
    if blocks[last].kind == BlockKind::Divider {
        let below = Block {
            id: new_id(),
            kind: BlockKind::Paragraph,
            checked: false,
            indent: blocks[last].indent,
            content: String::new(),
            marks: Vec::new(),
            ..template.clone()
        };
        last += 1;
        blocks.insert(last, below);
    }
    let target = &mut blocks[last];
    let char = char_len(&target.content);
    target.content.push_str(&tail);
    target.marks = marks::join(&target.marks, char, &tail_marks);
    fit_marks(target);
    Some(Caret { block: last, char })
}
