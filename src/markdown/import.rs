//! CommonMark (+ GFM task lists, strikethrough and alerts) → blocks. The
//! language of fenced code blocks is not stored (Silex code blocks have no
//! language), nor the type of an alert (all are callouts).
use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use crate::marks::{self, Mark, MarkKind};
use crate::model::{BlockKind, MAX_INDENT, NewBlock};

/// A page read from Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPage {
    pub title: String,
    pub blocks: Vec<NewBlock>,
}

/// Where the text being collected will go.
enum Target {
    /// Text of the open list item (index into the output).
    Item(usize),
    Paragraph,
    Heading(BlockKind),
    Code,
    Html,
}

/// Text being collected, with its formatting.
#[derive(Default)]
struct Inline {
    text: String,
    marks: Vec<Mark>,
}

/// Inline syntax opened and not closed yet.
enum Open {
    /// Becomes a mark from this character on (ADR 0015).
    Mark(MarkKind, usize),
    /// Kept as literal Markdown: written when it closes (images, links
    /// to something that is not a web or mail address).
    Literal(String),
}

/// Parses Markdown into a page.
///
/// - `#`/`##`/`###` → headings (deeper levels become level 3); a first
///   `# Title` becomes the page title, otherwise `fallback_title` is used.
/// - Lists → list blocks with indent = nesting; `- [ ]`/`- [x]` → tasks.
/// - Quotes → quote blocks; code blocks (fenced or indented) → code.
/// - Emphasis, strong emphasis, strikethrough, inline code and links to
///   `http`, `https` or `mailto` become inline formatting (ADR 0015).
///   Other links (relative paths, other schemes), images and HTML are kept
///   as literal Markdown text, so nothing is lost.
/// - Line breaks inside a paragraph (soft or hard) become `\n`.
/// - A thematic break becomes a divider (H12.2); HTML blocks become
///   paragraphs with their raw text.
/// - A GitHub alert (`> [!NOTE]`, `> [!TIP]`…) or an Obsidian callout
///   (`> [!info]` on a line of its own) becomes callouts (H12.3).
pub fn markdown_to_page(source: &str, fallback_title: &str) -> ImportedPage {
    let source = source.trim_start_matches('\u{feff}');
    let mut blocks: Vec<NewBlock> = Vec::new();
    let mut lists: Vec<bool> = Vec::new(); // ordered?
    let mut open_item: Option<usize> = None;
    let mut target: Option<Target> = None;
    let mut inline = Inline::default();
    let mut quote_depth = 0usize;
    // Per open quote: whether it is a callout.
    let mut callouts: Vec<bool> = Vec::new();
    let mut open: Vec<Open> = Vec::new();

    let depth = |lists: &Vec<bool>| {
        u8::try_from(lists.len())
            .unwrap_or(MAX_INDENT)
            .min(MAX_INDENT)
    };
    let push = |blocks: &mut Vec<NewBlock>, kind, indent, inline: Inline| {
        let marks = if kind == BlockKind::Code {
            Vec::new()
        } else {
            marks::sanitize(&inline.marks, &inline.text)
        };
        blocks.push(NewBlock {
            kind,
            indent,
            checked: false,
            content: inline.text,
            marks,
        });
    };
    let options = Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_GFM;

    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::List(start)) => {
                open_item = None; // a nested list ends the item's own text
                lists.push(start.is_some());
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let ordered = lists.last().copied().unwrap_or(false);
                let kind = if ordered {
                    BlockKind::Numbered
                } else {
                    BlockKind::Bulleted
                };
                let indent = depth(&lists).saturating_sub(1);
                push(&mut blocks, kind, indent, Inline::default());
                open_item = Some(blocks.len() - 1);
            }
            Event::End(TagEnd::Item) => open_item = None,
            Event::TaskListMarker(checked) => {
                if let Some(block) = open_item.and_then(|i| blocks.get_mut(i)) {
                    block.kind = BlockKind::Todo;
                    block.checked = checked;
                }
            }
            Event::Start(Tag::Paragraph) => {
                inline = Inline::default();
                target = Some(match open_item {
                    Some(i) if blocks[i].content.is_empty() => Target::Item(i),
                    _ => Target::Paragraph,
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                open_item = None;
                inline = Inline::default();
                target = Some(Target::Heading(match level {
                    HeadingLevel::H1 => BlockKind::Heading1,
                    HeadingLevel::H2 => BlockKind::Heading2,
                    _ => BlockKind::Heading3,
                }));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                open_item = None;
                inline = Inline::default();
                target = Some(Target::Code);
            }
            Event::Start(Tag::HtmlBlock) => {
                open_item = None;
                inline = Inline::default();
                target = Some(Target::Html);
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::HtmlBlock,
            ) => {
                let mut content = std::mem::take(&mut inline);
                let indent = depth(&lists);
                match target.take() {
                    Some(Target::Item(i)) => {
                        let marks = marks::sanitize(&content.marks, &content.text);
                        blocks[i].content = content.text;
                        blocks[i].marks = marks;
                        open_item = None;
                    }
                    Some(Target::Paragraph) => {
                        let kind = if quote_depth == 0 {
                            BlockKind::Paragraph
                        } else if callouts.last() == Some(&true) {
                            BlockKind::Callout
                        } else if let Some(rest) = obsidian_callout(&content) {
                            // The marker makes the whole quote a callout.
                            if let Some(last) = callouts.last_mut() {
                                *last = true;
                            }
                            match rest {
                                Some(rest) => content = rest,
                                None => continue,
                            }
                            BlockKind::Callout
                        } else {
                            BlockKind::Quote
                        };
                        push(&mut blocks, kind, indent, content);
                    }
                    Some(Target::Heading(kind)) => push(&mut blocks, kind, indent, content),
                    Some(Target::Code) => {
                        if content.text.ends_with('\n') {
                            content.text.pop();
                        }
                        push(&mut blocks, BlockKind::Code, indent, content);
                    }
                    Some(Target::Html) => {
                        content
                            .text
                            .truncate(content.text.trim_end_matches('\n').len());
                        content.marks.clear();
                        push(&mut blocks, BlockKind::Paragraph, indent, content);
                    }
                    None => {}
                }
            }
            Event::Start(Tag::BlockQuote(alert)) => {
                quote_depth += 1;
                callouts.push(alert.is_some());
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                quote_depth = quote_depth.saturating_sub(1);
                callouts.pop();
            }
            Event::Rule => {
                open_item = None;
                push(
                    &mut blocks,
                    BlockKind::Divider,
                    depth(&lists),
                    Inline::default(),
                );
            }

            // Inline content.
            Event::Text(t) => {
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    dest.push(&t);
                }
            }
            Event::Code(code) => {
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    let start = dest.len();
                    dest.push(&code);
                    let end = dest.len();
                    dest.marks_mut().push(Mark::new(start, end, MarkKind::Code));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    dest.push("\n");
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    dest.push(&html);
                }
            }
            Event::Start(Tag::Emphasis) => {
                open_mark(
                    &mut blocks,
                    &target,
                    open_item,
                    &mut inline,
                    &mut open,
                    MarkKind::Italic,
                );
            }
            Event::Start(Tag::Strong) => {
                open_mark(
                    &mut blocks,
                    &target,
                    open_item,
                    &mut inline,
                    &mut open,
                    MarkKind::Bold,
                );
            }
            Event::Start(Tag::Strikethrough) => {
                open_mark(
                    &mut blocks,
                    &target,
                    open_item,
                    &mut inline,
                    &mut open,
                    MarkKind::Strike,
                );
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) => match web_url(link_type, &dest_url) {
                Some(url) => open_mark(
                    &mut blocks,
                    &target,
                    open_item,
                    &mut inline,
                    &mut open,
                    MarkKind::Link(url),
                ),
                None => {
                    let autolink = is_autolink(link_type);
                    if let Some(mut dest) =
                        destination(&mut blocks, &target, open_item, &mut inline)
                    {
                        dest.push(if autolink { "<" } else { "[" });
                    }
                    open.push(Open::Literal(closing(autolink, &dest_url, &title)));
                }
            },
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    dest.push("![");
                }
                open.push(Open::Literal(closing(
                    is_autolink(link_type),
                    &dest_url,
                    &title,
                )));
            }
            Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image,
            ) => {
                let Some(opened) = open.pop() else {
                    continue;
                };
                if let Some(mut dest) = destination(&mut blocks, &target, open_item, &mut inline) {
                    match opened {
                        Open::Mark(kind, start) => {
                            let end = dest.len();
                            dest.marks_mut().push(Mark::new(start, end, kind));
                        }
                        Open::Literal(text) => dest.push(&text),
                    }
                }
            }
            _ => {}
        }
    }

    let title = match blocks.first() {
        Some(first) if first.kind == BlockKind::Heading1 && first.indent == 0 => {
            blocks.remove(0).content
        }
        _ => fallback_title.to_owned(),
    };
    ImportedPage { title, blocks }
}

