//! User preferences. Stored as key/value text in the `settings` table (see
//! `db`); unknown keys and invalid values are ignored, so a damaged or newer
//! row can never stop the app from starting.
use std::ops::RangeInclusive;

/// Light, dark, or whatever the operating system uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

    /// Value stored in the database.
    pub fn key(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "Sistema",
            ThemeMode::Light => "Claro",
            ThemeMode::Dark => "Oscuro",
        }
    }
}

/// Interface sizes offered in the settings menu (percent).
pub const ZOOM_STEPS: [u16; 6] = [80, 90, 100, 110, 125, 150];
/// Accepted interface sizes (keyboard zoom can land between the steps).
pub const ZOOM_RANGE: RangeInclusive<u16> = 50..=200;

const THEME_KEY: &str = "theme";
const ZOOM_KEY: &str = "zoom_percent";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    /// Interface size in percent (text and controls scale together).
    pub zoom_percent: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            zoom_percent: 100,
        }
    }
}

impl Settings {
    /// Builds settings from stored rows, keeping the default for anything
    /// missing or invalid.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut settings = Self::default();
        for (key, value) in pairs {
            match key {
                THEME_KEY => {
                    if let Some(mode) = ThemeMode::from_key(value) {
                        settings.theme = mode;
                    }
                }
                ZOOM_KEY => {
                    if let Ok(zoom) = value.trim().parse::<u16>()
                        && ZOOM_RANGE.contains(&zoom)
                    {
                        settings.zoom_percent = zoom;
                    }
                }
                _ => {}
            }
        }
        settings
    }

    /// Rows to store.
    pub fn to_pairs(&self) -> [(&'static str, String); 2] {
        [
            (THEME_KEY, self.theme.key().to_owned()),
            (ZOOM_KEY, self.zoom_percent.to_string()),
        ]
    }

    /// Clamps a zoom factor (e.g. from the keyboard) to a stored percent.
    pub fn zoom_from_factor(factor: f32) -> u16 {
        let percent = (factor * 100.0).round();
        let (min, max) = (*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        if percent.is_nan() {
            100
        } else {
            percent.clamp(f32::from(min), f32::from(max)) as u16
        }
    }
}

/// The next interface size in [`ZOOM_STEPS`] above (`up`) or below the
/// current one; `None` at the ends. A size between steps (keyboard zoom)
/// goes to the nearest step in that direction.
pub fn zoom_step(current: u16, up: bool) -> Option<u16> {
    if up {
        ZOOM_STEPS.into_iter().find(|&step| step > current)
    } else {
        ZOOM_STEPS.into_iter().rev().find(|&step| step < current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_up_and_down_and_stop_at_the_ends() {
        assert_eq!(zoom_step(100, true), Some(110));
        assert_eq!(zoom_step(100, false), Some(90));
        assert_eq!(zoom_step(150, true), None);
        assert_eq!(zoom_step(80, false), None);
        // Between steps (e.g. 115 % from the keyboard), or outside them.
        assert_eq!(zoom_step(115, true), Some(125));
        assert_eq!(zoom_step(115, false), Some(110));
        assert_eq!(zoom_step(200, false), Some(150));
        assert_eq!(zoom_step(50, true), Some(80));
    }

    #[test]
    fn roundtrip_through_pairs() {
        let settings = Settings {
            theme: ThemeMode::Dark,
            zoom_percent: 125,
        };
        let pairs = settings.to_pairs();
        let back = Settings::from_pairs(pairs.iter().map(|(k, v)| (*k, v.as_str())));
        assert_eq!(back, settings);
    }

    #[test]
    fn invalid_or_unknown_values_keep_defaults() {
        let settings = Settings::from_pairs([
            ("theme", "violeta"),
            ("zoom_percent", "9000"),
            ("algo_futuro", "x"),
        ]);
        assert_eq!(settings, Settings::default());
        let settings = Settings::from_pairs([("zoom_percent", "abc"), ("theme", "light")]);
        assert_eq!(settings.theme, ThemeMode::Light);
        assert_eq!(settings.zoom_percent, 100);
    }

    #[test]
    fn zoom_factor_is_rounded_and_clamped() {
        assert_eq!(Settings::zoom_from_factor(1.1), 110);
        assert_eq!(Settings::zoom_from_factor(0.1), 50);
        assert_eq!(Settings::zoom_from_factor(9.0), 200);
        assert_eq!(Settings::zoom_from_factor(f32::NAN), 100);
    }
}
