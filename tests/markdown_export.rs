//! Export: golden files and a structural check with a real CommonMark parser.
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use silex_desktop::markdown::page_to_markdown;
use silex_desktop::model::{Block, BlockKind};

fn b(kind: BlockKind, indent: u8, content: &str) -> Block {
    Block {
        id: String::new(),
        page_id: "p".into(),
        kind,
        content: content.into(),
        checked: false,
        position: 0,
        indent,
        attachment: None,
    }
}

fn todo(indent: u8, content: &str, checked: bool) -> Block {
    Block {
        checked,
        ..b(BlockKind::Todo, indent, content)
    }
}

use BlockKind::*;

/// A page touching every block type, nesting and the escaping rules.
pub fn sample() -> Vec<Block> {
    vec![
        b(Heading1, 0, "Introducción"),
        b(
            Paragraph,
            0,
            "Texto con *asteriscos*, _guiones_ y `código` literal.",
        ),
        b(Paragraph, 0, "Línea uno\nLínea dos"),
        b(Heading2, 0, "Lista de la compra #2"),
        b(Bulleted, 0, "Pan"),
        b(Bulleted, 1, "Integral"),
        b(Bulleted, 1, "Centeno"),
        b(Numbered, 2, "Paso [uno]"),
        b(Bulleted, 0, "Leche"),
        b(Heading3, 0, "Tareas"),
        todo(0, "Pagar facturas", true),
        todo(0, "Llamar & confirmar", false),
        todo(1, "Sub-tarea", false),
        b(Numbered, 0, "Primero"),
        b(Numbered, 0, "Segundo"),
        b(Paragraph, 1, "Detalle del segundo"),
        b(Quote, 0, "Una cita\ncon dos líneas"),
        b(Code, 0, "fn main() {\n    println!(\"```\");\n}"),
        b(Paragraph, 0, "# no es un título"),
        b(Paragraph, 0, "- no es una lista"),
        b(Paragraph, 0, "1. no es una lista numerada"),
        b(Paragraph, 0, "---"),
        b(Paragraph, 0, "    cuatro espacios y fin  "),
        b(Paragraph, 0, ""),
        b(Paragraph, 0, "C:\\ruta\\archivo <html> ~tachado~ | tabla !"),
        b(Paragraph, 0, "日本語 🙂 e\u{301}"),
    ]
}

/// What a CommonMark reader sees, flattened: enough to check the export.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Heading(u8, String),
    Paragraph {
        depth: usize,
        text: String,
    },
    Item {
        ordered: bool,
        depth: usize,
        checked: Option<bool>,
        text: String,
    },
    Quote(String),
    Code(String),
}

/// Parses Markdown with pulldown-cmark (CommonMark + task lists) and
/// flattens it. Text, escapes and character references are decoded by the
/// parser; hard breaks become "\n".
fn read_back(markdown: &str) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut lists: Vec<bool> = Vec::new(); // ordered?
    // Open item waiting for its first text: (ordered, depth, checked, text).
    let mut item: Option<(bool, usize, Option<bool>, String)> = None;
    let mut text = String::new();
    let mut in_quote = false;
    let mut in_paragraph = false;
    let flush_item = |item: &mut Option<(bool, usize, Option<bool>, String)>,
                      seen: &mut Vec<Seen>| {
        if let Some((ordered, depth, checked, text)) = item.take() {
            seen.push(Seen::Item {
                ordered,
                depth,
                checked,
                text,
            });
        }
    };
    for event in Parser::new_ext(markdown, Options::ENABLE_TASKLISTS) {
        match event {
            Event::Start(Tag::List(start)) => {
                flush_item(&mut item, &mut seen);
                lists.push(start.is_some());
            }
            Event::End(TagEnd::List(_)) => {
                flush_item(&mut item, &mut seen);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush_item(&mut item, &mut seen);
                let ordered = lists.last().copied().unwrap_or(false);
                item = Some((ordered, lists.len() - 1, None, String::new()));
            }
            Event::End(TagEnd::Item) => flush_item(&mut item, &mut seen),
            Event::TaskListMarker(checked) => {
                if let Some(open) = item.as_mut() {
                    open.2 = Some(checked);
                }
            }
            Event::Start(Tag::Paragraph) => {
                in_paragraph = true;
                text.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                in_paragraph = false;
                let done = std::mem::take(&mut text);
                if let Some(open) = item.as_mut().filter(|i| i.3.is_empty()) {
                    open.3 = done; // the item's own text (loose list)
                    flush_item(&mut item, &mut seen);
                } else if in_quote {
                    seen.push(Seen::Quote(done));
                } else {
                    seen.push(Seen::Paragraph {
                        depth: lists.len(),
                        text: done,
                    });
                }
            }
            Event::Start(Tag::Heading { .. }) | Event::Start(Tag::CodeBlock(_)) => {
                flush_item(&mut item, &mut seen);
                text.clear();
            }
            Event::End(TagEnd::Heading(level)) => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
                seen.push(Seen::Heading(n, std::mem::take(&mut text)));
            }
            Event::End(TagEnd::CodeBlock) => {
                let code = std::mem::take(&mut text);
                seen.push(Seen::Code(
                    code.strip_suffix('\n').unwrap_or(&code).to_owned(),
                ));
            }
            Event::Start(Tag::BlockQuote(_)) => in_quote = true,
            Event::End(TagEnd::BlockQuote(_)) => in_quote = false,
            Event::Text(t) => {
                if in_paragraph || item.is_none() {
                    text.push_str(&t);
                } else if let Some(open) = item.as_mut() {
                    open.3.push_str(&t); // tight list: text directly in the item
                }
            }
            Event::HardBreak => {
                if in_paragraph || item.is_none() {
                    text.push('\n');
                } else if let Some(open) = item.as_mut() {
                    open.3.push('\n');
                }
            }
            other => panic!("unexpected event in exported Markdown: {other:?}"),
        }
    }
    seen
}