/// An Obsidian callout marker (`[!info]`, `[!warning]`…) on the first
/// line of a quote's paragraph: `Some` with the text after it (`None` if
/// there is none), or `None` when it is not one.
fn obsidian_callout(content: &Inline) -> Option<Option<Inline>> {
    let rest = content.text.strip_prefix("[!")?;
    let close = rest.find(']')?;
    let name = &rest[..close];
    let after = &rest[close + 1..];
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || !(after.is_empty() || after.starts_with('\n'))
    {
        return None;
    }
    let skip = "[!".len() + close + 1 + usize::from(after.starts_with('\n'));
    let removed = content.text[..skip].chars().count();
    let text = content.text[skip..].to_owned();
    if text.is_empty() {
        return Some(None);
    }
    let marks = marks::replace(&content.marks, 0, removed, 0);
    Some(Some(Inline { text, marks }))
}

fn is_autolink(link_type: LinkType) -> bool {
    matches!(link_type, LinkType::Autolink | LinkType::Email)
}

/// The URL of a link that becomes a mark: one written with `http`,
/// `https` or `mailto` (an `<a@b.es>` autolink is a `mailto`). A relative
/// path like `nota.md` is not, even if it looks like a host name.
fn web_url(link_type: LinkType, dest: &str) -> Option<String> {
    let url = if link_type == LinkType::Email && !dest.to_ascii_lowercase().starts_with("mailto:") {
        format!("mailto:{dest}")
    } else {
        dest.to_owned()
    };
    (marks::normalize_url(&url).as_deref() == Some(url.as_str())).then_some(url)
}

