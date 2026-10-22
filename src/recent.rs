//! Pages opened recently (H11.8), listed when the search window opens with
//! nothing typed. Pure: the app says which pages are live and stores the
//! list in `settings` as JSON, read leniently like the tabs (ADR 0011).
use serde_json::{Value, json};

/// Pages remembered: more than are shown, so pages sent to the trash (and
/// restored later) or templates do not leave the list short.
pub const MAX_STORED: usize = 20;

/// Pages shown in the search window.
pub const MAX_SHOWN: usize = 8;

/// Page ids, most recent first, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    ids: Vec<String>,
}

impl Recent {
    /// `id` was opened: it goes first.
    pub fn visit(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        self.ids.retain(|known| known != id);
        self.ids.insert(0, id.to_owned());
        self.ids.truncate(MAX_STORED);
    }

    /// What the search window lists: up to [`MAX_SHOWN`] pages that `live`
    /// accepts (not in the trash, not a template), most recent first,
    /// without `open` (the page on screen).
    pub fn shown<'a>(&'a self, open: Option<&str>, live: impl Fn(&str) -> bool) -> Vec<&'a str> {
        self.ids
            .iter()
            .map(String::as_str)
            .filter(|id| Some(*id) != open && live(id))
            .take(MAX_SHOWN)
            .collect()
    }

    pub fn to_json(&self) -> String {
        json!({ "pages": self.ids }).to_string()
    }

    /// Reads [`Recent::to_json`] leniently: anything that is not a list of
    /// ids gives an empty list; empty and repeated ids are dropped; at
    /// most [`MAX_STORED`].
    pub fn from_json(text: &str) -> Self {
        let mut recent = Self::default();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return recent;
        };
        if let Some(Value::Array(items)) = map.get("pages") {
            for id in items.iter().filter_map(Value::as_str) {
                if !id.is_empty() && !recent.ids.iter().any(|known| known == id) {
                    recent.ids.push(id.to_owned());
                }
            }
        }
        recent.ids.truncate(MAX_STORED);
        recent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(recent: &Recent) -> Vec<&str> {
        recent.shown(None, |_| true)
    }

    #[test]
    fn a_visit_goes_first_and_is_never_repeated() {
        let mut recent = Recent::default();
        for id in ["a", "b", "c", "a", ""] {
            recent.visit(id);
        }
        assert_eq!(ids(&recent), ["a", "c", "b"]);
    }

    #[test]
    fn at_most_the_stored_and_the_shown_limits() {
        let mut recent = Recent::default();
        for n in 0..30 {
            recent.visit(&n.to_string());
        }
        assert_eq!(recent.ids.len(), MAX_STORED);
        assert_eq!(recent.ids[0], "29");
        assert_eq!(
            ids(&recent),
            ["29", "28", "27", "26", "25", "24", "23", "22"]
        );
    }

    #[test]
    fn the_open_page_and_pages_not_live_are_left_out() {
        let mut recent = Recent::default();
        for id in ["papelera", "b", "plantilla", "a", "abierta"] {
            recent.visit(id);
        }
        let shown = recent.shown(Some("abierta"), |id| {
            !matches!(id, "papelera" | "plantilla")
        });
        assert_eq!(shown, ["a", "b"]);
        // Pages not live still count towards what is stored, not shown.
        let mut many = Recent::default();
        for n in 0..12 {
            many.visit(&n.to_string());
        }
        let even = many.shown(None, |id| id.parse::<u32>().is_ok_and(|n| n % 2 == 0));
        assert_eq!(even, ["10", "8", "6", "4", "2", "0"]);
    }

    #[test]
    fn json_round_trips_and_bad_json_is_empty() {
        let mut recent = Recent::default();
        recent.visit("b");
        recent.visit("a");
        assert_eq!(Recent::from_json(&recent.to_json()), recent);
        for bad in [
            "",
            "nope",
            "[]",
            r#"{"pages": "a"}"#,
            r#"{"pages": [1, null]}"#,
        ] {
            assert_eq!(Recent::from_json(bad), Recent::default(), "{bad}");
        }
        let messy = Recent::from_json(r#"{"pages": ["a", "", "a", 3, "b"], "extra": 1}"#);
        assert_eq!(ids(&messy), ["a", "b"]);
    }
}
