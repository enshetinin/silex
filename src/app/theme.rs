//! Design system: color tokens for light and dark, spacing, type scale and
//! fonts (Inter + Lucide icons, see `assets/fonts/README.md`).
//!
//! Every color the UI uses comes from a [`Palette`]; text/background pairs
//! are checked for WCAG AA contrast in the tests below.
// The whole token set is defined up front; screens adopt it as they are
// redesigned (H5.3–H5.5). Remove this once every token is in use.
#![allow(dead_code)]
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, Vec2,
};
use silex_desktop::property::OptionColor;

/// Colors for one theme. Names describe the role, not the hue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Main canvas (editor).
    pub bg: Color32,
    /// Raised surfaces: menus, dialogs, popups.
    pub surface: Color32,
    /// Hover and quiet fills on the canvas.
    pub subtle: Color32,
    /// Pressed state on the canvas.
    pub pressed: Color32,
    pub border: Color32,
    /// Outline of small controls on the canvas (option pills).
    pub border_strong: Color32,
    pub text: Color32,
    /// Secondary text (meta, labels). Meets AA on the canvas.
    pub text_muted: Color32,
    /// Placeholders and decoration only (not for content).
    pub text_faint: Color32,
    /// Icons, focus and selection outlines.
    pub accent: Color32,
    /// Accent used as text or as a button background.
    pub accent_strong: Color32,
    /// Text on an `accent_strong` background.
    pub on_accent: Color32,
    /// Selected rows and text selection.
    pub accent_soft: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    /// Sidebar: a neutral grey of the canvas' family, split from the page
    /// by a 1 px line; the selected page is a grey fill.
    pub sidebar_bg: Color32,
    pub sidebar_hover: Color32,
    pub sidebar_selected: Color32,
    /// Label and icon of the selected page.
    pub sidebar_selected_text: Color32,
    pub sidebar_text: Color32,
    pub sidebar_muted: Color32,
    pub code_bg: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const LIGHT: Palette = Palette {
    bg: hex(0xFFFFFF),
    surface: hex(0xFFFFFF),
    subtle: hex(0xF4F4F5),
    pressed: hex(0xE9E9EB),
    border: hex(0xE4E4E7),
    border_strong: hex(0xD4D4D8),
    text: hex(0x18181B),
    text_muted: hex(0x5F5F68),
    text_faint: hex(0x8A8A92),
    accent: hex(0x2F8FE0),
    accent_strong: hex(0x1F6FB8),
    on_accent: hex(0xFFFFFF),
    accent_soft: hex(0xE6F1FC),
    danger: hex(0xC62828),
    warning: hex(0x9A5800),
    success: hex(0x2E7D32),
    sidebar_bg: hex(0xFAFAFA),
    sidebar_hover: hex(0xF1F1F2),
    sidebar_selected: hex(0xE9E9EB),
    sidebar_selected_text: hex(0x18181B),
    sidebar_text: hex(0x3F3F46),
    sidebar_muted: hex(0x5F5F68),
    code_bg: hex(0xF4F4F5),
};

pub const DARK: Palette = Palette {
    bg: hex(0x18181A),
    surface: hex(0x1F1F22),
    subtle: hex(0x232326),
    pressed: hex(0x2A2A2E),
    border: hex(0x29292D),
    border_strong: hex(0x323236),
    text: hex(0xEDEDEF),
    text_muted: hex(0x8E8E95),
    text_faint: hex(0x66666E),
    accent: hex(0x5AB4F5),
    accent_strong: hex(0x6BBEF7),
    on_accent: hex(0x0B0B0C),
    accent_soft: hex(0x182634),
    danger: hex(0xEF6B6B),
    warning: hex(0xE0A04B),
    success: hex(0x5BC583),
    sidebar_bg: hex(0x18181A),
    sidebar_hover: hex(0x232326),
    sidebar_selected: hex(0x2A2A2E),
    sidebar_selected_text: hex(0xEDEDEF),
    sidebar_text: hex(0xC9C9CE),
    sidebar_muted: hex(0x8E8E95),
    code_bg: hex(0x1F1F22),
};