/// How a link kept as literal text ends.
fn closing(autolink: bool, dest: &str, title: &str) -> String {
    if autolink {
        ">".to_owned()
    } else if title.is_empty() {
        format!("]({dest})")
    } else {
        format!("]({dest} \"{title}\")")
    }
}

/// Where inline text goes: the text being collected, or straight the open
/// list item (tight lists have no paragraph inside items).
enum Destination<'a> {
    Inline(&'a mut Inline),
    Item(&'a mut NewBlock),
}

impl Destination<'_> {
    fn push(&mut self, piece: &str) {
        match self {
            Destination::Inline(inline) => inline.text.push_str(piece),
            Destination::Item(block) => block.content.push_str(piece),
        }
    }

    /// Length in characters.
    fn len(&self) -> usize {
        match self {
            Destination::Inline(inline) => inline.text.chars().count(),
            Destination::Item(block) => block.content.chars().count(),
        }
    }

    fn marks_mut(&mut self) -> &mut Vec<Mark> {
        match self {
            Destination::Inline(inline) => &mut inline.marks,
            Destination::Item(block) => &mut block.marks,
        }
    }
}

fn destination<'a>(
    blocks: &'a mut [NewBlock],
    target: &Option<Target>,
    open_item: Option<usize>,
    inline: &'a mut Inline,
) -> Option<Destination<'a>> {
    if target.is_some() {
        Some(Destination::Inline(inline))
    } else {
        open_item
            .and_then(|i| blocks.get_mut(i))
            .map(Destination::Item)
    }
}

fn open_mark(
    blocks: &mut [NewBlock],
    target: &Option<Target>,
    open_item: Option<usize>,
    inline: &mut Inline,
    open: &mut Vec<Open>,
    kind: MarkKind,
) {
    let start = destination(blocks, target, open_item, inline).map_or(0, |d| d.len());
    open.push(Open::Mark(kind, start));
}
