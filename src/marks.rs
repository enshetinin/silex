//! Inline formatting (ADR 0015), without SQL or egui: bold, italic, code,
//! strikethrough and web links as ranges over a block's plain text.
//!
//! A [`Mark`] covers `[start, end)` in **characters** (Unicode scalar
//! values, like `src/editor.rs` and egui's `CCursor`). A list of marks is
//! kept in normal form ([`normalize`]): sorted, inside the text, no empty
//! ranges, marks of one kind never overlapping or touching (they merge),
//! and links never overlapping each other. Different kinds may overlap.
use serde_json::{Map, Value, json};

/// What a mark does to its text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkKind {
    Bold,
    Italic,
    Code,
    Strike,
    /// A web link; the URL is always one [`normalize_url`] accepts.
    Link(String),
}

impl MarkKind {
    /// Name in the stored JSON.
    fn as_str(&self) -> &'static str {
        match self {
            MarkKind::Bold => "bold",
            MarkKind::Italic => "italic",
            MarkKind::Code => "code",
            MarkKind::Strike => "strike",
            MarkKind::Link(_) => "link",
        }
    }

    /// Whether `other` is the same kind of formatting (any two links are).
    pub fn same_type(&self, other: &MarkKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Whether text typed right at the end of the mark continues it:
    /// yes for bold, italic and strikethrough, no for code and links.
    fn inclusive_end(&self) -> bool {
        matches!(self, MarkKind::Bold | MarkKind::Italic | MarkKind::Strike)
    }
}

/// Formatting over the characters `[start, end)` of a block's text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mark {
    pub start: usize,
    pub end: usize,
    pub kind: MarkKind,
}

impl Mark {
    pub fn new(start: usize, end: usize, kind: MarkKind) -> Self {
        Self { start, end, kind }
    }

    /// The URL of a link.
    pub fn url(&self) -> Option<&str> {
        match &self.kind {
            MarkKind::Link(url) => Some(url),
            _ => None,
        }
    }
}

/// Puts `marks` in normal form for a text of `len` characters: ranges are
/// cut to the text and empty ones dropped; marks of one kind that overlap
/// or touch are merged (links only with the same URL); a link overlapping
/// an earlier link loses the overlapping part.
pub fn normalize(marks: &mut Vec<Mark>, len: usize) {
    marks.retain_mut(|m| {
        m.end = m.end.min(len);
        m.start < m.end
    });
    marks.sort_by(|a, b| {
        (type_order(&a.kind), a.start, a.end).cmp(&(type_order(&b.kind), b.start, b.end))
    });
    let mut out: Vec<Mark> = Vec::with_capacity(marks.len());
    for mut mark in marks.drain(..) {
        if let Some(last) = out.last_mut()
            && last.kind.same_type(&mark.kind)
        {
            if last.kind == mark.kind && mark.start <= last.end {
                last.end = last.end.max(mark.end);
                continue;
            }
            // Two different links: the earlier one keeps the overlap.
            if mark.start < last.end {
                mark.start = last.end;
                if mark.start >= mark.end {
                    continue;
                }
            }
        }
        out.push(mark);
    }
    out.sort_by(|a, b| (a.start, a.end, &a.kind).cmp(&(b.start, b.end, &b.kind)));
    *marks = out;
}

/// Sort key that groups the marks of one type together.
fn type_order(kind: &MarkKind) -> u8 {
    match kind {
        MarkKind::Bold => 0,
        MarkKind::Italic => 1,
        MarkKind::Code => 2,
        MarkKind::Strike => 3,
        MarkKind::Link(_) => 4,
    }
}

/// Marks after the characters `[start, end)` of the text are replaced by
/// `inserted` new characters (an insertion has `start == end`, a deletion
/// `inserted == 0`).
///
/// Marks before the edit stay, marks after it move, and marks that lose
/// all their characters disappear. The new text takes the formatting of
/// the first replaced character; an insertion takes the formatting of the
/// marks it falls inside, and continues bold, italic and strikethrough
/// when it is typed right at their end (ADR 0015).
pub fn replace(marks: &[Mark], start: usize, end: usize, inserted: usize) -> Vec<Mark> {
    let end = end.max(start);
    let new_end = start + inserted;
    // Only called with `pos >= end`, so it never underflows.
    let shift = |pos: usize| pos + inserted - (end - start);
    let mut out: Vec<Mark> = marks
        .iter()
        .filter_map(|m| {
            let covers = if end > start {
                m.start <= start && start < m.end
            } else {
                m.start < start && (start < m.end || (start == m.end && m.kind.inclusive_end()))
            };
            let mut s = if m.start >= end {
                shift(m.start)
            } else if m.start <= start {
                m.start
            } else {
                new_end
            };
            let mut e = if m.end <= start {
                m.end
            } else if m.end >= end {
                shift(m.end)
            } else {
                start
            };
            if covers {
                s = s.min(start);
                e = e.max(new_end);
            }
            (s < e).then(|| Mark::new(s, e, m.kind.clone()))
        })
        .collect();
    let len = out.iter().map(|m| m.end).max().unwrap_or(0);
    normalize(&mut out, len);
    out
}