/// Background and text of a select option's tag (H6.2), in the order of
/// [`OptionColor::ALL`]: gray, blue, green, yellow, orange, red, purple, pink.
/// Pills (H10.2) draw their dot and text in the second color with no fill;
/// the first one is the color swatch's fill in the option menus.
const LIGHT_TAGS: [(Color32, Color32); 8] = [
    (hex(0xECEEF1), hex(0x3F4651)),
    (hex(0xDCEBFB), hex(0x1A4F85)),
    (hex(0xDDF1E2), hex(0x1D5B30)),
    (hex(0xFBF1C7), hex(0x6B4E00)),
    (hex(0xFDE5D2), hex(0x8A3B0A)),
    (hex(0xFCE0DE), hex(0x9A1F1A)),
    (hex(0xECE2F8), hex(0x5A2E8A)),
    (hex(0xFBE1EE), hex(0x8C2357)),
];

const DARK_TAGS: [(Color32, Color32); 8] = [
    (hex(0x2B3038), hex(0xD0D5DC)),
    (hex(0x17324F), hex(0xA9D2F7)),
    (hex(0x173A26), hex(0xA6E0B8)),
    (hex(0x3D3212), hex(0xF0D98A)),
    (hex(0x43260F), hex(0xF5C095)),
    (hex(0x4A1D1D), hex(0xF6B0AB)),
    (hex(0x33244A), hex(0xD3BBF2)),
    (hex(0x47203A), hex(0xF3B3D3)),
];

/// Background and text color of a select option's tag in the current theme.
pub fn option_colors(ctx: &egui::Context, color: OptionColor) -> (Color32, Color32) {
    let tags = if ctx.theme() == Theme::Dark {
        &DARK_TAGS
    } else {
        &LIGHT_TAGS
    };
    let index = OptionColor::ALL
        .iter()
        .position(|c| *c == color)
        .unwrap_or_default();
    tags[index]
}

/// Spacing scale (px). Use these instead of ad-hoc numbers.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Corner radii.
pub mod radius {
    pub const SM: u8 = 4;
    pub const MD: u8 = 6;
    pub const LG: u8 = 10;
}

/// Icon sizes (px): 18 in rows, menus and toolbars; 20 in the window frame.
pub mod icon {
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 18.0;
}

/// Named text styles beyond egui's built-in ones.
pub mod text {
    pub const TITLE: &str = "title";
    pub const H1: &str = "h1";
    pub const H2: &str = "h2";
    pub const H3: &str = "h3";
}

/// Font family for semibold text (titles, headings, emphasis).
pub const SEMIBOLD: &str = "semibold";

pub fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    }
}

/// The palette of the theme the UI is currently using.
pub fn current(ctx: &egui::Context) -> &'static Palette {
    palette(ctx.theme())
}

/// Installs fonts and both themes' styles. The theme follows the system
/// (egui's default `ThemePreference::System`). `SilexApp` calls it on its
/// first frame: the named text styles must exist before anything uses them.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_style_of(Theme::Light, style(&LIGHT, false));
    ctx.set_style_of(Theme::Dark, style(&DARK, true));
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let insert = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes).into());
    };
    insert(
        &mut fonts,
        "Inter",
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    );
    insert(
        &mut fonts,
        "Inter-SemiBold",
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    // Measured on the snapshots: at 16 px next to 13–14 px Inter, Lucide's
    // glyphs and the text share a centre line, so no vertical tweak.
    insert(
        &mut fonts,
        "Lucide",
        include_bytes!("../../assets/fonts/Lucide.ttf"),
    );

    // Icons get a family of their own and are always drawn with it. They
    // cannot share a fallback chain with text: Inter has 745 glyphs in the
    // Private Use Area, where icon fonts put their glyphs, so text would
    // pick Inter's glyph instead of the icon.
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let mut semibold = vec!["Inter-SemiBold".to_owned()];
    semibold.extend(proportional.iter().skip(1).cloned());
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts.families.insert(
        FontFamily::Name(crate::app::icons::FAMILY.into()),
        vec!["Lucide".to_owned()],
    );
    fonts
}

