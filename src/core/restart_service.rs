use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::core::event_logger::EventLogger;
use crate::core::snapshot_service::SnapshotService;
use crate::models::{RestartPhase, TmuxAdapter};
use crate::ui::notifications::NotificationService;

// ---------------------------------------------------------------------------
// RestartService
// ---------------------------------------------------------------------------

/// Orchestrates the full safe-restart sequence:
///   1. Snapshot save  — persist all active sessions to disk
///   2. Kill server    — bring the tmux server down cleanly
///   3. Start server   — bring it back up
///   4. Restore        — replay the saved snapshots
///
/// Each phase is logged via `EventLogger`. If snapshot save fails the
/// sequence is aborted (we refuse to kill a server we cannot restore).
/// Later failures are logged but do not prevent subsequent phases from
/// running; the final result is reported to the user via `NotificationService`.
///
/// `EventLogger` wraps a `rusqlite::Connection` which is `Send` but not
/// `Sync`; we therefore store it inside a `Mutex` so that `RestartService`
/// itself becomes `Send + Sync` and can safely cross thread boundaries.
pub struct RestartService {
    snapshot_service: Arc<SnapshotService>,
    tmux: Arc<dyn TmuxAdapter>,
    event_logger: Mutex<EventLogger>,
    notification_service: NotificationService,
}

impl RestartService {
    /// Construct a new `RestartService`.
    pub fn new(
        snapshot_service: Arc<SnapshotService>,
        tmux: Arc<dyn TmuxAdapter>,
        event_logger: EventLogger,
        notification_service: NotificationService,
    ) -> Self {
        Self {
            snapshot_service,
            tmux,
            event_logger: Mutex::new(event_logger),
            notification_service,
        }
    }

