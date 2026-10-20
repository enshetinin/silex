//! Lucide icons, drawn as text with the bundled icon font. Codepoints from
//! `lucide-static@1.49.0/font/lucide.css`; the comment after each constant
//! is the Lucide name (see `assets/fonts/README.md`). Add icons here as the
//! UI needs them.
#![allow(dead_code)] // the set is filled in ahead of the screens that use it

use eframe::egui::{self, Color32, FontFamily, FontId, KeyboardShortcut, RichText};

/// Font family that contains only the icon font. Icons must be drawn with
/// it (see `theme::fonts` for why they cannot share the text families).
pub const FAMILY: &str = "icons";

/// An icon as rich text (its own font family), at the standard size.
pub fn icon(icon: &str) -> RichText {
    RichText::new(icon)
        .family(FontFamily::Name(FAMILY.into()))
        .size(crate::app::theme::icon::MD)
}

/// Side of an icon-only button: a square around an 18 px icon.
pub const BUTTON: f32 = 28.0;

/// An icon-only button: a square of [`BUTTON`] px with the icon painted at
/// its exact centre (egui's `Button` left-aligns its content inside a
/// minimum size), filled only on hover. Its name is the tooltip (with the
/// shortcut, if it has one) and what screen readers announce.
pub fn button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: &str,
    color: Color32,
    name: &str,
    shortcut: Option<KeyboardShortcut>,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(BUTTON), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if enabled
            && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on())
        {
            ui.painter()
                .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect,
                visuals.corner_radius,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Inside,
            );
        }
        let color = if enabled {
            color
        } else {
            color.gamma_multiply(0.45)
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            font(crate::app::theme::icon::MD),
            color,
        );
    }
    let tip = match shortcut {
        Some(shortcut) => format!("{name}   {}", ui.ctx().format_shortcut(&shortcut)),
        None => name.to_owned(),
    };
    let response = if enabled {
        response.on_hover_text(tip)
    } else {
        response.on_hover_text(name)
    };
    response.accessible_name(name)
}

/// Font for painting an icon directly.
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY.into()))
}

pub const PLUS: &str = "\u{e13d}"; // plus
pub const DOTS_THREE: &str = "\u{e0b6}"; // ellipsis
pub const DOTS_THREE_VERTICAL: &str = "\u{e0b7}"; // ellipsis-vertical
pub const DRAG_HANDLE: &str = "\u{e0eb}"; // grip-vertical
pub const CARET_RIGHT: &str = "\u{e06f}"; // chevron-right
pub const CARET_LEFT: &str = "\u{e06e}"; // chevron-left
pub const CARET_DOWN: &str = "\u{e06d}"; // chevron-down
pub const CARET_UP: &str = "\u{e070}"; // chevron-up
pub const SEARCH: &str = "\u{e151}"; // search
pub const STAR: &str = "\u{e176}"; // star
pub const TRASH: &str = "\u{e18e}"; // trash-2
pub const PAGE: &str = "\u{e0cc}"; // file-text
pub const FOLDER: &str = "\u{e0d7}"; // folder
pub const FOLDER_OPEN: &str = "\u{e247}"; // folder-open
pub const UNDO: &str = "\u{e2a1}"; // undo-2
pub const REDO: &str = "\u{e2a0}"; // redo-2
pub const EXPORT: &str = "\u{e155}"; // share
pub const DOWNLOAD: &str = "\u{e0b2}"; // download
pub const UPLOAD: &str = "\u{e19e}"; // upload
pub const SETTINGS: &str = "\u{e154}"; // settings
pub const BACKUP: &str = "\u{e1f5}"; // history
/// Page history (H7.8): the same clock turning back as backups.
pub const HISTORY: &str = BACKUP;
pub const CLOSE: &str = "\u{e1b2}"; // x
pub const CHECK: &str = "\u{e06c}"; // check
pub const TEXT: &str = "\u{e198}"; // type
pub const HEADING_1: &str = "\u{e385}"; // heading-1
pub const HEADING_2: &str = "\u{e386}"; // heading-2
pub const HEADING_3: &str = "\u{e387}"; // heading-3
pub const LIST_BULLETS: &str = "\u{e106}"; // list
pub const LIST_NUMBERS: &str = "\u{e1d1}"; // list-ordered
pub const TODO: &str = "\u{e559}"; // square-check
pub const QUOTE: &str = "\u{e239}"; // quote
pub const CODE: &str = "\u{e093}"; // code
pub const COPY: &str = "\u{e09e}"; // copy
pub const ARROW_UP: &str = "\u{e04a}"; // arrow-up
pub const ARROW_DOWN: &str = "\u{e042}"; // arrow-down
pub const SIDEBAR: &str = "\u{e12a}"; // panel-left
pub const SUN: &str = "\u{e178}"; // sun
pub const MOON: &str = "\u{e11e}"; // moon
pub const HOME: &str = "\u{e0f5}"; // house
pub const SAVE: &str = "\u{e14d}"; // save
pub const WARNING: &str = "\u{e193}"; // triangle-alert
pub const INFO: &str = "\u{e0f9}"; // info
pub const OPEN_EXTERNAL: &str = "\u{e0b9}"; // external-link
pub const KEYBOARD: &str = "\u{e284}"; // keyboard
pub const TABLE: &str = "\u{e17d}"; // table
pub const KANBAN: &str = "\u{e170}"; // square-kanban
pub const PAPERCLIP: &str = "\u{e12d}"; // paperclip
pub const LINK: &str = "\u{e102}"; // link
pub const FILE: &str = "\u{e0c0}"; // file
pub const FILE_IMAGE: &str = "\u{e31c}"; // file-image
pub const FILE_PDF: &str = "\u{e329}"; // file-type
pub const FILE_ZIP: &str = "\u{e30d}"; // file-archive
pub const FILE_AUDIO: &str = "\u{e31a}"; // file-audio
pub const FILE_VIDEO: &str = "\u{e321}"; // file-video
/// "Aa": the name column of a database.
pub const TEXT_AA: &str = "\u{e3d9}"; // case-sensitive
/// Property types (H6.2): text, number, date and select.
pub const TEXT_ALIGN_LEFT: &str = "\u{e185}"; // text-align-start
pub const HASH: &str = "\u{e0ef}"; // hash
pub const CALENDAR: &str = "\u{e063}"; // calendar
pub const CARET_CIRCLE_DOWN: &str = "\u{e4dd}"; // circle-chevron-down
pub const ARROW_LEFT: &str = "\u{e048}"; // arrow-left
pub const ARROW_RIGHT: &str = "\u{e049}"; // arrow-right
/// View menus (H6.3a): filter, sort, columns shown or hidden.
pub const FUNNEL: &str = "\u{e0dc}"; // funnel
pub const ARROWS_DOWN_UP: &str = "\u{e046}"; // arrow-down-up
pub const EYE: &str = "\u{e0ba}"; // eye
pub const EYE_SLASH: &str = "\u{e0bb}"; // eye-off

