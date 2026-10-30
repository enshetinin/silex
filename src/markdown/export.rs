//! Blocks → CommonMark (+ GFM task lists).
use std::collections::HashMap;

use crate::marks::{self, Mark, MarkKind};
use crate::model::{Block, BlockKind, Page, list_numbers};
use crate::property::{CellValue, Property, PropertyKind, TableValues};

/// Characters escaped anywhere in text: CommonMark inline syntax, entities,
/// and GFM extensions other renderers may enable (strikethrough, tables).
const ESCAPED: &[char] = &[
    '\\', '`', '*', '_', '[', ']', '<', '>', '#', '&', '!', '~', '|',
];

/// Renders a page: `# title`, a blank line, then the blocks.
///
/// - Headings `#`/`##`/`###`, lists `- ` / `N. `, tasks `- [ ] ` / `- [x] `,
///   quotes `> `, code in a fence longer than any backtick run inside.
/// - Indent becomes list nesting. An indented block with no list item above
///   it cannot be nested in Markdown and is written at the deepest level a
///   list allows (its text is kept, the extra indent is not).
/// - Newlines inside a block are hard breaks (`\` at the end of the line).
/// - Inline formatting (ADR 0015): `**negrita**`, `*cursiva*`, `` `código` ``,
///   `~~tachado~~` and `[texto](url)`; see [`inline_lines`].
/// - Empty paragraphs cannot be represented and are skipped.
pub fn page_to_markdown(title: &str, blocks: &[Block]) -> String {
    page_to_markdown_with(title, blocks, &HashMap::new(), &HashMap::new())
}

/// Where an exported attachment was written, for its link (ADR 0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLink {
    /// Relative to the page's `.md`, with `/` separators. Written between
    /// `<…>`, so it may have spaces; it must have no `<`, `>` or newline
    /// (names from `safe_path` have none).
    pub path: String,
    /// Shown as an image (`![…]`) rather than a link.
    pub image: bool,
}

