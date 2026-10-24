//! Page templates (ADR 0010), without SQL or egui: `{{fecha}}` and the
//! example templates Silex offers the first time.
use crate::marks::Mark;
use crate::model::{BlockKind, NewBlock};
use crate::versions::MONTHS;

/// Replaced by today's date when a page is created from a template.
pub const DATE_PLACEHOLDER: &str = "{{fecha}}";

/// `YYYY-MM-DD` as «20 oct 2026»; `None` if it is not such a date.
pub fn long_date(ymd: &str) -> Option<String> {
    let mut parts = ymd.trim().splitn(3, '-');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: usize = parts
        .next()?
        .parse()
        .ok()
        .filter(|m| (1..=12).contains(m))?;
    let day: u32 = parts
        .next()?
        .parse()
        .ok()
        .filter(|d| (1..=31).contains(d))?;
    Some(format!("{day} {} {year}", MONTHS[month - 1]))
}

/// `text` with every `{{fecha}}` replaced by `today` (`YYYY-MM-DD`) as a
/// long date. A bad `today` leaves it as it is.
pub fn fill_date(text: &str, today: &str) -> String {
    match long_date(today) {
        Some(date) if text.contains(DATE_PLACEHOLDER) => text.replace(DATE_PLACEHOLDER, &date),
        _ => text.to_owned(),
    }
}

/// Like [`fill_date`], also moving the formatting of `text` (ADR 0015)
/// so it stays on the same words.
pub fn fill_date_marked(text: &str, marks: &[Mark], today: &str) -> (String, Vec<Mark>) {
    match long_date(today) {
        Some(date) if text.contains(DATE_PLACEHOLDER) => (
            text.replace(DATE_PLACEHOLDER, &date),
            crate::marks::replace_all(marks, text, DATE_PLACEHOLDER, &date),
        ),
        _ => (text.to_owned(), marks.to_vec()),
    }
}

/// The templates Silex creates the first time the list of templates is
/// opened: a title and its blocks.
pub fn examples() -> Vec<(&'static str, Vec<NewBlock>)> {
    let block = |kind: BlockKind, content: &str| NewBlock {
        kind,
        indent: 0,
        checked: false,
        content: content.to_owned(),
    };
    use BlockKind::*;
    vec![
        (
            "Reunión {{fecha}}",
            vec![
                block(Paragraph, "Asistentes: "),
                block(Heading2, "Temas"),
                block(Bulleted, ""),
                block(Heading2, "Decisiones"),
                block(Bulleted, ""),
                block(Heading2, "Tareas"),
                block(Todo, ""),
            ],
        ),
        (
            "Diario {{fecha}}",
            vec![
                block(Heading2, "Hoy"),
                block(Paragraph, ""),
                block(Heading2, "Lo mejor del día"),
                block(Paragraph, ""),
                block(Heading2, "Para mañana"),
                block(Todo, ""),
            ],
        ),
        (
            "Lista de tareas",
            vec![
                block(Heading2, "Por hacer"),
                block(Todo, ""),
                block(Heading2, "Hecho"),
                block(Quote, "Marca una tarea y muévela aquí con Alt+Mayús+↓."),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_written_long_and_fill_the_placeholder() {
        assert_eq!(long_date("2026-10-20").as_deref(), Some("20 oct 2026"));
        assert_eq!(long_date("2026-01-05").as_deref(), Some("5 ene 2026"));
        assert_eq!(long_date("2026-13-05"), None);
        assert_eq!(long_date("ayer"), None);
        assert_eq!(
            fill_date("Diario {{fecha}} ({{fecha}})", "2026-10-20"),
            "Diario 20 oct 2026 (20 oct 2026)"
        );
        assert_eq!(fill_date("Sin fecha", "2026-10-20"), "Sin fecha");
        assert_eq!(fill_date("Diario {{fecha}}", "mal"), "Diario {{fecha}}");
        // Other braces are left alone.
        assert_eq!(
            fill_date("{{hora}} {fecha}", "2026-10-20"),
            "{{hora}} {fecha}"
        );
    }

    #[test]
    fn the_examples_are_three_text_pages() {
        let examples = examples();
        let titles: Vec<&str> = examples.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            titles,
            ["Reunión {{fecha}}", "Diario {{fecha}}", "Lista de tareas"]
        );
        assert!(
            examples
                .iter()
                .all(|(_, blocks)| !blocks.is_empty() && blocks.iter().all(|b| b.kind.is_text()))
        );
    }
}