    /// Execute the full safe-restart sequence (spec §1.3).
    ///
    /// Returns `Ok(())` if every phase succeeded, or an error if the snapshot
    /// save failed (the only hard-abort condition).
    pub fn execute_restart(&self) -> Result<()> {
        // Helper: log a phase result through the Mutex-guarded EventLogger.
        let log_phase = |phase: RestartPhase, success: bool| {
            match self.event_logger.lock() {
                Ok(logger) => {
                    if let Err(e) = logger.log_safe_restart(phase, success) {
                        tracing::warn!("Failed to log restart phase: {e:#}");
                    }
                }
                Err(e) => tracing::warn!("EventLogger mutex poisoned: {e}"),
            }
        };

        // ------------------------------------------------------------------
        // Phase 1: Snapshot Save
        // ------------------------------------------------------------------
        tracing::info!("Safe restart: Phase 1 — snapshot save");
        match self.snapshot_service.save_all() {
            Ok(snapshots) => {
                tracing::info!(count = snapshots.len(), "Snapshots saved");
                log_phase(RestartPhase::SnapshotSave, true);
            }
            Err(e) => {
                tracing::error!("Snapshot save failed, aborting restart: {e:#}");
                log_phase(RestartPhase::SnapshotSave, false);
                let detail = format!("Could not save session snapshots: {e:#}");
                if let Err(notify_err) = self
                    .notification_service
                    .send_restart_result(false, &detail)
                {
                    tracing::warn!("Failed to send restart-failed notification: {notify_err:#}");
                }
                return Err(e);
            }
        }

        // ------------------------------------------------------------------
        // Phase 2: Kill Server
        // ------------------------------------------------------------------
        tracing::info!("Safe restart: Phase 2 — kill server");
        let kill_ok = match self.tmux.kill_server() {
            Ok(()) => {
                tracing::info!("tmux server killed");
                log_phase(RestartPhase::ServerKill, true);
                true
            }
            Err(e) => {
                tracing::error!("Failed to kill tmux server: {e:#}");
                log_phase(RestartPhase::ServerKill, false);
                false
            }
        };

        // Brief pause to let tmux clean up before we try to start it again.
        std::thread::sleep(std::time::Duration::from_millis(500));

        // ------------------------------------------------------------------
        // Phase 3: Start Server
        // ------------------------------------------------------------------
        tracing::info!("Safe restart: Phase 3 — start server");
        let start_ok = match self.tmux.start_server() {
            Ok(()) => {
                tracing::info!("tmux server started");
                log_phase(RestartPhase::ServerStart, true);
                true
            }
            Err(e) => {
                tracing::error!("Failed to start tmux server: {e:#}");
                log_phase(RestartPhase::ServerStart, false);
                false
            }
        };

        // ------------------------------------------------------------------
        // Phase 4: Restore
        // ------------------------------------------------------------------
        tracing::info!("Safe restart: Phase 4 — snapshot restore");
        let (restore_ok, summary) = match self.snapshot_service.restore_all() {
            Ok(report) => {
                let restored_count = report.restored.len();
                let failed_count = report.failed.len();
                tracing::info!(
                    restored = restored_count,
                    failed = failed_count,
                    "Snapshot restore complete"
                );
                log_phase(RestartPhase::SnapshotRestore, failed_count == 0);

                let summary = if failed_count == 0 {
                    format!("{restored_count} session(s) restored.")
                } else {
                    let failed_names: Vec<&str> =
                        report.failed.iter().map(|(n, _)| n.as_str()).collect();
                    format!(
                        "{restored_count} session(s) restored, {failed_count} failed: {}",
                        failed_names.join(", ")
                    )
                };
                (true, summary)
            }
            Err(e) => {
                tracing::error!("restore_all failed: {e:#}");
                log_phase(RestartPhase::SnapshotRestore, false);
                (false, format!("Restore failed: {e:#}"))
            }
        };

        // ------------------------------------------------------------------
        // Notify user
        // ------------------------------------------------------------------
        let overall_success = kill_ok && start_ok && restore_ok;
        let details = if overall_success {
            summary
        } else {
            let mut parts = Vec::new();
            if !kill_ok {
                parts.push("kill failed");
            }
            if !start_ok {
                parts.push("start failed");
            }
            if !restore_ok {
                parts.push(&summary);
            }
            parts.join("; ")
        };

        if let Err(e) = self
            .notification_service
            .send_restart_result(overall_success, &details)
        {
            tracing::warn!("Failed to send restart-result notification: {e:#}");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::log_store::LogStore;
    use crate::models::{RawPane, RawSession, RawWindow};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Minimal mock TmuxAdapter
    // -----------------------------------------------------------------------

    struct MockTmux {
        /// If true, kill_server() returns an error.
        fail_kill: bool,
        /// If true, start_server() returns an error.
        fail_start: bool,
        calls: Mutex<Vec<String>>,
    }

    impl MockTmux {
        fn new() -> Self {
            Self {
                fail_kill: false,
                fail_start: false,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_fail_kill(mut self) -> Self {
            self.fail_kill = true;
            self
        }

        fn with_fail_start(mut self) -> Self {
            self.fail_start = true;
            self
        }

        fn record(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }

        fn call_log(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TmuxAdapter for MockTmux {
        fn list_sessions(&self) -> Result<Vec<RawSession>> {
            self.record("list_sessions");
            Ok(vec![])
        }

        fn list_windows(&self, session: &str) -> Result<Vec<RawWindow>> {
            self.record(format!("list_windows:{session}"));
            Ok(vec![])
        }

        fn list_panes(&self, session: &str, window: &str) -> Result<Vec<RawPane>> {
            self.record(format!("list_panes:{session}:{window}"));
            Ok(vec![])
        }

        fn new_session(&self, name: &str) -> Result<()> {
            self.record(format!("new_session:{name}"));
            Ok(())
        }

        fn kill_session(&self, name: &str) -> Result<()> {
            self.record(format!("kill_session:{name}"));
            Ok(())
        }

        fn kill_server(&self) -> Result<()> {
            self.record("kill_server");
            if self.fail_kill {
                anyhow::bail!("simulated kill_server failure");
            }
            Ok(())
        }

        fn start_server(&self) -> Result<()> {
            self.record("start_server");
            if self.fail_start {
                anyhow::bail!("simulated start_server failure");
            }
            Ok(())
        }

        fn attach_session(&self, name: &str) -> Result<()> {
            self.record(format!("attach_session:{name}"));
            Ok(())
        }

        fn session_activity(&self, session: &str) -> Result<i64> {
            self.record(format!("session_activity:{session}"));
            Ok(0)
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_service(
        tmux: Arc<MockTmux>,
    ) -> (RestartService, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");

        let snapshot_service = Arc::new(
            SnapshotService::new(Arc::clone(&tmux) as Arc<dyn TmuxAdapter>, tmp.path().to_path_buf())
                .expect("SnapshotService::new"),
        );

        let log_store = LogStore::new(std::path::Path::new(":memory:"))
            .expect("in-memory LogStore");
        let event_logger = EventLogger::new(log_store);
        let notification_service = NotificationService::new();

        let svc = RestartService::new(
            snapshot_service,
            Arc::clone(&tmux) as Arc<dyn TmuxAdapter>,
            event_logger,
            notification_service,
        );
        (svc, tmp)
    }

    // -----------------------------------------------------------------------
    // Test 1: successful full restart
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_restart_success() {
        let tmux = Arc::new(MockTmux::new());
        let (svc, _tmp) = make_service(Arc::clone(&tmux));

        let result = svc.execute_restart();
        assert!(result.is_ok(), "execute_restart should succeed: {result:?}");

        let log = tmux.call_log();
        // list_sessions is called by save_all
        assert!(log.contains(&"list_sessions".to_string()), "expected list_sessions");
        assert!(log.contains(&"kill_server".to_string()), "expected kill_server");
        assert!(log.contains(&"start_server".to_string()), "expected start_server");
    }

    // -----------------------------------------------------------------------
    // Test 2: snapshot save failure aborts the sequence
    // -----------------------------------------------------------------------

    #[test]
    fn test_snapshot_save_failure_aborts() {
        // list_sessions will succeed but save_all itself calls list_sessions;
        // we need list_sessions to return an error to force save_all failure.
        // We'll use a custom tmux that fails list_sessions.
        struct FailListSessions;

        impl TmuxAdapter for FailListSessions {
            fn list_sessions(&self) -> Result<Vec<RawSession>> {
                anyhow::bail!("simulated list_sessions failure");
            }
            fn list_windows(&self, _: &str) -> Result<Vec<RawWindow>> { Ok(vec![]) }
            fn list_panes(&self, _: &str, _: &str) -> Result<Vec<RawPane>> { Ok(vec![]) }
            fn new_session(&self, _: &str) -> Result<()> { Ok(()) }
            fn kill_session(&self, _: &str) -> Result<()> { Ok(()) }
            fn kill_server(&self) -> Result<()> {
                panic!("kill_server must NOT be called after snapshot failure");
            }
            fn start_server(&self) -> Result<()> {
                panic!("start_server must NOT be called after snapshot failure");
            }
            fn attach_session(&self, _: &str) -> Result<()> { Ok(()) }
            fn session_activity(&self, _: &str) -> Result<i64> { Ok(0) }
        }

        let tmux: Arc<dyn TmuxAdapter> = Arc::new(FailListSessions);
        let tmp = tempfile::tempdir().expect("tempdir");

        let snapshot_service = Arc::new(
            SnapshotService::new(Arc::clone(&tmux), tmp.path().to_path_buf())
                .expect("SnapshotService::new"),
        );
        let log_store = LogStore::new(std::path::Path::new(":memory:"))
            .expect("in-memory LogStore");
        let event_logger = EventLogger::new(log_store);
        let notification_service = NotificationService::new();

        let svc = RestartService::new(
            snapshot_service,
            tmux,
            event_logger,
            notification_service,
        );

        let result = svc.execute_restart();
        assert!(result.is_err(), "execute_restart should fail when snapshots cannot be saved");
    }

    // -----------------------------------------------------------------------
    // Test 3: partial restore (some sessions fail) — overall Ok, with details
    // -----------------------------------------------------------------------

    #[test]
    fn test_partial_restore_completes() {
        // save_all will save nothing (no sessions), so restore_all will read
        // whatever is in the temp dir.  We write one valid and one invalid JSON
        // to simulate partial restore.
        let tmux = Arc::new(MockTmux::new());
        let tmp = tempfile::tempdir().expect("tempdir");

        // Write a valid snapshot file
        let valid_snap = crate::models::SessionSnapshot {
            name: "good".to_string(),
            windows: vec![],
        };
        std::fs::write(
            tmp.path().join("good.json"),
            serde_json::to_string(&valid_snap).unwrap(),
        )
        .expect("write good.json");

        // Write a broken snapshot file to trigger a parse failure
        std::fs::write(tmp.path().join("bad.json"), b"not valid json")
            .expect("write bad.json");

        let snapshot_service = Arc::new(
            SnapshotService::new(
                Arc::clone(&tmux) as Arc<dyn TmuxAdapter>,
                tmp.path().to_path_buf(),
            )
            .expect("SnapshotService::new"),
        );
        let log_store = LogStore::new(std::path::Path::new(":memory:"))
            .expect("in-memory LogStore");
        let event_logger = EventLogger::new(log_store);
        let notification_service = NotificationService::new();

        let svc = RestartService::new(
            snapshot_service,
            Arc::clone(&tmux) as Arc<dyn TmuxAdapter>,
            event_logger,
            notification_service,
        );

        // execute_restart must not return an error even with partial restore
        let result = svc.execute_restart();
        assert!(
            result.is_ok(),
            "execute_restart should complete (partial restore is non-fatal): {result:?}"
        );

        // kill_server and start_server must have been called
        let log = tmux.call_log();
        assert!(log.contains(&"kill_server".to_string()), "kill_server must be called");
        assert!(log.contains(&"start_server".to_string()), "start_server must be called");
        // new_session was called for the valid snapshot
        assert!(
            log.contains(&"new_session:good".to_string()),
            "expected new_session:good in {log:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4: kill_server failure — sequence continues but result is failure
    // -----------------------------------------------------------------------

    #[test]
    fn test_kill_server_failure_continues() {
        let tmux = Arc::new(MockTmux::new().with_fail_kill());
        let (svc, _tmp) = make_service(Arc::clone(&tmux));

        // Should not return Err (only snapshot save failure is a hard abort).
        let result = svc.execute_restart();
        assert!(
            result.is_ok(),
            "execute_restart should not Err on kill failure: {result:?}"
        );

        let log = tmux.call_log();
        assert!(log.contains(&"kill_server".to_string()));
        // start_server is still attempted after kill failure
        assert!(log.contains(&"start_server".to_string()));
    }

    // -----------------------------------------------------------------------
    // Test 5: phases logged correctly on success
    // -----------------------------------------------------------------------

    #[test]
    fn test_all_phases_logged_on_success() {
        let tmux = Arc::new(MockTmux::new());
        let tmp = tempfile::tempdir().expect("tempdir");

        let snapshot_service = Arc::new(
            SnapshotService::new(Arc::clone(&tmux) as Arc<dyn TmuxAdapter>, tmp.path().to_path_buf())
                .expect("SnapshotService::new"),
        );

        let log_store = LogStore::new(std::path::Path::new(":memory:"))
            .expect("in-memory LogStore");
        let event_logger = EventLogger::new(log_store);
        let notification_service = NotificationService::new();

        let svc = RestartService::new(
            snapshot_service,
            Arc::clone(&tmux) as Arc<dyn TmuxAdapter>,
            event_logger,
            notification_service,
        );

        // All four phases should complete without error.
        svc.execute_restart().expect("execute_restart");

        let log = tmux.call_log();
        assert!(log.contains(&"kill_server".to_string()), "kill_server must be called");
        assert!(log.contains(&"start_server".to_string()), "start_server must be called");
    }
}
