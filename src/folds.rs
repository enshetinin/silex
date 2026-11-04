//! Folded toggle blocks (ADR 0016): view state, not part of the document.
//! Kept in `settings` as a JSON list of block ids, like the open tabs.
use std::collections::BTreeSet;

/// The toggles folded on this computer. A toggle not in it is open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folds(BTreeSet<String>);

impl Folds {
    /// From [`Folds::to_json`]. A damaged value is no folds: everything
    /// shows, nothing is lost.
    pub fn from_json(json: &str) -> Self {
        Self(serde_json::from_str(json).unwrap_or_default())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_else(|_| "[]".to_owned())
    }

    pub fn is_folded(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    /// Folds or opens toggle `id`. Returns whether that changed anything.
    pub fn set(&mut self, id: &str, folded: bool) -> bool {
        if folded {
            self.0.insert(id.to_owned())
        } else {
            self.0.remove(id)
        }
    }

    /// Keeps only the toggles `live` still knows (at start, so deleted
    /// blocks do not pile up).
    pub fn retain(&mut self, live: impl Fn(&str) -> bool) {
        self.0.retain(|id| live(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_round_trip_and_survive_damage() {
        let mut folds = Folds::default();
        assert!(folds.set("b", true));
        assert!(folds.set("a", true));
        assert!(!folds.set("a", true), "already folded");
        assert_eq!(folds.to_json(), r#"["a","b"]"#);
        assert_eq!(Folds::from_json(&folds.to_json()), folds);
        assert!(folds.set("a", false));
        assert!(!folds.is_folded("a") && folds.is_folded("b"));
        folds.retain(|id| id != "b");
        assert_eq!(folds, Folds::default());
        for damaged in ["", "{", r#"{"a":1}"#, "[1, 2]"] {
            assert_eq!(Folds::from_json(damaged), Folds::default(), "{damaged}");
        }
    }
}