/// [`page_to_markdown`], with file blocks as links to their exported copy
/// (`files`, by attachment id) and links to pages as links to their exported
/// file (`pages`, by page id: a relative path with `/`, ADR 0008). A block
/// without its target in the maps is just its name.
pub fn page_to_markdown_with(
    title: &str,
    blocks: &[Block],
    files: &HashMap<String, FileLink>,
    pages: &HashMap<String, String>,
) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&escape_line(&title.replace('\n', " "), false));
    out.push('\n');

    let numbers = list_numbers(blocks);
    // Per indent level: content offset of the list item that opened it, if
    // the latest block at that level was a list item.
    let mut item_widths: Vec<Option<usize>> = Vec::new();
    let mut previous_was_item = false;
    for (block, number) in blocks.iter().zip(numbers) {
        if block.kind == BlockKind::Paragraph && block.content.is_empty() {
            continue;
        }
        let mut level = 0;
        let mut leading = String::new();
        while level < usize::from(block.indent) {
            match item_widths.get(level).copied().flatten() {
                Some(width) => {
                    leading.push_str(&" ".repeat(width));
                    level += 1;
                }
                None => break,
            }
        }
        item_widths.truncate(level);

        let marker = match block.kind {
            BlockKind::Bulleted => Some("- ".to_owned()),
            BlockKind::Numbered => Some(format!("{}. ", number.unwrap_or(1))),
            BlockKind::Todo if block.checked => Some("- [x] ".to_owned()),
            BlockKind::Todo => Some("- [ ] ".to_owned()),
            _ => None,
        };
        let is_item = marker.is_some();
        // Consecutive list items form a tight list; anything else is separated.
        if !(is_item && previous_was_item) {
            out.push('\n');
        }

        match block.kind {
            BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3 => {
                let hashes = match block.kind {
                    BlockKind::Heading1 => "#",
                    BlockKind::Heading2 => "##",
                    _ => "###",
                };
                out.push_str(&leading);
                out.push_str(hashes);
                out.push(' ');
                let text = block.content.replace('\n', " ");
                out.push_str(&inline_lines(&text, &block.marks, false).concat());
                out.push('\n');
            }
            BlockKind::Code => push_code(&mut out, &leading, &block.content),
            BlockKind::PageLink => {
                let name = escape_line(&block.content.replace('\n', " "), false);
                out.push_str(&leading);
                match block.link.as_ref().and_then(|id| pages.get(id)) {
                    Some(path) => out.push_str(&format!("[{name}](<{}>)", clean_path(path))),
                    None => out.push_str(&name),
                }
                out.push('\n');
            }
            BlockKind::File => {
                let name = escape_line(&block.content.replace('\n', " "), false);
                let link = block.attachment.as_ref().and_then(|id| files.get(id));
                out.push_str(&leading);
                match link {
                    Some(link) => {
                        let path = clean_path(&link.path);
                        if link.image {
                            out.push('!');
                        }
                        out.push_str(&format!("[{name}](<{path}>)"));
                    }
                    None => out.push_str(&name),
                }
                out.push('\n');
            }
            BlockKind::Quote => {
                let prefix = format!("{leading}> ");
                let lines = inline_lines(&block.content, &block.marks, true);
                push_text_lines(&mut out, &prefix, &prefix, &lines);
            }
            _ => {
                let marker = marker.unwrap_or_default();
                // Task items nest under the text after "- ", like bullets.
                let width = if block.kind == BlockKind::Todo {
                    2
                } else {
                    marker.len()
                };
                let first = format!("{leading}{marker}");
                let rest = format!("{leading}{}", " ".repeat(if is_item { width } else { 0 }));
                let lines = inline_lines(&block.content, &block.marks, true);
                push_text_lines(&mut out, &first, &rest, &lines);
            }
        }
        item_widths.push(if is_item {
            Some(if block.kind == BlockKind::Todo {
                2
            } else {
                marker_len(block.kind, number)
            })
        } else {
            None
        });
        previous_was_item = is_item;
    }
    out
}

/// A path written between `<…>`: no `<`, `>` or line breaks.
fn clean_path(path: &str) -> String {
    path.chars()
        .filter(|c| !matches!(c, '<' | '>' | '\n' | '\r'))
        .collect()
}

fn marker_len(kind: BlockKind, number: Option<usize>) -> usize {
    match kind {
        BlockKind::Numbered => format!("{}. ", number.unwrap_or(1)).len(),
        _ => 2,
    }
}

/// Rendered lines joined by hard breaks; the first line gets `first`, the
/// others `rest` (continuation indent or quote marker).
fn push_text_lines(out: &mut String, first: &str, rest: &str, lines: &[String]) {
    for (i, line) in lines.iter().enumerate() {
        out.push_str(if i == 0 { first } else { rest });
        out.push_str(line);
        if i + 1 < lines.len() {
            out.push('\\');
        }
        out.push('\n');
    }
}