/// Every icon above, for tests.
#[cfg(test)]
pub const ALL: &[(&str, &str)] = &[
    ("PLUS", PLUS),
    ("DOTS_THREE", DOTS_THREE),
    ("DOTS_THREE_VERTICAL", DOTS_THREE_VERTICAL),
    ("DRAG_HANDLE", DRAG_HANDLE),
    ("CARET_RIGHT", CARET_RIGHT),
    ("CARET_DOWN", CARET_DOWN),
    ("CARET_UP", CARET_UP),
    ("CARET_LEFT", CARET_LEFT),
    ("SEARCH", SEARCH),
    ("STAR", STAR),
    ("TRASH", TRASH),
    ("PAGE", PAGE),
    ("FOLDER", FOLDER),
    ("FOLDER_OPEN", FOLDER_OPEN),
    ("UNDO", UNDO),
    ("REDO", REDO),
    ("EXPORT", EXPORT),
    ("DOWNLOAD", DOWNLOAD),
    ("UPLOAD", UPLOAD),
    ("SETTINGS", SETTINGS),
    ("BACKUP", BACKUP),
    ("CLOSE", CLOSE),
    ("CHECK", CHECK),
    ("TEXT", TEXT),
    ("HEADING_1", HEADING_1),
    ("HEADING_2", HEADING_2),
    ("HEADING_3", HEADING_3),
    ("LIST_BULLETS", LIST_BULLETS),
    ("LIST_NUMBERS", LIST_NUMBERS),
    ("TODO", TODO),
    ("QUOTE", QUOTE),
    ("CODE", CODE),
    ("COPY", COPY),
    ("ARROW_UP", ARROW_UP),
    ("ARROW_DOWN", ARROW_DOWN),
    ("SIDEBAR", SIDEBAR),
    ("SUN", SUN),
    ("MOON", MOON),
    ("HOME", HOME),
    ("SAVE", SAVE),
    ("WARNING", WARNING),
    ("INFO", INFO),
    ("OPEN_EXTERNAL", OPEN_EXTERNAL),
    ("KEYBOARD", KEYBOARD),
    ("TABLE", TABLE),
    ("KANBAN", KANBAN),
    ("PAPERCLIP", PAPERCLIP),
    ("LINK", LINK),
    ("FILE", FILE),
    ("FILE_IMAGE", FILE_IMAGE),
    ("FILE_PDF", FILE_PDF),
    ("FILE_ZIP", FILE_ZIP),
    ("FILE_AUDIO", FILE_AUDIO),
    ("FILE_VIDEO", FILE_VIDEO),
    ("TEXT_AA", TEXT_AA),
    ("TEXT_ALIGN_LEFT", TEXT_ALIGN_LEFT),
    ("HASH", HASH),
    ("CALENDAR", CALENDAR),
    ("CARET_CIRCLE_DOWN", CARET_CIRCLE_DOWN),
    ("ARROW_LEFT", ARROW_LEFT),
    ("ARROW_RIGHT", ARROW_RIGHT),
    ("FUNNEL", FUNNEL),
    ("ARROWS_DOWN_UP", ARROWS_DOWN_UP),
    ("EYE", EYE),
    ("EYE_SLASH", EYE_SLASH),
];