fn style(p: &Palette, dark: bool) -> egui::Style {
    let mut style = egui::Style::default();
    let semibold = || FontFamily::Name(SEMIBOLD.into());
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
        (TextStyle::Heading, FontId::new(20.0, semibold())),
        (
            TextStyle::Name(text::TITLE.into()),
            FontId::new(30.0, semibold()),
        ),
        (
            TextStyle::Name(text::H1.into()),
            FontId::new(26.0, semibold()),
        ),
        (
            TextStyle::Name(text::H2.into()),
            FontId::new(21.0, semibold()),
        ),
        (
            TextStyle::Name(text::H3.into()),
            FontId::new(17.0, semibold()),
        ),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(space::SM, 6.0);
    s.button_padding = Vec2::new(10.0, 5.0);
    s.interact_size = Vec2::new(24.0, 28.0);
    s.indent = 18.0;
    s.window_margin = Margin::same(space::MD as i8);
    s.menu_margin = Margin::same(6);
    s.menu_spacing = 2.0;

    let v = &mut style.visuals;
    *v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(radius::LG);
    v.menu_corner_radius = CornerRadius::same(radius::MD);
    let shadow_alpha = if dark { 90 } else { 28 };
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.faint_bg_color = p.subtle;
    v.extreme_bg_color = p.bg;
    v.text_edit_bg_color = Some(p.bg);
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.accent_strong;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_muted);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent_strong);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    let radius = CornerRadius::same(radius::MD);
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, p.subtle),
        (&mut w.hovered, p.pressed),
        (&mut w.active, p.pressed),
        (&mut w.open, p.subtle),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    // Keyboard focus and hovered inputs get a visible accent outline.
    w.hovered.bg_stroke = Stroke::new(1.0, p.border);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    // Contrast (WCAG 2.x)
    fn channel(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Color32) -> f64 {
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG contrast ratio between two opaque colors (1.0 to 21.0).
    pub fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// WCAG AA: 4.5:1 for text, 3:1 for non-text UI parts and decoration.
    const TEXT: f64 = 4.5;
    const NON_TEXT: f64 = 3.0;

    fn check(name: &str, pairs: &[(&str, Color32, Color32, f64)]) {
        let mut failures = Vec::new();
        for (what, fg, bg, min) in pairs {
            let ratio = contrast(*fg, *bg);
            if ratio < *min {
                failures.push(format!("{name}: {what} = {ratio:.2} < {min}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    fn pairs(p: &Palette) -> Vec<(&'static str, Color32, Color32, f64)> {
        vec![
            ("text / bg", p.text, p.bg, TEXT),
            ("text / surface", p.text, p.surface, TEXT),
            ("text / subtle", p.text, p.subtle, TEXT),
            ("text / pressed", p.text, p.pressed, TEXT),
            ("text / accent_soft", p.text, p.accent_soft, TEXT),
            ("text / code_bg", p.text, p.code_bg, TEXT),
            ("text_muted / bg", p.text_muted, p.bg, TEXT),
            ("text_muted / surface", p.text_muted, p.surface, TEXT),
            ("text_muted / subtle", p.text_muted, p.subtle, TEXT),
            (
                "text_muted / accent_soft (selected search result)",
                p.text_muted,
                p.accent_soft,
                TEXT,
            ),
            (
                "accent_strong / accent_soft (selected result icon)",
                p.accent_strong,
                p.accent_soft,
                NON_TEXT,
            ),
            ("accent_strong / bg (links)", p.accent_strong, p.bg, TEXT),
            (
                "on_accent / accent_strong",
                p.on_accent,
                p.accent_strong,
                TEXT,
            ),
            ("danger / bg", p.danger, p.bg, TEXT),
            ("warning / bg", p.warning, p.bg, TEXT),
            ("success / bg", p.success, p.bg, TEXT),
            (
                "sidebar_text / sidebar_bg",
                p.sidebar_text,
                p.sidebar_bg,
                TEXT,
            ),
            (
                "sidebar_text / sidebar_hover",
                p.sidebar_text,
                p.sidebar_hover,
                TEXT,
            ),
            (
                "sidebar_text / sidebar_selected",
                p.sidebar_text,
                p.sidebar_selected,
                TEXT,
            ),
            (
                "sidebar_selected_text / sidebar_selected",
                p.sidebar_selected_text,
                p.sidebar_selected,
                TEXT,
            ),
            ("accent / bg (checked to-do)", p.accent, p.bg, NON_TEXT),
            (
                "sidebar_muted / sidebar_bg",
                p.sidebar_muted,
                p.sidebar_bg,
                TEXT,
            ),
            (
                "sidebar_muted / sidebar_hover",
                p.sidebar_muted,
                p.sidebar_hover,
                TEXT,
            ),
            (
                "text_faint / bg (placeholders)",
                p.text_faint,
                p.bg,
                NON_TEXT,
            ),
            ("accent / bg (icons, focus)", p.accent, p.bg, NON_TEXT),
        ]
    }

    #[test]
    fn light_palette_meets_wcag_aa() {
        check("claro", &pairs(&LIGHT));
    }

    #[test]
    fn dark_palette_meets_wcag_aa() {
        check("oscuro", &pairs(&DARK));
    }

    /// Option pills: their text, in the option's color, on every surface
    /// a pill is drawn on (rows at rest, hovered or selected, menus, board
    /// columns and cards, a column a card is dragged over).
    #[test]
    fn option_tags_meet_wcag_aa() {
        for (theme, p, tags) in [
            ("claro", &LIGHT, &LIGHT_TAGS),
            ("oscuro", &DARK, &DARK_TAGS),
        ] {
            let surfaces = [
                ("bg", p.bg),
                ("surface", p.surface),
                ("subtle", p.subtle),
                ("pressed", p.pressed),
                ("accent_soft", p.accent_soft),
            ];
            let mut named = Vec::new();
            for (color, (_, fg)) in OptionColor::ALL.iter().zip(tags.iter()) {
                for (surface, bg) in surfaces {
                    named.push((format!("{} / {surface}", color.as_str()), *fg, bg));
                }
            }
            let pairs: Vec<_> = named
                .iter()
                .map(|(what, fg, bg)| (what.as_str(), *fg, *bg, TEXT))
                .collect();
            check(theme, &pairs);
        }
    }

    #[test]
    fn contrast_matches_known_values() {
        let black = Color32::BLACK;
        let white = Color32::WHITE;
        assert!((contrast(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
        // #777777 on white is the classic 4.48:1 that fails AA.
        assert!((contrast(hex(0x777777), white) - 4.48).abs() < 0.01);
    }

    #[test]
    fn fonts_and_icons_load() {
        let ctx = egui::Context::default();
        install(&ctx);
        let has = |c: char, family: FontFamily| {
            let mut found = false;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                found = ui.fonts_mut(|f| f.has_glyph(&FontId::new(14.0, family.clone()), c));
            });
            output.textures_delta.clear();
            found
        };
        assert!(has('ñ', FontFamily::Proportional));
        assert!(has('ñ', FontFamily::Name(SEMIBOLD.into())));
        // Used by the search window and breadcrumbs.
        for c in ['↑', '↓', '›', '…', '«', '»'] {
            assert!(has(c, FontFamily::Proportional), "{c:?}");
        }

        // The icon family is Lucide alone, and text families never use it.
        let fonts = fonts();
        let icons = FontFamily::Name(crate::app::icons::FAMILY.into());
        assert_eq!(fonts.families[&icons], ["Lucide"]);
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            FontFamily::Name(SEMIBOLD.into()),
        ] {
            let list = &fonts.families[&family];
            assert!(!list.iter().any(|f| f == "Lucide"), "{family:?}: {list:?}");
        }
        // Every icon we define exists in the icon font.
        for (name, icon) in crate::app::icons::ALL {
            let c = icon.chars().next().unwrap_or(' ');
            assert!(has(c, icons.clone()), "icon {name} ({c:?}) missing");
        }
    }
}