/// Where `old` and `new` differ, as one edit found from their common
/// prefix and suffix: `(start, old_end, new_end)` in characters, so
/// `old[start..old_end]` became `new[start..new_end]`.
pub fn edit_range(old: &str, new: &str) -> (usize, usize, usize) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars
        .iter()
        .zip(&new_chars)
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars
        .iter()
        .rev()
        .zip(new_chars.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    (prefix, old_chars.len() - suffix, new_chars.len() - suffix)
}

/// Marks after the text changed from `old` to `new` (one edit: typing,
/// pasting, deleting or IME text). The changed part is found from the
/// common prefix and suffix ([`edit_range`]), so formatting outside it is
/// kept.
pub fn adjust(marks: &[Mark], old: &str, new: &str) -> Vec<Mark> {
    let len = new.chars().count();
    if marks.is_empty() || old == new {
        let mut out = marks.to_vec();
        normalize(&mut out, len);
        return out;
    }
    let (start, old_end, new_end) = edit_range(old, new);
    let mut out = replace(marks, start, old_end, new_end - start);
    normalize(&mut out, len);
    out
}

/// The kinds of formatting text typed at `at` takes: the marks it falls
/// inside, and bold, italic and strikethrough that end right there
/// (as [`replace`] does with an insertion).
pub fn typing_kinds(marks: &[Mark], at: usize) -> Vec<MarkKind> {
    marks
        .iter()
        .filter(|m| m.start < at && (at < m.end || (at == m.end && m.kind.inclusive_end())))
        .map(|m| m.kind.clone())
        .collect()
}

/// Formatting with a keyboard shortcut (ADR 0015): everything but links,
/// which need a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Bold,
    Italic,
    Code,
    Strike,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Bold, Format::Italic, Format::Code, Format::Strike];

    pub fn kind(self) -> MarkKind {
        match self {
            Format::Bold => MarkKind::Bold,
            Format::Italic => MarkKind::Italic,
            Format::Code => MarkKind::Code,
            Format::Strike => MarkKind::Strike,
        }
    }

    /// Name in menus and the palette.
    pub fn label(self) -> &'static str {
        match self {
            Format::Bold => "Negrita",
            Format::Italic => "Cursiva",
            Format::Code => "Código en línea",
            Format::Strike => "Tachado",
        }
    }
}

/// Bold, italic, code or strikethrough chosen with no text selected: it
/// applies to what is typed next at `at`, until the cursor moves
/// (ADR 0015). `on` adds formatting the text would not take, `off` drops
/// formatting it would continue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypingFormat {
    /// Where the cursor was when it was chosen. An IME rewrites its text
    /// from there, so edits between `from` and `at` keep it.
    pub from: usize,
    /// Where the next text goes: the end of what was typed with it.
    pub at: usize,
    pub on: Vec<Format>,
    pub off: Vec<Format>,
}

impl TypingFormat {
    /// The typing format after `format` is toggled at `at` in a text with
    /// `marks`, starting from `current` (kept only if it is for the same
    /// place). `None` when typing would just follow the marks again.
    pub fn toggle(
        marks: &[Mark],
        current: Option<TypingFormat>,
        at: usize,
        format: Format,
    ) -> Option<TypingFormat> {
        let mut next = current.filter(|t| t.at == at).unwrap_or(TypingFormat {
            from: at,
            at,
            ..TypingFormat::default()
        });
        let follows = typing_kinds(marks, at).contains(&format.kind());
        if next.has(marks, format) {
            next.on.retain(|f| *f != format);
            if follows {
                next.off.push(format);
            }
        } else {
            next.off.retain(|f| *f != format);
            if !follows {
                next.on.push(format);
            }
        }
        (!next.on.is_empty() || !next.off.is_empty()).then_some(next)
    }

    /// Whether text typed now at `self.at` would have `format`.
    pub fn has(&self, marks: &[Mark], format: Format) -> bool {
        if self.on.contains(&format) {
            return true;
        }
        !self.off.contains(&format) && typing_kinds(marks, self.at).contains(&format.kind())
    }

    /// Applies an edit `(start, old_end, new_end)` (see [`edit_range`])
    /// to `marks` (already moved for it): the new text gets this format,
    /// and the next text goes after it. Returns `None` if the edit was
    /// somewhere else, which ends the typing format.
    pub fn typed(
        &mut self,
        marks: &[Mark],
        (start, old_end, new_end): (usize, usize, usize),
    ) -> Option<Vec<Mark>> {
        if start < self.from || start > self.at || old_end > self.at {
            return None;
        }
        self.at = new_end;
        Some(self.apply(marks, start, new_end))
    }

