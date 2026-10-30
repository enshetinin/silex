//! Import: golden files from other apps, round trips and stability.
use silex_desktop::markdown::{ImportedPage, markdown_to_page, page_to_markdown};
use silex_desktop::model::{BlockKind, NewBlock};

/// One line per block: `kind indent [x]| content` (newlines as ⏎).
fn dump(page: &ImportedPage) -> String {
    let mut out = format!("título: {}\n", page.title);
    for b in &page.blocks {
        let check = match (b.kind, b.checked) {
            (BlockKind::Todo, true) => " [x]",
            (BlockKind::Todo, false) => " [ ]",
            _ => "",
        };
        out.push_str(&format!(
            "{} {}{}| {}{}\n",
            b.kind.as_str(),
            b.indent,
            check,
            b.content.replace('\n', "⏎"),
            dump_marks(&b.content, &b.marks),
        ));
    }
    out
}

/// The formatting of a block, after its text: `  {negrita «pan»; enlace
/// «web» → https://…}`.
fn dump_marks(content: &str, marks: &[silex_desktop::marks::Mark]) -> String {
    use silex_desktop::marks::MarkKind;
    if marks.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = marks
        .iter()
        .map(|m| {
            let text: String = content
                .chars()
                .skip(m.start)
                .take(m.end - m.start)
                .collect::<String>()
                .replace('\n', "⏎");
            match &m.kind {
                MarkKind::Bold => format!("negrita «{text}»"),
                MarkKind::Italic => format!("cursiva «{text}»"),
                MarkKind::Code => format!("código «{text}»"),
                MarkKind::Strike => format!("tachado «{text}»"),
                MarkKind::Link(url) => format!("enlace «{text}» → {url}"),
            }
        })
        .collect();
    format!("  {{{}}}", parts.join("; "))
}

const FIXTURES: &[(&str, &str)] = &[
    ("obsidian", include_str!("golden/import/obsidian.md")),
    ("github", include_str!("golden/import/github.md")),
    ("edge", include_str!("golden/import/edge.md")),
];

/// Golden: each fixture's blocks must match `<name>.expected` (reviewed by hand).
#[test]
fn fixtures_match_their_golden_dumps() {
    let expected = [
        include_str!("golden/import/obsidian.expected"),
        include_str!("golden/import/github.expected"),
        include_str!("golden/import/edge.expected"),
    ];
    for ((name, source), expected) in FIXTURES.iter().zip(expected) {
        let actual = dump(&markdown_to_page(source, name));
        if actual != expected {
            let out = std::env::temp_dir().join(format!("silex_import_{name}.actual"));
            let _ = std::fs::write(&out, &actual);
            panic!(
                "{name}: import changed; actual written to {}",
                out.display()
            );
        }
    }
}

/// Exporting what was imported, then importing and exporting again, gives
/// the same file: one round trip normalises, after that nothing drifts.
#[test]
fn export_after_import_is_stable() {
    for (name, source) in FIXTURES {
        let first = markdown_to_page(source, name);
        let md1 = page_to_markdown(&first.title, &to_blocks(&first.blocks));
        let second = markdown_to_page(&md1, "otro");
        assert_eq!(second, first, "{name}: blocks survive the round trip");
        let md2 = page_to_markdown(&second.title, &to_blocks(&second.blocks));
        assert_eq!(md2, md1, "{name}: export is stable");
    }
}

fn to_blocks(blocks: &[NewBlock]) -> Vec<silex_desktop::model::Block> {
    blocks
        .iter()
        .map(|b| silex_desktop::model::Block {
            id: String::new(),
            page_id: String::new(),
            kind: b.kind,
            content: b.content.clone(),
            checked: b.checked,
            position: 0,
            indent: b.indent,
            attachment: None,
            link: None,
            marks: b.marks.clone(),
        })
        .collect()
}

fn nb(kind: BlockKind, indent: u8, checked: bool, content: &str) -> NewBlock {
    NewBlock {
        kind,
        indent,
        checked,
        content: content.into(),
        marks: Vec::new(),
    }
}

