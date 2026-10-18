//! CommonMark (+ GFM task lists) → blocks. The language of fenced code
//! blocks is not stored (Silex code blocks have no language).
use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

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

/// Parses Markdown into a page.
///
/// - `#`/`##`/`###` → headings (deeper levels become level 3); a first
///   `# Title` becomes the page title, otherwise `fallback_title` is used.
/// - Lists → list blocks with indent = nesting; `- [ ]`/`- [x]` → tasks.
/// - Quotes → quote blocks; code blocks (fenced or indented) → code.
/// - Inline formatting Silex does not have (emphasis, links, inline code,
///   images, HTML) is kept as literal Markdown text, so nothing is lost.
/// - Line breaks inside a paragraph (soft or hard) become `\n`.
/// - A thematic break becomes a `---` paragraph; HTML blocks become paragraphs
///   with their raw text.
pub fn markdown_to_page(source: &str, fallback_title: &str) -> ImportedPage {
    let source = source.trim_start_matches('\u{feff}');
    let mut blocks: Vec<NewBlock> = Vec::new();
    let mut lists: Vec<bool> = Vec::new(); // ordered?
    let mut open_item: Option<usize> = None;
    let mut target: Option<Target> = None;
    let mut text = String::new();
    let mut quote_depth = 0usize;
    let mut links: Vec<(LinkType, String, String)> = Vec::new();

    let depth = |lists: &Vec<bool>| {
        u8::try_from(lists.len())
            .unwrap_or(MAX_INDENT)
            .min(MAX_INDENT)
    };
    let push = |blocks: &mut Vec<NewBlock>, kind, indent, content: String| {
        blocks.push(NewBlock {
            kind,
            indent,
            checked: false,
            content,
        });
    };

    for event in Parser::new_ext(source, Options::ENABLE_TASKLISTS) {
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
                push(&mut blocks, kind, indent, String::new());
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
                text.clear();
                target = Some(match open_item {
                    Some(i) if blocks[i].content.is_empty() => Target::Item(i),
                    _ => Target::Paragraph,
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                open_item = None;
                text.clear();
                target = Some(Target::Heading(match level {
                    HeadingLevel::H1 => BlockKind::Heading1,
                    HeadingLevel::H2 => BlockKind::Heading2,
                    _ => BlockKind::Heading3,
                }));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                open_item = None;
                text.clear();
                target = Some(Target::Code);
            }
            Event::Start(Tag::HtmlBlock) => {
                open_item = None;
                text.clear();
                target = Some(Target::Html);
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::HtmlBlock,
            ) => {
                let content = std::mem::take(&mut text);
                let indent = depth(&lists);
                match target.take() {
                    Some(Target::Item(i)) => {
                        blocks[i].content = content;
                        open_item = None;
                    }
                    Some(Target::Paragraph) => {
                        let kind = if quote_depth > 0 {
                            BlockKind::Quote
                        } else {
                            BlockKind::Paragraph
                        };
                        push(&mut blocks, kind, indent, content);
                    }
                    Some(Target::Heading(kind)) => push(&mut blocks, kind, indent, content),
                    Some(Target::Code) => {
                        let code = content.strip_suffix('\n').unwrap_or(&content).to_owned();
                        push(&mut blocks, BlockKind::Code, indent, code);
                    }
                    Some(Target::Html) => {
                        let html = content.trim_end_matches('\n').to_owned();
                        push(&mut blocks, BlockKind::Paragraph, indent, html);
                    }
                    None => {}
                }
            }
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            Event::Rule => {
                open_item = None;
                push(
                    &mut blocks,
                    BlockKind::Paragraph,
                    depth(&lists),
                    "---".into(),
                );
            }

            // Inline content.
            Event::Text(t) => append(&mut blocks, &target, open_item, &mut text, &t),
            Event::Code(code) => {
                let fence = "`".repeat(longest_run(&code, '`') + 1);
                let pad = if code.starts_with('`') || code.ends_with('`') {
                    " "
                } else {
                    ""
                };
                let literal = format!("{fence}{pad}{code}{pad}{fence}");
                append(&mut blocks, &target, open_item, &mut text, &literal);
            }
            Event::SoftBreak | Event::HardBreak => {
                append(&mut blocks, &target, open_item, &mut text, "\n");
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                append(&mut blocks, &target, open_item, &mut text, &html);
            }
            Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => {
                append(&mut blocks, &target, open_item, &mut text, "*");
            }
            Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => {
                append(&mut blocks, &target, open_item, &mut text, "**");
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                let opening = if is_autolink(link_type) { "<" } else { "[" };
                append(&mut blocks, &target, open_item, &mut text, opening);
                links.push((link_type, dest_url.to_string(), title.to_string()));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                append(&mut blocks, &target, open_item, &mut text, "![");
                links.push((link_type, dest_url.to_string(), title.to_string()));
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if let Some((link_type, dest, title)) = links.pop() {
                    let closing = if is_autolink(link_type) {
                        ">".to_owned()
                    } else if title.is_empty() {
                        format!("]({dest})")
                    } else {
                        format!("]({dest} \"{title}\")")
                    };
                    append(&mut blocks, &target, open_item, &mut text, &closing);
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

fn is_autolink(link_type: LinkType) -> bool {
    matches!(link_type, LinkType::Autolink | LinkType::Email)
}

fn longest_run(text: &str, c: char) -> usize {
    text.split(|x| x != c).map(str::len).max().unwrap_or(0)
}

/// Inline text goes to the block being collected, or straight into the open
/// list item (tight lists have no paragraph inside items).
fn append(
    blocks: &mut [crate::model::NewBlock],
    target: &Option<Target>,
    open_item: Option<usize>,
    text: &mut String,
    piece: &str,
) {
    if target.is_some() {
        text.push_str(piece);
    } else if let Some(block) = open_item.and_then(|i| blocks.get_mut(i)) {
        block.content.push_str(piece);
    }
}