    /// The marks once the characters `[start, end)` were typed with this
    /// format (`marks` already moved for them).
    pub fn apply(&self, marks: &[Mark], start: usize, end: usize) -> Vec<Mark> {
        let mut out = marks.to_vec();
        for format in &self.on {
            out = add(&out, format.kind(), start, end);
        }
        for format in &self.off {
            out = remove(&out, &format.kind(), start, end);
        }
        out
    }
}

/// Splits the marks of a text at character `at`: those of the part before
/// and those of the part after (counted from its own start).
pub fn split(marks: &[Mark], at: usize) -> (Vec<Mark>, Vec<Mark>) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for m in marks {
        if m.start < at {
            before.push(Mark::new(m.start, m.end.min(at), m.kind.clone()));
        }
        if m.end > at {
            after.push(Mark::new(m.start.max(at) - at, m.end - at, m.kind.clone()));
        }
    }
    let len_before = at;
    normalize(&mut before, len_before);
    let len_after = after.iter().map(|m| m.end).max().unwrap_or(0);
    normalize(&mut after, len_after);
    (before, after)
}

/// The marks of a text of `first_len` characters followed by another text
/// with `second` marks (merging a block into the one before it).
pub fn join(first: &[Mark], first_len: usize, second: &[Mark]) -> Vec<Mark> {
    let mut out: Vec<Mark> = first.to_vec();
    out.extend(
        second
            .iter()
            .map(|m| Mark::new(m.start + first_len, m.end + first_len, m.kind.clone())),
    );
    let len = out.iter().map(|m| m.end).max().unwrap_or(0);
    normalize(&mut out, len);
    out
}

/// The marks of the characters `[start, end)`, counted from `start`.
pub fn slice(marks: &[Mark], start: usize, end: usize) -> Vec<Mark> {
    let mut out: Vec<Mark> = marks
        .iter()
        .filter(|m| m.start < end && m.end > start)
        .map(|m| {
            Mark::new(
                m.start.max(start) - start,
                m.end.min(end) - start,
                m.kind.clone(),
            )
        })
        .collect();
    normalize(&mut out, end.saturating_sub(start));
    out
}

/// Whether every character of `[start, end)` has formatting of the same
/// type as `kind` (any link, for a link). False for an empty range.
pub fn covers(marks: &[Mark], kind: &MarkKind, start: usize, end: usize) -> bool {
    if start >= end {
        return false;
    }
    let mut ranges: Vec<(usize, usize)> = marks
        .iter()
        .filter(|m| m.kind.same_type(kind))
        .map(|m| (m.start, m.end))
        .collect();
    ranges.sort_unstable();
    let mut reached = start;
    for (s, e) in ranges {
        if s > reached {
            break;
        }
        reached = reached.max(e);
        if reached >= end {
            return true;
        }
    }
    false
}

/// Removes formatting of the same type as `kind` from `[start, end)`.
pub fn remove(marks: &[Mark], kind: &MarkKind, start: usize, end: usize) -> Vec<Mark> {
    let mut out = Vec::with_capacity(marks.len() + 1);
    for m in marks {
        if !m.kind.same_type(kind) || m.end <= start || m.start >= end {
            out.push(m.clone());
            continue;
        }
        if m.start < start {
            out.push(Mark::new(m.start, start, m.kind.clone()));
        }
        if m.end > end {
            out.push(Mark::new(end, m.end, m.kind.clone()));
        }
    }
    let len = out.iter().map(|m| m.end).max().unwrap_or(0);
    normalize(&mut out, len);
    out
}

/// Adds `kind` over `[start, end)`. A link replaces any link there.
pub fn add(marks: &[Mark], kind: MarkKind, start: usize, end: usize) -> Vec<Mark> {
    let mut out = if matches!(kind, MarkKind::Link(_)) {
        remove(marks, &kind, start, end)
    } else {
        marks.to_vec()
    };
    out.push(Mark::new(start, end, kind));
    let len = out.iter().map(|m| m.end).max().unwrap_or(0);
    normalize(&mut out, len);
    out
}

/// Bold, italic, code or strikethrough on a selection: removed if the
/// whole of `[start, end)` has it, added otherwise.
pub fn toggle(marks: &[Mark], kind: MarkKind, start: usize, end: usize) -> Vec<Mark> {
    if covers(marks, &kind, start, end) {
        remove(marks, &kind, start, end)
    } else {
        add(marks, kind, start, end)
    }
}

/// The marks of a text where every `from` (a non-empty string) became
/// `to`, as `str::replace` does (`{{fecha}}` in templates). Each
/// occurrence is one [`replace`], from the last to the first.
pub fn replace_all(marks: &[Mark], text: &str, from: &str, to: &str) -> Vec<Mark> {
    if marks.is_empty() || from.is_empty() {
        return marks.to_vec();
    }
    let from_len = from.chars().count();
    let to_len = to.chars().count();
    let starts: Vec<usize> = text
        .match_indices(from)
        .map(|(byte, _)| text[..byte].chars().count())
        .collect();
    let mut out = marks.to_vec();
    for start in starts.into_iter().rev() {
        out = replace(&out, start, start + from_len, to_len);
    }
    out
}

