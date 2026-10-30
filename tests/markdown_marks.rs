//! Inline formatting in Markdown (H12.1e, ADR 0015): what export writes,
//! what import reads, and round trips (also with random text and marks).
use silex_desktop::markdown::{markdown_to_page, page_to_markdown};
use silex_desktop::marks::{Mark, MarkKind};
use silex_desktop::model::{Block, BlockKind, NewBlock};

/// A block with `marks` in normal form (as the app keeps them).
fn block(kind: BlockKind, content: &str, mut marks: Vec<Mark>) -> Block {
    silex_desktop::marks::normalize(&mut marks, content.chars().count());
    Block {
        id: String::new(),
        page_id: String::new(),
        kind,
        content: content.to_owned(),
        checked: false,
        position: 0,
        indent: 0,
        attachment: None,
        link: None,
        marks,
    }
}

fn m(start: usize, end: usize, kind: MarkKind) -> Mark {
    Mark::new(start, end, kind)
}

fn link(url: &str) -> MarkKind {
    MarkKind::Link(url.to_owned())
}

/// The Markdown of one block (without the page title).
fn body(blocks: &[Block]) -> String {
    let markdown = page_to_markdown("t", blocks);
    markdown
        .strip_prefix("# t\n\n")
        .unwrap_or(&markdown)
        .to_owned()
}

/// Export, then import: the blocks that come back.
fn round_trip(blocks: &[Block]) -> Vec<NewBlock> {
    markdown_to_page(&page_to_markdown("t", blocks), "x").blocks
}

fn same(blocks: &[Block], back: &[NewBlock]) -> bool {
    blocks.len() == back.len()
        && blocks
            .iter()
            .zip(back)
            .all(|(a, b)| a.kind == b.kind && a.content == b.content && a.marks == b.marks)
}

#[test]
fn export_writes_each_kind_of_formatting() {
    let text = "Texto en negrita, cursiva, código, tachado y enlace.";
    let marks = vec![
        m(9, 16, MarkKind::Bold),
        m(18, 25, MarkKind::Italic),
        m(27, 33, MarkKind::Code),
        m(35, 42, MarkKind::Strike),
        m(45, 51, link("https://ejemplo.com/a_b")),
    ];
    assert_eq!(
        body(&[block(BlockKind::Paragraph, text, marks)]),
        "Texto en **negrita**, *cursiva*, `código`, ~~tachado~~ y \
         [enlace](https://ejemplo.com/a_b).\n"
    );
}

#[test]
fn overlapping_marks_close_and_open_in_order() {
    // «uno dos tres»: bold «uno dos», italic «dos tres».
    let marks = vec![m(0, 7, MarkKind::Bold), m(4, 12, MarkKind::Italic)];
    let blocks = [block(BlockKind::Paragraph, "uno dos tres", marks)];
    assert_eq!(body(&blocks), "**uno *dos*** *tres*\n");
    // The space between «dos» and «tres» is not italic: a mark cannot
    // open before a space, and it looks the same.
    assert_eq!(
        round_trip(&blocks)[0].marks,
        vec![
            m(0, 7, MarkKind::Bold),
            m(4, 7, MarkKind::Italic),
            m(8, 12, MarkKind::Italic),
        ]
    );
}