#[test]
fn exported_markdown_reads_back_as_the_same_structure_and_text() {
    let markdown = page_to_markdown("Mi página *especial*", &sample());
    let item = |ordered, depth, checked, text: &str| Seen::Item {
        ordered,
        depth,
        checked,
        text: text.into(),
    };
    let para = |depth, text: &str| Seen::Paragraph {
        depth,
        text: text.into(),
    };
    assert_eq!(
        read_back(&markdown),
        vec![
            Seen::Heading(1, "Mi página *especial*".into()),
            Seen::Heading(1, "Introducción".into()),
            para(0, "Texto con *asteriscos*, _guiones_ y `código` literal."),
            para(0, "Línea uno\nLínea dos"),
            Seen::Heading(2, "Lista de la compra #2".into()),
            item(false, 0, None, "Pan"),
            item(false, 1, None, "Integral"),
            item(false, 1, None, "Centeno"),
            item(true, 2, None, "Paso [uno]"),
            item(false, 0, None, "Leche"),
            Seen::Heading(3, "Tareas".into()),
            item(false, 0, Some(true), "Pagar facturas"),
            item(false, 0, Some(false), "Llamar & confirmar"),
            item(false, 1, Some(false), "Sub-tarea"),
            item(true, 0, None, "Primero"),
            item(true, 0, None, "Segundo"),
            para(1, "Detalle del segundo"),
            Seen::Quote("Una cita\ncon dos líneas".into()),
            Seen::Code("fn main() {\n    println!(\"```\");\n}".into()),
            para(0, "# no es un título"),
            para(0, "- no es una lista"),
            para(0, "1. no es una lista numerada"),
            para(0, "---"),
            para(0, "    cuatro espacios y fin  "),
            para(0, "C:\\ruta\\archivo <html> ~tachado~ | tabla !"),
            para(0, "日本語 🙂 e\u{301}"),
        ]
    );
}

#[test]
fn export_matches_the_golden_file() {
    let markdown = page_to_markdown("Mi página *especial*", &sample());
    let golden = include_str!("golden/export_sample.md");
    if markdown != golden {
        let out = std::env::temp_dir().join("silex_export_sample.actual.md");
        let _ = std::fs::write(&out, &markdown);
        panic!("export changed; actual output written to {}", out.display());
    }
}

#[test]
fn every_ascii_punctuation_survives_in_every_block_type() {
    let punctuation: String = (33u8..127)
        .map(char::from)
        .filter(char::is_ascii_punctuation)
        .collect();
    for kind in [Paragraph, Heading2, Bulleted, Numbered, Quote] {
        for text in [
            punctuation.clone(),
            format!("{punctuation} {punctuation}"),
            format!("x{punctuation}"),
        ] {
            let seen = read_back(&page_to_markdown("t", &[b(kind, 0, &text)]));
            let got = match &seen[1] {
                Seen::Heading(_, t) | Seen::Quote(t) => t.clone(),
                Seen::Paragraph { text, .. } | Seen::Item { text, .. } => text.clone(),
                Seen::Code(t) => t.clone(),
            };
            assert_eq!(got, text, "{kind:?}: {seen:?}");
        }
    }
}