/// Silex → Markdown → Silex gives back exactly the same blocks.
#[test]
fn silex_blocks_round_trip_exactly() {
    use BlockKind::*;
    let blocks = vec![
        nb(Heading2, 0, false, "Sección *con* #símbolos"),
        nb(Paragraph, 0, false, "  espacios al inicio y fin  "),
        nb(Paragraph, 0, false, "dos\nlíneas\n\ny una vacía"),
        nb(Bulleted, 0, false, "nivel 0"),
        nb(Bulleted, 1, false, "nivel 1"),
        nb(Numbered, 2, false, "nivel 2 numerado"),
        nb(Todo, 3, true, "tarea profunda"),
        nb(Bulleted, 0, false, ""),
        nb(Numbered, 0, false, "1. parece lista"),
        nb(Paragraph, 1, false, "párrafo dentro del elemento"),
        nb(Quote, 0, false, "> cita\ncon # cosas"),
        nb(Code, 0, false, "```\n  sangrado\n\tTab"),
        nb(Code, 0, false, ""),
        nb(Heading3, 0, false, ""),
        nb(Paragraph, 0, false, "日本語 🙂 e\u{301} مرحبا"),
    ];
    let markdown = page_to_markdown("Título: *raro* #1", &to_blocks(&blocks));
    let back = markdown_to_page(&markdown, "x");
    assert_eq!(back.title, "Título: *raro* #1");
    assert_eq!(back.blocks, blocks, "markdown was:\n{markdown}");
}

#[test]
fn every_ascii_punctuation_round_trips_in_every_text_kind() {
    use BlockKind::*;
    let punctuation: String = (33u8..127)
        .map(char::from)
        .filter(char::is_ascii_punctuation)
        .collect();
    for kind in [
        Paragraph, Heading1, Heading2, Heading3, Bulleted, Numbered, Todo, Quote, Callout, Code,
    ] {
        for text in [
            punctuation.clone(),
            format!(" {punctuation} x {punctuation} "),
        ] {
            let blocks = vec![nb(Paragraph, 0, false, "antes"), nb(kind, 0, false, &text)];
            let markdown = page_to_markdown("t", &to_blocks(&blocks));
            let back = markdown_to_page(&markdown, "x");
            assert_eq!(back.blocks, blocks, "{kind:?}:\n{markdown}");
        }
    }
}

#[test]
fn title_comes_from_a_leading_h1_or_the_file_name() {
    assert_eq!(markdown_to_page("# Hola\n\ntexto", "archivo").title, "Hola");
    let page = markdown_to_page("texto\n\n# Luego", "archivo");
    assert_eq!(page.title, "archivo");
    assert_eq!(
        page.blocks[1].kind,
        BlockKind::Heading1,
        "later H1 stays a block"
    );
    assert_eq!(markdown_to_page("", "vacío").blocks, []);
}

/// H12.2 and H12.3: dividers and callouts, both ways.
#[test]
fn dividers_and_callouts_round_trip() {
    use BlockKind::*;
    use silex_desktop::marks::{Mark, MarkKind};
    let mut callout = nb(Callout, 0, false, "Ojo con el horno\nsegunda línea");
    callout.marks = vec![Mark::new(0, 3, MarkKind::Bold)];
    let blocks = vec![
        nb(Paragraph, 0, false, "antes"),
        nb(Divider, 0, false, ""),
        callout,
        nb(Callout, 0, false, "otro aviso"),
        nb(Quote, 0, false, "una cita"),
        nb(Bulleted, 0, false, "lista"),
        nb(Divider, 1, false, ""),
        nb(Paragraph, 0, false, "[!NOTE] no es un aviso"),
    ];
    let markdown = page_to_markdown("t", &to_blocks(&blocks));
    assert!(markdown.contains("\n---\n"), "{markdown}");
    assert!(
        markdown.contains("> [!NOTE]\n> **Ojo** con el horno\\\n> segunda línea\n"),
        "{markdown}"
    );
    let back = markdown_to_page(&markdown, "x");
    assert_eq!(back.blocks, blocks, "markdown was:\n{markdown}");
}

#[test]
fn github_alerts_and_obsidian_callouts_become_callouts() {
    let page = markdown_to_page(
        "> [!TIP]\n> un consejo\n>\n> dos párrafos\n\n\
         > [!warning]\n> cuidado\n\n\
         > [!info]\n> de Obsidian con *cursiva*\n\n\
         > [!faq]\n>\n> marcador solo\n\n\
         > [!info] con título\n> sigue siendo cita\n\n\
         > cita normal\n\n***\n",
        "x",
    );
    let got: Vec<(BlockKind, &str)> = page
        .blocks
        .iter()
        .map(|b| (b.kind, b.content.as_str()))
        .collect();
    use BlockKind::*;
    assert_eq!(
        got,
        [
            (Callout, "un consejo"),
            (Callout, "dos párrafos"),
            (Callout, "cuidado"),
            (Callout, "de Obsidian con cursiva"),
            (Callout, "marcador solo"),
            (Quote, "[!info] con título\nsigue siendo cita"),
            (Quote, "cita normal"),
            (Divider, ""),
        ]
    );
    // The formatting after an Obsidian marker stays on its words.
    let words: String = {
        let b = &page.blocks[3];
        let m = &b.marks[0];
        b.content
            .chars()
            .skip(m.start)
            .take(m.end - m.start)
            .collect()
    };
    assert_eq!(words, "cursiva");
}