/// `marks` as they may be stored for `text`: in normal form, inside the
/// text, and without links to a URL that is not allowed.
pub fn sanitize(marks: &[Mark], text: &str) -> Vec<Mark> {
    let mut out: Vec<Mark> = marks
        .iter()
        .filter(|m| match &m.kind {
            MarkKind::Link(url) => normalize_url(url).as_deref() == Some(url.as_str()),
            _ => true,
        })
        .cloned()
        .collect();
    normalize(&mut out, text.chars().count());
    out
}

/// The stored form of `marks` (ADR 0015): `None` without marks, so the
/// column stays `NULL`; otherwise a JSON list of
/// `{"start", "end", "type"}` with `"url"` for links.
pub fn to_json(marks: &[Mark]) -> Option<String> {
    if marks.is_empty() {
        return None;
    }
    Some(to_value(marks).to_string())
}

/// `marks` as a JSON list (see [`to_json`]); also used by page versions.
pub fn to_value(marks: &[Mark]) -> Value {
    Value::Array(
        marks
            .iter()
            .map(|m| {
                let mut item = json!({
                    "start": m.start,
                    "end": m.end,
                    "type": m.kind.as_str(),
                });
                if let (MarkKind::Link(url), Value::Object(map)) = (&m.kind, &mut item) {
                    map.insert("url".into(), json!(url));
                }
                item
            })
            .collect(),
    )
}

/// Reads stored marks for a text of `len` characters. Anything that does
/// not fit (unknown type, range outside the text, a URL that is not
/// allowed) makes the whole list unreadable: the caller keeps the text
/// without formatting (ADR 0015).
pub fn from_json(json: &str, len: usize) -> Result<Vec<Mark>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|_| "el formato no es JSON válido".to_owned())?;
    from_value(&value, len)
}

/// Reads marks from a JSON list (see [`from_json`]).
pub fn from_value(value: &Value, len: usize) -> Result<Vec<Mark>, String> {
    let Value::Array(items) = value else {
        return Err("el formato no es una lista".to_owned());
    };
    let mut marks = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(map) = item else {
            return Err("una marca no es un objeto".to_owned());
        };
        let start = index(map, "start")?;
        let end = index(map, "end")?;
        if start >= end || end > len {
            return Err(format!("marca fuera del texto: {start}..{end}"));
        }
        let kind = match map.get("type").and_then(Value::as_str) {
            Some("bold") => MarkKind::Bold,
            Some("italic") => MarkKind::Italic,
            Some("code") => MarkKind::Code,
            Some("strike") => MarkKind::Strike,
            Some("link") => {
                let url = map
                    .get("url")
                    .and_then(Value::as_str)
                    .and_then(normalize_url)
                    .ok_or_else(|| "enlace sin una dirección permitida".to_owned())?;
                MarkKind::Link(url)
            }
            other => return Err(format!("tipo de marca desconocido: {other:?}")),
        };
        marks.push(Mark::new(start, end, kind));
    }
    normalize(&mut marks, len);
    Ok(marks)
}

fn index(map: &Map<String, Value>, key: &str) -> Result<usize, String> {
    map.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("marca sin «{key}»"))
}

/// The URL a link may point at, or `None` if it is not allowed: `http`,
/// `https` and `mailto` only (never `file:` or `javascript:`). Something
/// that looks like an address without a scheme (`ejemplo.com/x`) gets
/// `https://`. Spaces at the ends are trimmed; spaces or control
/// characters inside are refused.
pub fn normalize_url(input: &str) -> Option<String> {
    let url = input.trim();
    if url.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    {
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        return (!host.is_empty()).then(|| url.to_owned());
    }
    if let Some(rest) = lower.strip_prefix("mailto:") {
        return rest.contains('@').then(|| url.to_owned());
    }
    // Any other scheme ("file:", "javascript:", "ftp://") is refused; a
    // colon later on (a port, a path) is fine.
    let head = url.split(['/', '?', '#']).next().unwrap_or("");
    let host = head.split(':').next().unwrap_or("");
    let looks_like_host = host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '-');
    let port_ok = match head.split_once(':') {
        Some((_, port)) => !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()),
        None => true,
    };
    (looks_like_host && port_ok).then(|| format!("https://{url}"))
}

/// The link the cursor at `at` is in or touches (`start <= at <= end`):
/// what Cmd+Intro opens and what «Enlace…» edits with nothing selected.
/// With two links touching at `at`, the one that starts there.
pub fn link_at(marks: &[Mark], at: usize) -> Option<&Mark> {
    let mut links = marks
        .iter()
        .filter(|m| matches!(m.kind, MarkKind::Link(_)) && m.start <= at && at <= m.end);
    let first = links.next()?;
    Some(links.next().filter(|m| m.start == at).unwrap_or(first))
}

