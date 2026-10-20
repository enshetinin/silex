//! Open tabs (ADR 0011), without egui or SQL: which pages are open, which
//! one is shown, and how they are remembered between sessions.
//!
//! A page is in one tab at most. There are at most [`MAX_TABS`]: opening
//! another closes the one used longest ago, never the active one. Closing
//! the last tab is refused (the window always shows a page).
use serde_json::{Value, json};

/// Most tabs open at once.
pub const MAX_TABS: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tabs {
    /// Page ids, in tab order.
    ids: Vec<String>,
    /// When each tab was last shown (a counter), for closing the oldest.
    used: Vec<u64>,
    active: usize,
    clock: u64,
}

impl Tabs {
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The page shown, if any tab is open.
    pub fn active_id(&self) -> Option<&str> {
        self.ids.get(self.active).map(String::as_str)
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.ids.iter().position(|t| t == id)
    }

    fn touch(&mut self, index: usize) {
        self.clock += 1;
        self.active = index;
        if let Some(used) = self.used.get_mut(index) {
            *used = self.clock;
        }
    }

    /// Shows `id` in the active tab (a click on a page). If another tab has
    /// it, that tab is shown instead. With no tabs, opens the first one.
    /// Returns the page that left the tabs, if any.
    pub fn open_here(&mut self, id: &str) -> Option<String> {
        if let Some(index) = self.position(id) {
            self.touch(index);
            return None;
        }
        if self.ids.is_empty() {
            self.ids.push(id.to_owned());
            self.used.push(0);
            self.touch(0);
            return None;
        }
        let left = std::mem::replace(&mut self.ids[self.active], id.to_owned());
        self.touch(self.active);
        Some(left)
    }

    /// Opens `id` in a new tab after the active one and shows it (or shows
    /// the tab that has it). Returns the pages whose tabs were closed to
    /// stay within [`MAX_TABS`].
    pub fn open_new(&mut self, id: &str) -> Vec<String> {
        if let Some(index) = self.position(id) {
            self.touch(index);
            return Vec::new();
        }
        let at = if self.ids.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.ids.insert(at, id.to_owned());
        self.used.insert(at, 0);
        self.touch(at);
        let mut closed = Vec::new();
        while self.ids.len() > MAX_TABS {
            // The one used longest ago, never the active one.
            let oldest = (0..self.ids.len())
                .filter(|&i| i != self.active)
                .min_by_key(|&i| self.used[i]);
            let Some(oldest) = oldest else { break };
            closed.extend(self.remove(oldest));
        }
        closed
    }

