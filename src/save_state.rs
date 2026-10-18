//! Save scheduling for the open document, independent of egui and SQL.
//!
//! Time is passed in explicitly so the rules can be tested without a clock.
use std::time::{Duration, Instant};

/// Pause after the last edit before autosaving.
pub const DEBOUNCE: Duration = Duration::from_millis(650);
/// First retry delay after a failed save; doubles on each failure.
pub const FIRST_RETRY: Duration = Duration::from_secs(1);
/// Upper bound for the retry delay.
pub const MAX_RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveStatus<'a> {
    Saved,
    Dirty,
    Failed { message: &'a str },
}

#[derive(Debug, Default)]
pub struct SaveState {
    dirty: bool,
    next_attempt: Option<Instant>,
    failures: u32,
    error: Option<String>,
}

impl SaveState {
    /// Records an edit; the autosave waits for a pause, and never runs
    /// earlier than an already scheduled retry.
    pub fn mark_dirty(&mut self, now: Instant) {
        self.dirty = true;
        let after_pause = now + DEBOUNCE;
        self.next_attempt = Some(match (self.error.is_some(), self.next_attempt) {
            (true, Some(retry_at)) => retry_at.max(after_pause),
            _ => after_pause,
        });
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// True when an automatic save attempt should run now.
    pub fn is_due(&self, now: Instant) -> bool {
        self.dirty && self.next_attempt.is_some_and(|at| at <= now)
    }

    /// When the UI must wake up for the next automatic attempt.
    pub fn next_attempt(&self) -> Option<Instant> {
        self.next_attempt.filter(|_| self.dirty)
    }

    pub fn record_success(&mut self) {
        *self = Self::default();
    }

    /// Keeps the document dirty and schedules a retry with exponential backoff.
    pub fn record_failure(&mut self, now: Instant, message: impl Into<String>) {
        self.failures = self.failures.saturating_add(1);
        self.error = Some(message.into());
        self.next_attempt = Some(now + retry_delay(self.failures));
    }

    /// The user explicitly chose to drop unsaved changes.
    pub fn discard(&mut self) {
        *self = Self::default();
    }

    pub fn status(&self) -> SaveStatus<'_> {
        match (&self.error, self.dirty) {
            (Some(message), true) => SaveStatus::Failed { message },
            (_, true) => SaveStatus::Dirty,
            _ => SaveStatus::Saved,
        }
    }

    /// Closing is only safe when nothing is pending.
    pub fn can_close(&self) -> bool {
        !self.dirty
    }
}

/// Delay before retry number `failures` (1-based): 1 s, 2 s, 4 s… up to 30 s.
pub fn retry_delay(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(16);
    FIRST_RETRY.saturating_mul(1 << exponent).min(MAX_RETRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn starts_saved_and_closable() {
        let state = SaveState::default();
        assert_eq!(state.status(), SaveStatus::Saved);
        assert!(state.can_close());
        assert_eq!(state.next_attempt(), None);
    }

    #[test]
    fn edit_waits_for_debounce() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        assert_eq!(state.status(), SaveStatus::Dirty);
        assert!(!state.can_close());
        assert!(!state.is_due(t0 + secs(0.6)));
        assert!(state.is_due(t0 + DEBOUNCE));
    }

    #[test]
    fn typing_postpones_autosave() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        state.mark_dirty(t0 + secs(0.5));
        assert!(!state.is_due(t0 + DEBOUNCE));
        assert!(state.is_due(t0 + secs(0.5) + DEBOUNCE));
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        let delays: Vec<_> = (1..=7).map(retry_delay).collect();
        assert_eq!(
            delays,
            [1, 2, 4, 8, 16, 30, 30].map(Duration::from_secs).to_vec()
        );
        assert_eq!(retry_delay(u32::MAX), MAX_RETRY);
    }

    #[test]
    fn failure_keeps_changes_pending_and_waits_for_backoff() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        state.record_failure(t0 + DEBOUNCE, "disco lleno");
        assert_eq!(
            state.status(),
            SaveStatus::Failed {
                message: "disco lleno"
            }
        );
        assert!(!state.can_close());
        assert!(!state.is_due(t0 + DEBOUNCE + secs(0.9)));
        assert!(state.is_due(t0 + DEBOUNCE + secs(1.0)));

        state.record_failure(t0 + secs(2.0), "disco lleno");
        assert!(!state.is_due(t0 + secs(3.9)));
        assert!(state.is_due(t0 + secs(4.0)));
    }

    #[test]
    fn editing_after_failure_does_not_retry_sooner() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        state.record_failure(t0, "bloqueada");
        state.record_failure(t0, "bloqueada"); // next retry at t0 + 2 s
        state.mark_dirty(t0 + secs(0.1));
        assert!(!state.is_due(t0 + secs(1.9)));
        assert!(state.is_due(t0 + secs(2.0)));
        assert!(matches!(state.status(), SaveStatus::Failed { .. }));
    }

    #[test]
    fn success_clears_error_and_backoff() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        state.record_failure(t0, "error");
        state.record_success();
        assert_eq!(state.status(), SaveStatus::Saved);
        assert!(state.can_close());
        state.mark_dirty(t0 + secs(10.0));
        assert!(state.is_due(t0 + secs(10.0) + DEBOUNCE));
    }

    #[test]
    fn discard_allows_close() {
        let t0 = Instant::now();
        let mut state = SaveState::default();
        state.mark_dirty(t0);
        state.record_failure(t0, "error");
        state.discard();
        assert!(state.can_close());
        assert_eq!(state.next_attempt(), None);
    }
}
