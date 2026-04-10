use crate::models::SessionStatus;

/// Stateless checker that identifies sessions idle beyond a configurable timeout.
///
/// Called by MonitorService each tick with a fresh `&[SessionStatus]` slice and the
/// current Unix timestamp. Returns the names of sessions whose last-activity timestamp
/// is strictly more than `timeout_secs` seconds in the past.
pub struct InactivityDetector {
    timeout_secs: u64,
}

impl InactivityDetector {
    /// Create a new detector with the given timeout in seconds.
    ///
    /// The caller is expected to convert `inactivity_timeout_mins` from the config
    /// by multiplying by 60 before passing it here.
    pub fn new(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }

    /// Return the names of sessions that have been inactive for strictly longer than
    /// `timeout_secs`. A session is considered inactive when
    /// `now - session.last_activity > timeout_secs`.
    ///
    /// `now` is passed in rather than read from the system clock so that callers and
    /// tests can control the reference time.
    pub fn check_inactive(&self, sessions: &[SessionStatus], now: i64) -> Vec<String> {
        sessions
            .iter()
            .filter(|s| {
                let elapsed = now - s.last_activity;
                // Cast timeout to i64 for comparison; u64::MAX safely fits in i64 for
                // practical timeout values (centuries).
                elapsed > self.timeout_secs as i64
            })
            .map(|s| s.name.clone())
            .collect()
    }

    /// Update the inactivity timeout, e.g. when the config is hot-reloaded.
    pub fn update_timeout(&mut self, timeout_secs: u64) {
        self.timeout_secs = timeout_secs;
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SessionStats;

    fn make_session(name: &str, last_activity: i64) -> SessionStatus {
        SessionStatus {
            name: name.to_string(),
            stats: SessionStats {
                cpu_percent: 0.0,
                memory_bytes: 0,
            },
            last_activity,
            created: 0,
            attached_clients: 0,
            foreground_command: String::new(),
        }
    }

    #[test]
    fn test_no_inactive_sessions() {
        let detector = InactivityDetector::new(300); // 5 min timeout
        let now = 1_000_000_i64;
        let sessions = vec![
            make_session("alpha", now - 100),
            make_session("beta", now - 200),
            make_session("gamma", now - 300),
        ];
        let result = detector.check_inactive(&sessions, now);
        assert!(
            result.is_empty(),
            "expected no inactive sessions, got {result:?}"
        );
    }

    #[test]
    fn test_one_inactive_out_of_three() {
        let detector = InactivityDetector::new(300);
        let now = 1_000_000_i64;
        let sessions = vec![
            make_session("active1", now - 100),
            make_session("idle", now - 500), // 500 > 300 → inactive
            make_session("active2", now - 299),
        ];
        let result = detector.check_inactive(&sessions, now);
        assert_eq!(result, vec!["idle".to_string()]);
    }

    #[test]
    fn test_all_sessions_inactive() {
        let detector = InactivityDetector::new(60);
        let now = 1_000_000_i64;
        let sessions = vec![
            make_session("s1", now - 61),
            make_session("s2", now - 120),
            make_session("s3", now - 999),
        ];
        let mut result = detector.check_inactive(&sessions, now);
        result.sort();
        assert_eq!(result, vec!["s1", "s2", "s3"]);
    }

    #[test]
    fn test_empty_session_list() {
        let detector = InactivityDetector::new(300);
        let result = detector.check_inactive(&[], 1_000_000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_boundary_exactly_at_timeout_is_not_inactive() {
        let detector = InactivityDetector::new(300);
        let now = 1_000_000_i64;
        // elapsed == timeout_secs exactly → should NOT be considered inactive
        let sessions = vec![make_session("boundary", now - 300)];
        let result = detector.check_inactive(&sessions, now);
        assert!(
            result.is_empty(),
            "session exactly at timeout boundary should not be flagged"
        );
    }

    #[test]
    fn test_one_past_boundary_is_inactive() {
        let detector = InactivityDetector::new(300);
        let now = 1_000_000_i64;
        // elapsed == timeout_secs + 1 → should be inactive
        let sessions = vec![make_session("just_over", now - 301)];
        let result = detector.check_inactive(&sessions, now);
        assert_eq!(result, vec!["just_over".to_string()]);
    }

    #[test]
    fn test_update_timeout_changes_behavior() {
        let mut detector = InactivityDetector::new(600); // 10 min
        let now = 1_000_000_i64;
        let sessions = vec![make_session("session", now - 400)];

        // With 600 s timeout, 400 s idle is still active.
        assert!(detector.check_inactive(&sessions, now).is_empty());

        // After reducing timeout to 300 s, the same session becomes inactive.
        detector.update_timeout(300);
        let result = detector.check_inactive(&sessions, now);
        assert_eq!(result, vec!["session".to_string()]);
    }
}