    /// Shows tab `index`; false if there is none.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.ids.len() {
            return false;
        }
        self.touch(index);
        true
    }

    /// The next (or previous) tab, going round.
    pub fn cycle(&mut self, forward: bool) {
        let n = self.ids.len();
        if n > 1 {
            let next = if forward {
                (self.active + 1) % n
            } else {
                (self.active + n - 1) % n
            };
            self.touch(next);
        }
    }

    /// Closes tab `index`, unless it is the only one. The tab on its right
    /// (or left, for the last) is shown if it was the active one. Returns
    /// the page closed.
    pub fn close(&mut self, index: usize) -> Option<String> {
        if self.ids.len() <= 1 {
            return None;
        }
        let was_active = index == self.active;
        let closed = self.remove(index)?;
        if was_active {
            let next = index.min(self.ids.len() - 1);
            self.touch(next);
        }
        Some(closed)
    }

    /// Removes tab `index` (even the last one), keeping the active page.
    fn remove(&mut self, index: usize) -> Option<String> {
        if index >= self.ids.len() {
            return None;
        }
        self.used.remove(index);
        let id = self.ids.remove(index);
        if index < self.active || (index == self.active && self.active >= self.ids.len()) {
            self.active = self.active.saturating_sub(1);
        }
        Some(id)
    }

    /// Keeps the tabs of pages for which `live` is true (a page sent to the
    /// trash closes its tab). Returns the pages closed.
    pub fn retain(&mut self, live: impl Fn(&str) -> bool) -> Vec<String> {
        let active = self.active_id().map(str::to_owned);
        let mut closed = Vec::new();
        let mut index = 0;
        while index < self.ids.len() {
            if live(&self.ids[index]) {
                index += 1;
            } else {
                closed.extend(self.remove(index));
            }
        }
        // The active page, if it stayed; else its neighbour (already set).
        if let Some(position) = active.as_deref().and_then(|id| self.position(id)) {
            self.active = position;
        }
        self.active = self.active.min(self.ids.len().saturating_sub(1));
        closed
    }

    /// What is stored to reopen them: `{"tabs": [ids], "active": n}`.
    pub fn to_json(&self) -> String {
        json!({"tabs": self.ids, "active": self.active}).to_string()
    }

    /// Reads [`Tabs::to_json`] leniently: anything that is not a list of
    /// ids gives no tabs, repeated ids are dropped, at most [`MAX_TABS`],
    /// and a bad active index shows the first tab.
    pub fn from_json(text: &str) -> Self {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let mut tabs = Self::default();
        if let Some(Value::Array(items)) = map.get("tabs") {
            for id in items.iter().filter_map(Value::as_str) {
                if !id.is_empty() && tabs.position(id).is_none() && tabs.len() < MAX_TABS {
                    tabs.ids.push(id.to_owned());
                    tabs.used.push(0);
                }
            }
        }
        let active = map
            .get("active")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n < tabs.len())
            .unwrap_or(0);
        if !tabs.is_empty() {
            tabs.touch(active);
        }
        tabs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(ids: &[&str], active: usize) -> Tabs {
        let mut tabs = Tabs::default();
        for id in ids {
            tabs.open_new(id);
        }
        tabs.select(active);
        tabs
    }

    fn ids(tabs: &Tabs) -> Vec<&str> {
        tabs.ids().iter().map(String::as_str).collect()
    }

    #[test]
    fn a_click_shows_the_page_in_the_active_tab_or_its_own() {
        let mut t = Tabs::default();
        assert_eq!(t.open_here("a"), None);
        assert_eq!((ids(&t), t.active_id()), (vec!["a"], Some("a")));
        assert_eq!(t.open_here("b"), Some("a".into()), "a leaves the tabs");
        assert_eq!(ids(&t), ["b"]);
        let mut t = tabs(&["a", "b", "c"], 0);
        // «c» already has a tab: it is shown, nothing is replaced.
        assert_eq!(t.open_here("c"), None);
        assert_eq!((ids(&t), t.active()), (vec!["a", "b", "c"], 2));
    }

    #[test]
    fn new_tabs_go_after_the_active_one() {
        let mut t = tabs(&["a", "b"], 0);
        assert!(t.open_new("x").is_empty());
        assert_eq!((ids(&t), t.active()), (vec!["a", "x", "b"], 1));
        // A page with a tab is shown, not opened twice.
        t.open_new("b");
        assert_eq!((ids(&t), t.active()), (vec!["a", "x", "b"], 2));
    }

    #[test]
    fn the_thirteenth_tab_closes_the_one_used_longest_ago() {
        let names: Vec<String> = (0..MAX_TABS).map(|i| format!("p{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut t = tabs(&refs, 0);
        // p0 is active (just used); p1 is the oldest after it.
        t.select(5);
        t.select(0);
        let closed = t.open_new("nueva");
        assert_eq!(closed, ["p1"]);
        assert_eq!(t.len(), MAX_TABS);
        assert_eq!(t.active_id(), Some("nueva"));
        assert!(t.position("p0").is_some() && t.position("p5").is_some());
    }

    #[test]
    fn closing_shows_the_neighbour_and_never_the_last_tab() {
        let mut t = tabs(&["a", "b", "c"], 1);
        assert_eq!(t.close(1), Some("b".into()));
        assert_eq!((ids(&t), t.active_id()), (vec!["a", "c"], Some("c")));
        assert_eq!(t.close(1), Some("c".into()), "the last one: its left");
        assert_eq!(t.active_id(), Some("a"));
        assert_eq!(t.close(0), None, "the only tab stays");
        // Closing another tab keeps the active page.
        let mut t = tabs(&["a", "b", "c"], 2);
        t.close(0);
        assert_eq!(t.active_id(), Some("c"));
        assert_eq!(t.close(7), None);
    }

    #[test]
    fn tabs_go_round_and_select_by_number() {
        let mut t = tabs(&["a", "b", "c"], 2);
        t.cycle(true);
        assert_eq!(t.active_id(), Some("a"));
        t.cycle(false);
        assert_eq!(t.active_id(), Some("c"));
        assert!(t.select(1));
        assert!(!t.select(3));
        assert_eq!(t.active_id(), Some("b"));
    }

    #[test]
    fn pages_not_live_close_their_tabs() {
        let mut t = tabs(&["a", "b", "c", "d"], 2);
        assert_eq!(t.retain(|id| id != "a" && id != "d"), ["a", "d"]);
        assert_eq!((ids(&t), t.active_id()), (vec!["b", "c"], Some("c")));
        // The active one too: its neighbour is shown.
        assert_eq!(t.retain(|id| id != "c"), ["c"]);
        assert_eq!(t.active_id(), Some("b"));
        assert_eq!(t.retain(|_| false), ["b"]);
        assert!(t.is_empty() && t.active_id().is_none());
    }

    #[test]
    fn tabs_are_remembered_and_read_leniently() {
        let t = tabs(&["a", "b", "c"], 1);
        let back = Tabs::from_json(&t.to_json());
        assert_eq!(
            (ids(&back), back.active_id()),
            (vec!["a", "b", "c"], Some("b"))
        );
        assert!(Tabs::from_json("no es json").is_empty());
        assert!(Tabs::from_json(r#"{"tabs": "a"}"#).is_empty());
        let odd = Tabs::from_json(r#"{"tabs": ["a", 3, "", "a", "b"], "active": 9}"#);
        assert_eq!((ids(&odd), odd.active_id()), (vec!["a", "b"], Some("a")));
        let many: Vec<String> = (0..30).map(|i| format!("\"p{i}\"")).collect();
        let many = Tabs::from_json(&format!(r#"{{"tabs": [{}]}}"#, many.join(",")));
        assert_eq!(many.len(), MAX_TABS);
    }
}