fn push_code(out: &mut String, leading: &str, content: &str) {
    let longest_run = content.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    out.push_str(leading);
    out.push_str(&fence);
    out.push('\n');
    if !content.is_empty() {
        for line in content.split('\n') {
            if !line.is_empty() {
                out.push_str(leading);
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(leading);
    out.push_str(&fence);
    out.push('\n');
}

/// A database as a GFM table (ADR 0006, H6.4): «Nombre» and one column
/// per live property, every live row in its manual order (an export of the
/// data, not of a view). A select is its option's name, a checkbox `[x]`
/// or `[ ]`, a date `AAAA-MM-DD`, a number as shown in the app. Each row
/// stays on one line and the text is escaped, so `|` cannot split a cell.
pub fn database_to_markdown(
    title: &str,
    properties: &[Property],
    rows: &[Page],
    values: &TableValues,
) -> String {
    let cell = |text: &str| escape_line(&text.replace(['\n', '\r'], " "), false);
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(&cell(title));
    out.push_str("\n\n| Nombre |");
    for property in properties {
        out.push(' ');
        out.push_str(&cell(&property.name));
        out.push_str(" |");
    }
    out.push_str("\n| --- |");
    for _ in properties {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows {
        out.push_str("| ");
        out.push_str(&cell(&row.title));
        out.push_str(" |");
        let cells = values.get(&row.id);
        for property in properties {
            let value = cells.and_then(|c| c.get(&property.id));
            let text = match (property.kind, value) {
                // Written as is: brackets would otherwise be escaped.
                (PropertyKind::Checkbox, v) => {
                    let on = matches!(v, Some(CellValue::Checkbox(true)));
                    (if on { "[x]" } else { "[ ]" }).to_owned()
                }
                (_, Some(value)) => cell(&value.display(property)),
                (_, None) => String::new(),
            };
            out.push(' ');
            out.push_str(&text);
            out.push_str(" |");
        }
        out.push('\n');
    }
    out
}

/// Escapes one line of block text so Markdown reads it back verbatim.
/// `line_start` also guards constructs only recognised at the start of a
/// line (lists, thematic breaks, setext underlines).
fn escape_line(line: &str, line_start: bool) -> String {
    escaped_chars(line, line_start).concat()
}

/// [`escape_line`], one piece per character of `line`, so formatting
/// delimiters can go between them.
fn escaped_chars(line: &str, line_start: bool) -> Vec<String> {
    // Leading/trailing spaces and tabs would be dropped (or turn the line
    // into a code block): write them as character references.
    let body = line.trim_matches([' ', '\t']);
    let lead = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
    let trail = if body.is_empty() {
        ""
    } else {
        &line[lead.len() + body.len()..]
    };

    let mut out = Vec::with_capacity(line.len());
    out.extend(lead.chars().map(|c| char_ref(c).to_owned()));
    let start_guard = line_start && lead.is_empty() && needs_start_escape(body);
    for (i, c) in body.char_indices() {
        let at_start = i == 0;
        let mut piece = String::with_capacity(c.len_utf8() + 1);
        if ESCAPED.contains(&c)
            || (start_guard && at_start && matches!(c, '-' | '+' | '='))
            || (start_guard && is_ordered_delimiter(body, i))
        {
            piece.push('\\');
        }
        piece.push(c);
        out.push(piece);
    }
    out.extend(trail.chars().map(|c| char_ref(c).to_owned()));
    out
}

/// A block's text as Markdown lines (split at `\n`), with its inline
/// formatting (ADR 0015).
///
/// Bold, italic and strikethrough leave the spaces at their edges out
/// (CommonMark does not read `** así**`); inline code is cut at line
/// breaks. CommonMark cannot write every mark next to every character
/// (`a**(b)**` is not bold), so the result is read back: a mark that would
/// not come back is written as plain text. The text itself always does.
/// Line breaks at the very end are left out: Markdown has no way to end a
/// paragraph with one (a `\` there would be read as text).
fn inline_lines(content: &str, marks: &[Mark], line_start: bool) -> Vec<String> {
    let content = content.trim_end_matches('\n');
    if marks.is_empty() {
        return content
            .split('\n')
            .map(|line| escape_line(line, line_start))
            .collect();
    }
    let wanted = export_marks(content, marks);
    let (lines, written) = render(content, &wanted, line_start);
    if reads_back(content, &lines, &written, line_start) {
        return lines;
    }
    // Keep each mark that still reads back with those kept so far.
    let mut kept: Vec<Mark> = Vec::new();
    for mark in wanted {
        let mut trial = kept.clone();
        trial.push(mark);
        marks::normalize(&mut trial, content.chars().count());
        let (lines, written) = render(content, &trial, line_start);
        if reads_back(content, &lines, &written, line_start) {
            kept = trial;
        }
    }
    render(content, &kept, line_start).0
}

/// The marks as Markdown can write them: in normal form, without spaces
/// at the edges of bold, italic and strikethrough, and inline code cut at
/// line breaks.
fn export_marks(content: &str, marks: &[Mark]) -> Vec<Mark> {
    let chars: Vec<char> = content.chars().collect();
    let mut out = Vec::new();
    for mark in marks::sanitize(marks, content) {
        match mark.kind {
            MarkKind::Bold | MarkKind::Italic | MarkKind::Strike => {
                let (mut start, mut end) = (mark.start, mark.end);
                while start < end && chars[start].is_whitespace() {
                    start += 1;
                }
                while end > start && chars[end - 1].is_whitespace() {
                    end -= 1;
                }
                out.push(Mark::new(start, end, mark.kind));
            }
            MarkKind::Code => {
                let mut start = mark.start;
                for (i, c) in chars.iter().enumerate().take(mark.end).skip(mark.start) {
                    if *c == '\n' {
                        out.push(Mark::new(start, i, MarkKind::Code));
                        start = i + 1;
                    }
                }
                out.push(Mark::new(start, mark.end, MarkKind::Code));
            }
            MarkKind::Link(_) => out.push(mark),
        }
    }
    marks::normalize(&mut out, chars.len());
    out
}

/// Whether `lines` (`content` rendered with marks that cover `written`)
/// read back as the same text and marks: as a paragraph, or as a heading
/// when written without `line_start` guards.
fn reads_back(content: &str, lines: &[String], written: &[Mark], line_start: bool) -> bool {
    let markdown = if line_start {
        lines.join("\\\n")
    } else {
        format!("## {}", lines.concat())
    };
    let page = super::markdown_to_page(&markdown, "");
    matches!(page.blocks.as_slice(), [block] if block.content == content && block.marks == written)
}

/// `content` with `marks` (as [`export_marks`] gives them), one string per
/// line, and the marks it really writes. Marks stay open across line
/// breaks (hard breaks keep them); when one closes inside another, those
/// above it close and open again. Inline code is always innermost. Bold,
/// italic and strikethrough cannot open before a space, so there they wait
/// for the next character (a space without bold looks the same).
fn render(content: &str, marks: &[Mark], line_start: bool) -> (Vec<String>, Vec<Mark>) {
    let chars: Vec<char> = content.chars().collect();
    let mut lines = Vec::new();
    let mut out = String::new();
    let mut stack: Vec<usize> = Vec::new();
    // What each character really gets, to compare with what comes back.
    let mut written: Vec<Mark> = Vec::new();
    let mut at = 0;
    let mut step = |out: &mut String, stack: &mut Vec<usize>, at: usize| {
        switch_marks(out, stack, marks, at, &chars);
        for &i in stack.iter() {
            match written
                .iter_mut()
                .rev()
                .find(|w| w.kind == marks[i].kind && w.end == at)
            {
                Some(run) => run.end = at + 1,
                None => written.push(Mark::new(at, at + 1, marks[i].kind.clone())),
            }
        }
    };
    for (n, line) in content.split('\n').enumerate() {
        if n > 0 {
            // The line break itself: marks over it stay open.
            step(&mut out, &mut stack, at);
            lines.push(std::mem::take(&mut out));
            at += 1;
        }
        for piece in escaped_chars(line, line_start) {
            step(&mut out, &mut stack, at);
            if stack.iter().any(|&i| marks[i].kind == MarkKind::Code) {
                out.push(chars[at]);
            } else {
                out.push_str(&piece);
            }
            at += 1;
        }
    }
    switch_marks(&mut out, &mut stack, marks, at, &chars);
    lines.push(out);
    marks::normalize(&mut written, chars.len());
    (lines, written)
}

/// Closes and opens delimiters before the character at `at` so that the
/// open marks (`stack`, innermost last) are those over it.
fn switch_marks(
    out: &mut String,
    stack: &mut Vec<usize>,
    marks: &[Mark],
    at: usize,
    chars: &[char],
) {
    let space = chars.get(at).is_some_and(|c| c.is_whitespace());
    let wanted: Vec<usize> = (0..marks.len())
        .filter(|&i| marks[i].start <= at && at < marks[i].end)
        .collect();
    let mut cut = stack
        .iter()
        .position(|i| !wanted.contains(i))
        .unwrap_or(stack.len());
    let opening = wanted.iter().any(|i| !stack.contains(i));
    if opening && let Some(code) = stack.iter().position(|&i| marks[i].kind == MarkKind::Code) {
        cut = cut.min(code);
    }
    for &i in stack[cut..].iter().rev() {
        out.push_str(&delimiter(&marks[i], chars, false));
    }
    stack.truncate(cut);
    let mut open: Vec<usize> = wanted
        .into_iter()
        .filter(|i| !stack.contains(i))
        .filter(|&i| {
            !(space
                && matches!(
                    marks[i].kind,
                    MarkKind::Bold | MarkKind::Italic | MarkKind::Strike
                ))
        })
        .collect();
    // Longest outside, links outside other marks, code inside everything.
    open.sort_by_key(|&i| {
        let mark = &marks[i];
        (
            mark.kind == MarkKind::Code,
            std::cmp::Reverse(mark.end),
            !matches!(mark.kind, MarkKind::Link(_)),
        )
    });
    for i in open {
        out.push_str(&delimiter(&marks[i], chars, true));
        stack.push(i);
    }
}

/// What opens (`opening`) or closes a mark in Markdown.
fn delimiter(mark: &Mark, chars: &[char], opening: bool) -> String {
    match &mark.kind {
        MarkKind::Bold => "**".into(),
        MarkKind::Italic => "*".into(),
        MarkKind::Strike => "~~".into(),
        MarkKind::Link(_) if opening => "[".into(),
        MarkKind::Link(url) => {
            let mut dest = String::with_capacity(url.len());
            for c in url.chars() {
                if matches!(c, '\\' | '(' | ')' | '<' | '>') {
                    dest.push('\\');
                }
                dest.push(c);
            }
            format!("]({dest})")
        }
        MarkKind::Code => {
            // A fence longer than any run of backticks inside, and a space
            // inside it where CommonMark would otherwise eat one.
            let code = &chars[mark.start..mark.end];
            let longest = code
                .split(|c| *c != '`')
                .map(<[char]>::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(longest + 1);
            let all_spaces = code.iter().all(|c| *c == ' ');
            let pad = code.first() == Some(&'`')
                || code.last() == Some(&'`')
                || (!all_spaces && code.first() == Some(&' ') && code.last() == Some(&' '));
            match (opening, pad) {
                (true, true) => format!("{fence} "),
                (false, true) => format!(" {fence}"),
                _ => fence,
            }
        }
    }
}

fn char_ref(c: char) -> &'static str {
    if c == '\t' { "&#9;" } else { "&#32;" }
}

/// Lines that would start a list item, a thematic break or a setext
/// heading underline (the other block markers are always escaped).
fn needs_start_escape(body: &str) -> bool {
    let first = body.chars().next();
    let second = body.chars().nth(1);
    let bullet = matches!(first, Some('-' | '+')) && matches!(second, None | Some(' ' | '\t'));
    let rule = !body.is_empty() && body.chars().all(|c| matches!(c, '-' | '=' | ' '));
    bullet || rule || ordered_marker_end(body).is_some()
}

/// "12. x" / "12) x" → index of the delimiter.
fn ordered_marker_end(body: &str) -> Option<usize> {
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &body[digits..];
    let delimiter = rest.chars().next()?;
    let after = rest[delimiter.len_utf8()..].chars().next();
    (matches!(delimiter, '.' | ')') && matches!(after, None | Some(' ' | '\t'))).then_some(digits)
}

fn is_ordered_delimiter(body: &str, index: usize) -> bool {
    ordered_marker_end(body) == Some(index)
}