#[test]
fn formatting_round_trips_exactly_in_every_text_block() {
    use BlockKind::*;
    let cases: Vec<(&str, Vec<Mark>)> = vec![
        // Bold and italic together, and a link with bold inside.
        (
            "café con leche y más",
            vec![
                m(0, 4, MarkKind::Bold),
                m(0, 4, MarkKind::Italic),
                m(9, 14, link("https://a.es/x?y=1#z")),
                m(9, 14, MarkKind::Bold),
            ],
        ),
        // Code with backticks, stars and spaces inside; Unicode.
        (
            "usa `x` o *y* aquí 🌾 日本",
            vec![m(4, 7, MarkKind::Code), m(10, 13, MarkKind::Code)],
        ),
        (" a ", vec![m(0, 3, MarkKind::Code)]),
        // Formatting over a line break.
        (
            "primera línea\nsegunda línea",
            vec![
                m(8, 21, MarkKind::Bold),
                m(0, 7, link("mailto:ana@ejemplo.com")),
            ],
        ),
        // Characters Markdown would read: escaped, also inside marks.
        (
            "precio *final* [sic] #1 & <b>",
            vec![m(7, 14, MarkKind::Strike), m(15, 20, MarkKind::Italic)],
        ),
        // A URL with parentheses.
        (
            "Wikipedia",
            vec![m(
                0,
                9,
                link("https://es.wikipedia.org/wiki/Rust_(lenguaje)"),
            )],
        ),
    ];
    for kind in [
        Paragraph, Heading1, Heading2, Heading3, Bulleted, Numbered, Todo, Quote,
    ] {
        for (text, marks) in &cases {
            if kind.is_heading() && text.contains('\n') {
                continue;
            }
            let blocks = vec![
                block(Paragraph, "antes", Vec::new()),
                block(kind, text, marks.clone()),
            ];
            let back = round_trip(&blocks);
            assert!(
                same(&blocks, &back),
                "{kind:?} {text:?}\n{}\n{back:#?}",
                page_to_markdown("t", &blocks)
            );
        }
    }
}

#[test]
fn spaces_at_the_edges_of_bold_italic_and_strike_stay_outside() {
    let blocks = [block(
        BlockKind::Paragraph,
        "uno  dos  tres",
        vec![m(3, 10, MarkKind::Bold)],
    )];
    assert_eq!(body(&blocks), "uno  **dos**  tres\n");
    assert_eq!(round_trip(&blocks)[0].marks, vec![m(5, 8, MarkKind::Bold)]);
}

#[test]
fn inline_code_is_cut_at_line_breaks() {
    let blocks = [block(
        BlockKind::Paragraph,
        "ab\ncd",
        vec![m(0, 5, MarkKind::Code)],
    )];
    assert_eq!(body(&blocks), "`ab`\\\n`cd`\n");
    assert_eq!(
        round_trip(&blocks)[0].marks,
        vec![m(0, 2, MarkKind::Code), m(3, 5, MarkKind::Code)]
    );
}

#[test]
fn a_mark_markdown_cannot_write_there_is_left_out_but_the_text_stays() {
    // `a**(b)**c` is not bold in CommonMark.
    let blocks = [block(
        BlockKind::Paragraph,
        "a(b)c y d",
        vec![m(1, 4, MarkKind::Bold), m(8, 9, MarkKind::Italic)],
    )];
    let back = round_trip(&blocks);
    assert_eq!(back[0].content, "a(b)c y d");
    assert_eq!(back[0].marks, vec![m(8, 9, MarkKind::Italic)]);
}

#[test]
fn code_blocks_and_titles_have_no_formatting() {
    let blocks = [block(
        BlockKind::Code,
        "let x = 1;",
        vec![m(0, 3, MarkKind::Bold)],
    )];
    assert_eq!(body(&blocks), "```\nlet x = 1;\n```\n");
    let page = markdown_to_page("# Título con **negrita**\n\ntexto", "x");
    assert_eq!(page.title, "Título con negrita");
}

#[test]
fn import_keeps_links_that_are_not_web_addresses_as_text() {
    let page = markdown_to_page(
        "[nota](otra.md), [raro](javascript:alert(1)), [web](https://a.es) y ![i](i.png)",
        "x",
    );
    let block = &page.blocks[0];
    assert_eq!(
        block.content,
        "[nota](otra.md), [raro](javascript:alert(1)), web y ![i](i.png)"
    );
    assert_eq!(block.marks, vec![m(46, 49, link("https://a.es"))]);
}

