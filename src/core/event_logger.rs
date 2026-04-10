use anyhow::Result;
use chrono::Utc;

use crate::infra::log_store::LogStore;
use crate::models::{LogEvent, RestartPhase};

// ---------------------------------------------------------------------------
// EventLogger
// ---------------------------------------------------------------------------

/// Core-layer wrapper around `LogStore` that provides typed convenience
/// methods for creating and persisting structured `LogEvent`s.
pub struct EventLogger {
    store: LogStore,
}

impl EventLogger {
    /// Create a new `EventLogger` backed by the given `LogStore`.
    pub fn new(store: LogStore) -> Self {
        Self { store }
    }

    /// Log a pre-constructed `LogEvent`. Thin wrapper over `LogStore::insert_event`.
    pub fn log(&self, event: &LogEvent) -> Result<()> {
        self.store.insert_event(event)
    }

    /// Convenience: log an `FdSpike` event with the current UTC timestamp.
    pub fn log_fd_spike(&self, pct: u8) -> Result<()> {
        self.log(&LogEvent::FdSpike {
            pct,
            timestamp: Utc::now(),
        })
    }

    /// Convenience: log a `SessionCreated` event.
    pub fn log_session_created(&self, name: &str) -> Result<()> {
        self.log(&LogEvent::SessionCreated {
            name: name.to_owned(),
        })
    }

    /// Convenience: log a `SessionDestroyed` event.
    pub fn log_session_destroyed(&self, name: &str) -> Result<()> {
        self.log(&LogEvent::SessionDestroyed {
            name: name.to_owned(),
        })
    }

    /// Convenience: log a `SafeRestart` event.
    pub fn log_safe_restart(&self, phase: RestartPhase, success: bool) -> Result<()> {
        self.log(&LogEvent::SafeRestart { phase, success })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::log_store::LogStore;
    use crate::models::RestartPhase;
    use tempfile::tempdir;

    fn make_logger() -> (EventLogger, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let store = LogStore::new(&dir.path().join("test.db")).expect("LogStore::new");
        (EventLogger::new(store), dir)
    }

    // ------------------------------------------------------------------
    // Test 1: log_fd_spike — event is persisted and readable
    // ------------------------------------------------------------------
    #[test]
    fn fd_spike_is_stored() {
        let (logger, _dir) = make_logger();

        logger.log_fd_spike(88).expect("log_fd_spike");

        let rows = logger
            .store
            .query_events(Some("FdSpike"), 10)
            .expect("query_events");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event_type, "FdSpike");
        assert!(rows[0].payload_json.contains("88"));
    }

    // ------------------------------------------------------------------
    // Test 2: log_session_created / log_session_destroyed
    // ------------------------------------------------------------------
    #[test]
    fn session_created_and_destroyed_are_stored() {
        let (logger, _dir) = make_logger();

        logger
            .log_session_created("main")
            .expect("log_session_created");
        logger
            .log_session_destroyed("old")
            .expect("log_session_destroyed");

        let created = logger
            .store
            .query_events(Some("SessionCreated"), 10)
            .expect("query SessionCreated");
        assert_eq!(created.len(), 1);
        assert!(created[0].payload_json.contains("main"));

        let destroyed = logger
            .store
            .query_events(Some("SessionDestroyed"), 10)
            .expect("query SessionDestroyed");
        assert_eq!(destroyed.len(), 1);
        assert!(destroyed[0].payload_json.contains("old"));
    }

    // ------------------------------------------------------------------
    // Test 3: log_safe_restart — all phases can be logged
    // ------------------------------------------------------------------
    #[test]
    fn safe_restart_phases_are_stored() {
        let (logger, _dir) = make_logger();

        let phases = [
            RestartPhase::SnapshotSave,
            RestartPhase::ServerKill,
            RestartPhase::ServerStart,
            RestartPhase::SnapshotRestore,
        ];

        for phase in phases {
            logger
                .log_safe_restart(phase, true)
                .expect("log_safe_restart");
        }

        let rows = logger
            .store
            .query_events(Some("SafeRestart"), 10)
            .expect("query SafeRestart");

        assert_eq!(rows.len(), 4);
        // All rows should be SafeRestart
        for row in &rows {
            assert_eq!(row.event_type, "SafeRestart");
        }
    }

    // ------------------------------------------------------------------
    // Test 4: multiple events — verify total count
    // ------------------------------------------------------------------
    #[test]
    fn multiple_events_count() {
        let (logger, _dir) = make_logger();

        logger.log_fd_spike(50).expect("fd_spike 1");
        logger.log_fd_spike(60).expect("fd_spike 2");
        logger.log_session_created("alpha").expect("session alpha");
        logger.log_session_created("beta").expect("session beta");
        logger
            .log_safe_restart(RestartPhase::SnapshotSave, false)
            .expect("safe_restart");

        let all = logger.store.query_events(None, 100).expect("query all");
        assert_eq!(all.len(), 5);

        let fd_rows = logger
            .store
            .query_events(Some("FdSpike"), 10)
            .expect("query FdSpike");
        assert_eq!(fd_rows.len(), 2);
    }

    // ------------------------------------------------------------------
    // Test 5: log() thin-wrapper works with a pre-built LogEvent
    // ------------------------------------------------------------------
    #[test]
    fn log_raw_event() {
        let (logger, _dir) = make_logger();

        let event = LogEvent::SessionCreated {
            name: "direct".to_owned(),
        };
        logger.log(&event).expect("log raw event");

        let rows = logger
            .store
            .query_events(Some("SessionCreated"), 10)
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].payload_json.contains("direct"));
    }
}
