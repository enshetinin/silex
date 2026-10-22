//! Painting inline formatting (ADR 0015): a block's text as an egui
//! `LayoutJob` with one section per run of equal formatting.
//!
//! The job's text is always exactly the field's text, so the cursor, the
//! selection and IME keep working on the same characters; only the look of
//! each run changes.
use eframe::egui::{self, Color32, FontFamily, FontId, Stroke, TextFormat, text::LayoutJob};
use silex_desktop::marks::{Format, Mark, MarkKind};

use super::theme;

/// What formatting looks like on top of a block's own format.
pub(super) struct InlineLook {
    /// Body text and links of a link.
    pub(super) link: Color32,
    /// Behind inline code.
    pub(super) code_bg: Color32,
}

/// Monospace text is a little smaller than Inter at the same size.
const CODE_SCALE: f32 = 0.92;

/// `text` with `marks` over `base` (the block's font, color and, for a
/// done task, strikethrough). Marks outside the text are ignored.
pub(super) fn layout_job(
    text: &str,
    marks: &[Mark],
    base: &TextFormat,
    look: &InlineLook,
) -> LayoutJob {
    let mut job = LayoutJob {
        // Like egui's own layouter: trailing spaces stay while typing.
        keep_trailing_whitespace: true,
        ..Default::default()
    };
    // Byte offset of every character boundary, so char ranges map to
    // byte ranges in one pass.
    let bytes: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let len = bytes.len() - 1;
    let mut cuts: Vec<usize> = marks
        .iter()
        .flat_map(|m| [m.start.min(len), m.end.min(len)])
        .chain([0, len])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let active: Vec<&MarkKind> = marks
            .iter()
            .filter(|m| m.start <= start && end <= m.end)
            .map(|m| &m.kind)
            .collect();
        job.append(
            &text[bytes[start]..bytes[end]],
            0.0,
            format_for(base, &active, look),
        );
    }
    if job.sections.is_empty() {
        // An empty text still needs its format (cursor height).
        job.append("", 0.0, base.clone());
    }
    job
}

/// `base` with the formatting of `active` marks on top.
fn format_for(base: &TextFormat, active: &[&MarkKind], look: &InlineLook) -> TextFormat {
    let mut format = base.clone();
    for kind in active {
        match kind {
            // Headings are already semibold: there bold looks the same,
            // as in Notion (ADR 0015).
            MarkKind::Bold => {
                format.font_id.family = FontFamily::Name(theme::SEMIBOLD.into());
            }
            MarkKind::Italic => format.italics = true,
            MarkKind::Code => {}
            MarkKind::Strike => format.strikethrough = Stroke::new(1.0, format.color),
            MarkKind::Link(_) => {
                format.color = look.link;
                format.underline = Stroke::new(1.0, look.link);
            }
        }
    }
    // Code last: its font replaces bold's, but keeps the size.
    if active.iter().any(|k| matches!(k, MarkKind::Code)) {
        format.font_id = FontId::monospace(base.font_id.size * CODE_SCALE);
        format.background = look.code_bg;
    }
    if format.strikethrough != Stroke::NONE {
        format.strikethrough.color = format.color;
    }
    format
}

/// The keyboard shortcut of a format, as menus show it.
pub(super) fn shortcut(ctx: &egui::Context, format: Format) -> String {
    shortcut_label(format, super::cmd_label(ctx))
}

/// The keyboard shortcut of a format, with `cmd` "Cmd" or "Ctrl".
pub(super) fn shortcut_label(format: Format, cmd: &str) -> String {
    match format {
        Format::Bold => format!("{cmd}+B"),
        Format::Italic => format!("{cmd}+I"),
        Format::Code => format!("{cmd}+E"),
        Format::Strike => format!("{cmd}+Mayús+X"),
    }
}

/// The icon of a format.
pub(super) fn icon(format: Format) -> &'static str {
    match format {
        Format::Bold => super::icons::BOLD,
        Format::Italic => super::icons::ITALIC,
        Format::Code => super::icons::CODE,
        Format::Strike => super::icons::STRIKETHROUGH,
    }
}

/// The look of formatting in the current theme.
pub(super) fn look(ctx: &egui::Context) -> InlineLook {
    let p = theme::current(ctx);
    InlineLook {
        link: p.accent_strong,
        code_bg: p.code_bg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> TextFormat {
        TextFormat {
            font_id: FontId::proportional(14.0),
            color: Color32::BLACK,
            ..Default::default()
        }
    }

    const LOOK: InlineLook = InlineLook {
        link: Color32::BLUE,
        code_bg: Color32::GRAY,
    };

    /// The sections cover the text once, in order, on char boundaries.
    fn assert_covers(job: &LayoutJob) {
        let mut at = 0;
        for section in &job.sections {
            assert_eq!(section.byte_range.start.0, at);
            assert!(job.text.is_char_boundary(section.byte_range.end.0));
            at = section.byte_range.end.0;
        }
        assert_eq!(at, job.text.len());
    }

    #[test]
    fn the_text_is_the_fields_text_and_runs_get_their_format() {
        let text = "café 🌾 código y enlace";
        let marks = vec![
            Mark::new(0, 4, MarkKind::Bold),
            Mark::new(2, 6, MarkKind::Italic),
            Mark::new(7, 13, MarkKind::Code),
            Mark::new(16, 22, MarkKind::Link("https://a.es".into())),
            Mark::new(16, 22, MarkKind::Strike),
        ];
        let job = layout_job(text, &marks, &base(), &LOOK);
        assert_eq!(job.text, text);
        assert_covers(&job);
        let at = |char: usize| {
            let byte = text.char_indices().nth(char).map(|(b, _)| b).unwrap_or(0);
            &job.sections
                .iter()
                .find(|s| (s.byte_range.start.0..s.byte_range.end.0).contains(&byte))
                .expect("covered")
                .format
        };
        assert_eq!(
            at(0).font_id.family,
            FontFamily::Name(theme::SEMIBOLD.into())
        );
        assert!(!at(0).italics && at(3).italics && at(5).italics);
        assert_eq!(at(8).font_id.family, FontFamily::Monospace);
        assert_eq!(at(8).background, Color32::GRAY);
        assert_eq!(at(17).color, Color32::BLUE);
        assert_eq!(at(17).underline.color, Color32::BLUE);
        assert_eq!(at(17).strikethrough.color, Color32::BLUE);
        assert_eq!(at(14), &base());
    }

    #[test]
    fn marks_outside_the_text_and_empty_text_never_break_the_job() {
        let marks = vec![Mark::new(2, 50, MarkKind::Bold)];
        for text in ["", "a", "ab", "abc", "ñ🌾"] {
            let job = layout_job(text, &marks, &base(), &LOOK);
            assert_eq!(job.text, text);
            assert_covers(&job);
            assert!(!job.sections.is_empty());
        }
    }

    #[test]
    fn a_done_task_keeps_its_strikethrough_and_color() {
        let mut done = base();
        done.color = Color32::DARK_GRAY;
        done.strikethrough = Stroke::new(1.0, Color32::DARK_GRAY);
        let job = layout_job("hecho", &[Mark::new(0, 2, MarkKind::Bold)], &done, &LOOK);
        for section in &job.sections {
            assert_eq!(section.format.strikethrough.color, Color32::DARK_GRAY);
            assert_eq!(section.format.color, Color32::DARK_GRAY);
        }
    }
}