#[test]
fn import_reads_formatting_in_tight_list_items_and_headings() {
    let page = markdown_to_page(
        "# T\n\n## Un *título*\n\n- un **elemento**\n- [x] tarea ~~hecha~~\n\n> cita `x`\n",
        "x",
    );
    let marks: Vec<(&str, Vec<Mark>)> = page
        .blocks
        .iter()
        .map(|b| (b.content.as_str(), b.marks.clone()))
        .collect();
    assert_eq!(
        marks,
        vec![
            ("Un título", vec![m(3, 9, MarkKind::Italic)]),
            ("un elemento", vec![m(3, 11, MarkKind::Bold)]),
            ("tarea hecha", vec![m(6, 11, MarkKind::Strike)]),
            ("cita x", vec![m(5, 6, MarkKind::Code)]),
        ]
    );
}

/// A tiny deterministic generator (xorshift), so failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Random text (letters, spaces, line breaks, Markdown punctuation and
/// Unicode) with random marks: the text always comes back, the marks that
/// come back are among those written, and a second round trip changes
/// nothing.
#[test]
fn random_formatting_never_loses_text() {
    const ALPHABET: &[char] = &[
        'a', 'b', 'c', 'ñ', 'é', '🌾', '日', ' ', ' ', '\n', '*', '_', '`', '~', '[', ']', '(',
        ')', '#', '!', '<', '>', '&', '\\', '-', '.', '1', '|',
    ];
    let kinds = |rng: &mut Rng| match rng.next(5) {
        0 => MarkKind::Bold,
        1 => MarkKind::Italic,
        2 => MarkKind::Code,
        3 => MarkKind::Strike,
        _ => link(["https://a.es", "https://b.es/(x)", "mailto:c@d.es"][rng.next(3)]),
    };
    let mut rng = Rng(0x5eed_1234_abcd_0042);
    for case in 0..3000 {
        let len = 1 + rng.next(24);
        let text: String = (0..len)
            .map(|_| ALPHABET[rng.next(ALPHABET.len())])
            .collect();
        // Markdown cannot end a paragraph with a line break: export
        // leaves those out (tested below), so the text here has none.
        if text.ends_with('\n') {
            continue;
        }
        let mut marks: Vec<Mark> = (0..rng.next(5))
            .map(|_| {
                let start = rng.next(len);
                let end = start + 1 + rng.next(len - start);
                Mark::new(start, end, kinds(&mut rng))
            })
            .collect();
        silex_desktop::marks::normalize(&mut marks, len);
        let kind = [BlockKind::Paragraph, BlockKind::Bulleted, BlockKind::Quote][rng.next(3)];
        let blocks = [block(kind, &text, marks.clone())];
        let markdown = page_to_markdown("t", &blocks);
        let back = markdown_to_page(&markdown, "x").blocks;
        // Empty paragraphs are not written; whitespace-only ones come back.
        assert_eq!(back.len(), 1, "case {case}: {text:?} {marks:?}\n{markdown}");
        assert_eq!(back[0].content, text, "case {case}: {marks:?}\n{markdown}");
        for got in &back[0].marks {
            // Each mark read back is inside one of the same kind written.
            assert!(
                marks
                    .iter()
                    .any(|w| w.kind == got.kind && w.start <= got.start && got.end <= w.end),
                "case {case}: {text:?} {marks:?} gave {got:?}\n{markdown}"
            );
        }
        let again: Vec<Block> = back
            .iter()
            .map(|b| block(b.kind, &b.content, b.marks.clone()))
            .collect();
        assert_eq!(
            page_to_markdown("t", &again),
            markdown,
            "case {case}: not stable"
        );
    }
}

#[test]
fn line_breaks_at_the_end_of_a_block_are_left_out() {
    let blocks = [block(BlockKind::Paragraph, "uno\n\n", Vec::new())];
    assert_eq!(body(&blocks), "uno\n");
    let blocks = [block(
        BlockKind::Quote,
        "dos\n",
        vec![m(0, 4, link("https://a.es"))],
    )];
    assert_eq!(body(&blocks), "> [dos](https://a.es)\n");
}