/// The link over the character `index` (`start <= index < end`): the one
/// under the pointer.
pub fn link_over(marks: &[Mark], index: usize) -> Option<&Mark> {
    marks
        .iter()
        .find(|m| matches!(m.kind, MarkKind::Link(_)) && m.start <= index && index < m.end)
}

/// The URL of the first link inside `[start, end)`, if any: what the link
/// field starts with.
pub fn link_url_in(marks: &[Mark], start: usize, end: usize) -> Option<&str> {
    marks
        .iter()
        .filter(|m| m.start < end && start < m.end)
        .find_map(Mark::url)
}

/// The marks once `[start, end)` links to what was typed in the link
/// field: nothing (only spaces) removes the links there; an allowed URL
/// (see [`normalize_url`]) replaces them. `None` if the URL is not allowed.
pub fn set_link(marks: &[Mark], start: usize, end: usize, input: &str) -> Option<Vec<Mark>> {
    if input.trim().is_empty() {
        return Some(remove(marks, &MarkKind::Link(String::new()), start, end));
    }
    let url = normalize_url(input)?;
    Some(if start < end {
        add(marks, MarkKind::Link(url), start, end)
    } else {
        marks.to_vec()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use MarkKind::*;

    fn m(start: usize, end: usize, kind: MarkKind) -> Mark {
        Mark::new(start, end, kind)
    }

    fn link(url: &str) -> MarkKind {
        Link(url.to_owned())
    }

    #[test]
    fn normalize_merges_sorts_and_cuts() {
        let mut marks = vec![
            m(4, 6, Bold),
            m(0, 2, Bold),
            m(2, 3, Bold),
            m(1, 5, Italic),
            m(7, 7, Code),
            m(8, 20, Strike),
        ];
        normalize(&mut marks, 10);
        assert_eq!(
            marks,
            vec![
                m(0, 3, Bold),
                m(1, 5, Italic),
                m(4, 6, Bold),
                m(8, 10, Strike)
            ]
        );
    }

    #[test]
    fn normalize_never_overlaps_links() {
        let mut marks = vec![
            m(0, 4, link("https://a.es")),
            m(4, 6, link("https://a.es")),
            m(5, 9, link("https://b.es")),
            m(6, 7, link("https://c.es")),
        ];
        normalize(&mut marks, 20);
        assert_eq!(
            marks,
            vec![m(0, 6, link("https://a.es")), m(6, 9, link("https://b.es"))]
        );
    }

    #[test]
    fn typing_inside_or_at_the_end_continues_the_formatting() {
        // "hola mundo", bold "mundo" (5..10).
        let marks = vec![m(5, 10, Bold)];
        assert_eq!(replace(&marks, 7, 7, 2), vec![m(5, 12, Bold)]);
        assert_eq!(replace(&marks, 10, 10, 1), vec![m(5, 11, Bold)]);
        // At the start: before the mark, not in it.
        assert_eq!(replace(&marks, 5, 5, 1), vec![m(6, 11, Bold)]);
        assert_eq!(replace(&marks, 0, 0, 3), vec![m(8, 13, Bold)]);
        // Code and links do not grow at their end.
        let code = vec![m(5, 10, Code), m(0, 3, link("https://a.es"))];
        assert_eq!(
            replace(&code, 10, 10, 1),
            vec![m(0, 3, link("https://a.es")), m(5, 10, Code)]
        );
        assert_eq!(
            replace(&code, 3, 3, 1),
            vec![m(0, 3, link("https://a.es")), m(6, 11, Code)]
        );
    }

    #[test]
    fn deleting_shrinks_moves_or_drops_marks() {
        let marks = vec![m(2, 5, Bold), m(6, 8, Italic)];
        assert_eq!(
            replace(&marks, 0, 1, 0),
            vec![m(1, 4, Bold), m(5, 7, Italic)]
        );
        assert_eq!(
            replace(&marks, 3, 7, 0),
            vec![m(2, 3, Bold), m(3, 4, Italic)]
        );
        assert_eq!(replace(&marks, 1, 9, 0), vec![]);
    }

    #[test]
    fn replacing_takes_the_formatting_of_the_first_character() {
        let marks = vec![m(2, 5, Bold), m(6, 8, Italic)];
        // "··BBB·II": replace 4..7 with 5 characters.
        assert_eq!(
            replace(&marks, 4, 7, 5),
            vec![m(2, 9, Bold), m(9, 10, Italic)]
        );
        // Replace 1..3 (first char not bold) with 1 character.
        assert_eq!(
            replace(&marks, 1, 3, 1),
            vec![m(2, 4, Bold), m(5, 7, Italic)]
        );
    }

    #[test]
    fn adjust_finds_the_edit_with_unicode() {
        let old = "café 🌾 rico";
        let marks = vec![m(5, 6, Bold), m(7, 11, Italic)];
        // Typing inside the italic word.
        assert_eq!(
            adjust(&marks, old, "café 🌾 riquísimo"),
            vec![m(5, 6, Bold), m(7, 16, Italic)]
        );
        // Deleting the emoji.
        assert_eq!(adjust(&marks, old, "café  rico"), vec![m(6, 10, Italic)]);
        // A repeated letter: the edit is found once, the marks stay right.
        assert_eq!(adjust(&[m(0, 2, Bold)], "aa", "aaa"), vec![m(0, 3, Bold)]);
        assert_eq!(adjust(&[], "a", "b"), vec![]);
        assert_eq!(adjust(&[m(0, 4, Bold)], "hola", ""), vec![]);
    }

    #[test]
    fn split_and_join_are_inverse() {
        let marks = vec![
            m(0, 3, Bold),
            m(2, 7, Italic),
            m(5, 6, link("https://a.es")),
        ];
        let (before, after) = split(&marks, 4);
        assert_eq!(before, vec![m(0, 3, Bold), m(2, 4, Italic)]);
        assert_eq!(after, vec![m(0, 3, Italic), m(1, 2, link("https://a.es"))]);
        assert_eq!(join(&before, 4, &after), marks);
        assert_eq!(split(&marks, 0).0, vec![]);
        assert_eq!(
            slice(&marks, 2, 6),
            vec![
                m(0, 1, Bold),
                m(0, 4, Italic),
                m(3, 4, link("https://a.es"))
            ]
        );
    }

    #[test]
    fn toggle_adds_unless_everything_has_it() {
        let marks = vec![m(0, 3, Bold)];
        assert_eq!(toggle(&marks, Bold, 1, 5), vec![m(0, 5, Bold)]);
        assert_eq!(
            toggle(&marks, Bold, 1, 2),
            vec![m(0, 1, Bold), m(2, 3, Bold)]
        );
        assert_eq!(
            toggle(&marks, Italic, 0, 3),
            vec![m(0, 3, Bold), m(0, 3, Italic)]
        );
        assert!(covers(&[m(0, 2, Bold), m(2, 4, Bold)], &Bold, 0, 4));
        assert!(!covers(&[m(0, 2, Bold), m(3, 4, Bold)], &Bold, 0, 4));
        assert!(!covers(&marks, &Bold, 1, 1));
        // A new link replaces the one there.
        let links = add(&[m(0, 6, link("https://a.es"))], link("https://b.es"), 2, 4);
        assert_eq!(
            links,
            vec![
                m(0, 2, link("https://a.es")),
                m(2, 4, link("https://b.es")),
                m(4, 6, link("https://a.es"))
            ]
        );
        assert_eq!(remove(&links, &link(""), 0, 6), vec![]);
    }

    #[test]
    fn replace_all_moves_marks_after_each_occurrence() {
        let text = "{{fecha}} y {{fecha}} fin";
        // Bold "y" (10..11) and italic "fin" (22..25).
        let marks = vec![m(10, 11, Bold), m(22, 25, Italic)];
        let long = "22 de octubre de 2026"; // 21 chars instead of 9
        let filled = text.replace("{{fecha}}", long);
        let out = replace_all(&marks, text, "{{fecha}}", long);
        let chars: Vec<char> = filled.chars().collect();
        let word = |mark: &Mark| chars[mark.start..mark.end].iter().collect::<String>();
        assert_eq!(word(&out[0]), "y");
        assert_eq!(word(&out[1]), "fin");
    }

    #[test]
    fn json_round_trip_and_damage() {
        let marks = vec![m(0, 3, Bold), m(1, 2, link("https://a.es/x?y=1"))];
        let json = to_json(&marks).expect("marks");
        assert_eq!(from_json(&json, 3), Ok(marks));
        assert_eq!(to_json(&[]), None);
        for damaged in [
            "no",
            "{}",
            "[1]",
            r#"[{"start":0,"end":9,"type":"bold"}]"#,
            r#"[{"start":2,"end":2,"type":"bold"}]"#,
            r#"[{"start":0,"end":1,"type":"color"}]"#,
            r#"[{"start":0,"end":1}]"#,
            r#"[{"start":-1,"end":1,"type":"bold"}]"#,
            r#"[{"start":0,"end":1,"type":"link","url":"javascript:alert(1)"}]"#,
            r#"[{"start":0,"end":1,"type":"link"}]"#,
        ] {
            assert!(from_json(damaged, 3).is_err(), "{damaged}");
        }
        // Read marks come out normalized.
        assert_eq!(
            from_json(
                r#"[{"start":2,"end":3,"type":"bold"},{"start":0,"end":2,"type":"bold"}]"#,
                3
            ),
            Ok(vec![m(0, 3, Bold)])
        );
    }

    #[test]
    fn only_web_and_mail_links_are_allowed() {
        let ok = |s: &str| normalize_url(s);
        assert_eq!(
            ok(" https://ejemplo.com "),
            Some("https://ejemplo.com".into())
        );
        assert_eq!(
            ok("HTTP://Ejemplo.com/a"),
            Some("HTTP://Ejemplo.com/a".into())
        );
        assert_eq!(
            ok("mailto:ana@ejemplo.com"),
            Some("mailto:ana@ejemplo.com".into())
        );
        assert_eq!(ok("ejemplo.com/x"), Some("https://ejemplo.com/x".into()));
        assert_eq!(
            ok("www.ejemplo.es:8080/a"),
            Some("https://www.ejemplo.es:8080/a".into())
        );
        assert_eq!(ok("señal.es"), Some("https://señal.es".into()));
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://ejemplo.com",
            "https://",
            "http:///x",
            "mailto:nadie",
            "texto suelto",
            "hola",
            "localhost:8080",
            "a.es:x",
            "https://a.es/con espacio",
            ".es",
        ] {
            assert_eq!(normalize_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn edit_range_finds_the_one_change() {
        assert_eq!(edit_range("hola", "hola"), (4, 4, 4));
        assert_eq!(edit_range("hola", "hoXla"), (2, 2, 3));
        assert_eq!(edit_range("hola", "ha"), (1, 3, 1));
        assert_eq!(edit_range("café🌾", "café🌾!"), (5, 5, 6));
        assert_eq!(edit_range("aa", "aaa"), (2, 2, 3));
    }

    #[test]
    fn typing_follows_the_marks_it_is_in() {
        let marks = vec![m(2, 5, Bold), m(5, 8, Code), m(0, 2, link("https://a.es"))];
        assert_eq!(typing_kinds(&marks, 3), vec![Bold]);
        assert_eq!(
            typing_kinds(&marks, 5),
            vec![Bold],
            "bold continues, code starts"
        );
        assert_eq!(typing_kinds(&marks, 8), vec![], "code does not continue");
        assert_eq!(typing_kinds(&marks, 2), vec![], "links do not continue");
        assert_eq!(typing_kinds(&marks, 0), vec![]);
    }

    #[test]
    fn a_typing_format_turns_on_and_off() {
        use Format as F;
        // Plain text: Cmd+B turns bold on for what comes next…
        let plain: Vec<Mark> = vec![];
        let on = TypingFormat::toggle(&plain, None, 3, F::Bold).expect("on");
        assert!(on.has(&plain, F::Bold));
        assert_eq!(on.apply(&plain, 3, 6), vec![m(3, 6, Bold)]);
        // …and again turns it back off: nothing pending.
        assert_eq!(
            TypingFormat::toggle(&plain, Some(on.clone()), 3, F::Bold),
            None
        );
        // Another place starts fresh.
        let elsewhere = TypingFormat::toggle(&plain, Some(on), 1, F::Italic).expect("on");
        assert_eq!((elsewhere.at, elsewhere.on.clone()), (1, vec![F::Italic]));

        // At the end of bold: Cmd+B stops it, the text typed is plain.
        let bold = vec![m(0, 4, Bold)];
        let off = TypingFormat::toggle(&bold, None, 4, F::Bold).expect("off");
        assert!(!off.has(&bold, F::Bold));
        let typed = replace(&bold, 4, 4, 2);
        assert_eq!(typed, vec![m(0, 6, Bold)], "bold would continue");
        assert_eq!(off.apply(&typed, 4, 6), vec![m(0, 4, Bold)]);
        // Typing goes on after the text; an IME rewrites from `from`.
        let mut typing = TypingFormat::toggle(&plain, None, 2, F::Bold).expect("on");
        assert_eq!(typing.typed(&plain, (2, 2, 4)), Some(vec![m(2, 4, Bold)]));
        assert_eq!(typing.at, 4);
        assert_eq!(typing.typed(&plain, (2, 4, 3)), Some(vec![m(2, 3, Bold)]));
        assert_eq!(typing.at, 3);
        assert_eq!(typing.typed(&plain, (0, 0, 1)), None, "somewhere else");
        // Two formats at once.
        let both = TypingFormat::toggle(&plain, None, 0, F::Italic);
        let both = TypingFormat::toggle(&plain, both, 0, F::Code).expect("both");
        assert_eq!(
            both.apply(&plain, 0, 2),
            vec![m(0, 2, Italic), m(0, 2, Code)]
        );
    }

    /// Small deterministic generator (no new crates).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    fn is_normal(marks: &[Mark], len: usize) -> bool {
        let mut copy = marks.to_vec();
        normalize(&mut copy, len);
        copy == marks && marks.iter().all(|m| m.start < m.end && m.end <= len)
    }

    #[test]
    fn random_edits_keep_marks_normal_and_inside_the_text() {
        let kinds = [
            Bold,
            Italic,
            Code,
            Strike,
            link("https://a.es"),
            link("https://b.es"),
        ];
        let alphabet: Vec<char> = "ab ñ🌾\n".chars().collect();
        let mut rng = Rng(0x5eed_1234_abcd_ef01);
        for _ in 0..3000 {
            let len = rng.next(12);
            let mut text: String = (0..len)
                .map(|_| alphabet[rng.next(alphabet.len())])
                .collect();
            let mut marks = Vec::new();
            for _ in 0..rng.next(5) {
                if len == 0 {
                    break;
                }
                let s = rng.next(len);
                let e = s + 1 + rng.next(len - s);
                marks = add(&marks, kinds[rng.next(kinds.len())].clone(), s, e);
            }
            assert!(is_normal(&marks, len));
            for _ in 0..4 {
                let chars: Vec<char> = text.chars().collect();
                let s = rng.next(chars.len() + 1);
                let e = s + rng.next(chars.len() - s + 1);
                let insert: String = (0..rng.next(4))
                    .map(|_| alphabet[rng.next(alphabet.len())])
                    .collect();
                let new: String = chars[..s]
                    .iter()
                    .chain(insert.chars().collect::<Vec<_>>().iter())
                    .chain(chars[e..].iter())
                    .collect();
                let new_len = new.chars().count();
                let by_range = replace(&marks, s, e, insert.chars().count());
                assert!(
                    is_normal(&by_range, new_len),
                    "{marks:?} {s} {e} {insert:?}"
                );
                let by_diff = adjust(&marks, &text, &new);
                assert!(is_normal(&by_diff, new_len));
                // Text outside the edit keeps its formatting.
                let kept_before = slice(&marks, 0, s);
                assert_eq!(slice(&by_range, 0, s), kept_before);
                let (_, tail) = split(&marks, e);
                let new_tail_start = s + insert.chars().count();
                assert_eq!(split(&by_range, new_tail_start).1, tail);
                // Split + join gives the same marks back.
                let at = rng.next(new_len + 1);
                let (a, b) = split(&by_diff, at);
                assert_eq!(join(&a, at, &b), by_diff);
                marks = by_diff;
                text = new;
            }
        }
    }

    #[test]
    fn link_at_finds_the_link_the_cursor_is_in_or_touches() {
        let marks = vec![
            m(0, 3, Bold),
            m(2, 5, link("https://a.es")),
            m(5, 8, link("https://b.es")),
        ];
        assert_eq!(link_at(&marks, 0), None);
        assert_eq!(link_at(&marks, 2).map(|l| l.end), Some(5));
        assert_eq!(link_at(&marks, 4).map(|l| l.end), Some(5));
        // Between two links: the one that starts there.
        assert_eq!(link_at(&marks, 5).map(|l| l.start), Some(5));
        assert_eq!(link_at(&marks, 8).map(|l| l.start), Some(5));
        assert_eq!(link_at(&marks, 9), None);
        assert_eq!(link_at(&[], 0), None);
    }

    #[test]
    fn link_over_is_the_link_on_a_character() {
        let marks = vec![m(0, 2, Italic), m(2, 4, link("https://a.es"))];
        assert_eq!(link_over(&marks, 1), None);
        assert_eq!(link_over(&marks, 2).map(|l| l.start), Some(2));
        assert_eq!(link_over(&marks, 3).map(|l| l.start), Some(2));
        assert_eq!(link_over(&marks, 4), None, "end is outside");
    }

    #[test]
    fn link_url_in_is_the_first_link_in_a_range() {
        let marks = vec![
            m(0, 4, Bold),
            m(4, 6, link("https://a.es")),
            m(8, 9, link("https://b.es")),
        ];
        assert_eq!(link_url_in(&marks, 0, 4), None);
        assert_eq!(link_url_in(&marks, 0, 5), Some("https://a.es"));
        assert_eq!(link_url_in(&marks, 6, 9), Some("https://b.es"));
        assert_eq!(link_url_in(&marks, 6, 8), None);
    }

    #[test]
    fn set_link_adds_replaces_removes_or_refuses() {
        let marks = vec![m(0, 3, Bold), m(4, 8, link("https://a.es"))];
        // A plain address gets https.
        assert_eq!(
            set_link(&marks, 0, 3, " ejemplo.com/x "),
            Some(vec![
                m(0, 3, Bold),
                m(0, 3, link("https://ejemplo.com/x")),
                m(4, 8, link("https://a.es")),
            ])
        );
        // Over part of a link: that part points elsewhere.
        assert_eq!(
            set_link(&marks, 6, 8, "mailto:ana@ejemplo.com"),
            Some(vec![
                m(0, 3, Bold),
                m(4, 6, link("https://a.es")),
                m(6, 8, link("mailto:ana@ejemplo.com")),
            ])
        );
        // Nothing typed: the link goes, and the rest stays.
        assert_eq!(set_link(&marks, 4, 8, "  "), Some(vec![m(0, 3, Bold)]));
        assert_eq!(
            set_link(&marks, 5, 6, ""),
            Some(vec![
                m(0, 3, Bold),
                m(4, 5, link("https://a.es")),
                m(6, 8, link("https://a.es")),
            ])
        );
        // Not allowed: nothing changes.
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://a.es",
            "hola mundo",
        ] {
            assert_eq!(set_link(&marks, 0, 3, url), None, "{url}");
        }
        // An empty range never gets a link.
        assert_eq!(set_link(&marks, 2, 2, "a.es"), Some(marks.clone()));
    }
}