/// H12.4c: toggles as `<details>`, both ways (ADR 0016).
#[test]
fn toggles_round_trip_as_details() {
    use BlockKind::*;
    let blocks = vec![
        nb(Paragraph, 0, false, "antes"),
        nb(Toggle, 0, false, "Pasos <1> & más"),
        nb(Paragraph, 1, false, "dentro"),
        nb(Bulleted, 1, false, "punto"),
        nb(Bulleted, 2, false, "subpunto"),
        nb(Toggle, 1, false, "anidado"),
        nb(Code, 2, false, "let x = 1;"),
        nb(Paragraph, 1, false, "otra vez dentro"),
        nb(Paragraph, 0, false, "fuera"),
        nb(Bulleted, 0, false, "lista"),
        nb(Toggle, 1, false, "en una lista"),
        nb(Paragraph, 2, false, "su contenido"),
        nb(Toggle, 0, false, "vacío"),
        nb(Toggle, 0, false, "al final"),
        nb(Quote, 1, false, "una cita dentro"),
    ];
    let markdown = page_to_markdown("t", &to_blocks(&blocks));
    assert!(
        markdown.contains(
            "\n<details>\n<summary>Pasos &lt;1&gt; &amp; más</summary>\n\ndentro\n\n- punto\n  - subpunto\n"
        ),
        "{markdown}"
    );
    assert!(
        markdown.ends_with("> una cita dentro\n\n</details>\n"),
        "{markdown}"
    );
    let back = markdown_to_page(&markdown, "x");
    assert_eq!(back.blocks, blocks, "markdown was:\n{markdown}");
    // Stable after one round trip.
    assert_eq!(page_to_markdown("t", &to_blocks(&back.blocks)), markdown);
}

#[test]
fn details_from_github_become_toggles_and_other_html_stays_text() {
    let page = markdown_to_page(
        "<details open>\n<summary>Ver <b>más</b></summary>\n\nTexto **dentro**.\n\n</details>\n\n\
         <DETAILS><SUMMARY>Mayúsculas</SUMMARY></DETAILS>\n\n\
         <div>otra cosa</div>\n\n\
         <detailsx>no</detailsx>\n",
        "x",
    );
    let got: Vec<(BlockKind, u8, &str)> = page
        .blocks
        .iter()
        .map(|b| (b.kind, b.indent, b.content.as_str()))
        .collect();
    use BlockKind::*;
    assert_eq!(
        got,
        [
            (Toggle, 0, "Ver <b>más</b>"),
            (Paragraph, 1, "Texto dentro."),
            (Toggle, 0, "Mayúsculas"),
            (Paragraph, 0, "<div>otra cosa</div>"),
            (Paragraph, 0, "<detailsx>no</detailsx>"),
        ]
    );
    assert_eq!(page.blocks[1].marks.len(), 1, "formatting inside is read");
}

/// H12.8b: a `# Title` that starts with an emoji and a space gives the
/// page its icon; export writes it there, so it comes back.
#[test]
fn an_emoji_before_the_title_is_the_page_icon() {
    use silex_desktop::markdown::titled;
    let cases = [
        ("# 🌾 Recetas de pan", "Recetas de pan", Some("🌾")),
        ("# 👨‍👩‍👧 Familia", "Familia", Some("👨‍👩‍👧")),
        ("# 🇪🇸", "", Some("🇪🇸")),
        ("# 🌾Recetas", "🌾Recetas", None),
        ("# Recetas 🌾", "Recetas 🌾", None),
        ("# 12 recetas", "12 recetas", None),
    ];
    for (source, title, icon) in cases {
        let page = markdown_to_page(source, "archivo");
        assert_eq!(page.title, title, "{source}");
        assert_eq!(page.icon.as_deref(), icon, "{source}");
    }
    // A file without a `# Title`: its name, and no icon.
    assert_eq!(markdown_to_page("texto", "🌾 nombre").icon, None);
    // Round trip.
    let blocks = vec![nb(BlockKind::Paragraph, 0, false, "texto")];
    let markdown = page_to_markdown(&titled(Some("📒"), "Diario"), &to_blocks(&blocks));
    assert!(markdown.starts_with("# 📒 Diario\n"), "{markdown}");
    let back = markdown_to_page(&markdown, "x");
    assert_eq!(
        (back.title.as_str(), back.icon.as_deref()),
        ("Diario", Some("📒"))
    );
    assert_eq!(back.blocks, blocks);
    assert_eq!(titled(None, "Diario"), "Diario");
}
