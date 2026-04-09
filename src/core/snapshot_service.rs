use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::models::{PaneSnapshot, SessionSnapshot, TmuxAdapter, WindowSnapshot};

// ---------------------------------------------------------------------------
// RestoreReport
// ---------------------------------------------------------------------------

/// Summary returned by `restore_all`, collecting successes and failures.
#[derive(Debug)]
pub struct RestoreReport {
    /// Session names that were successfully restored.
    pub restored: Vec<String>,
    /// `(session_name, error_message)` pairs for sessions that failed.
    pub failed: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// SnapshotService
// ---------------------------------------------------------------------------

/// Saves and restores tmux sessions as JSON snapshots.
pub struct SnapshotService {
    tmux: Arc<dyn TmuxAdapter>,
    snapshot_dir: PathBuf,
}

impl SnapshotService {
    /// Create a new `SnapshotService`.
    ///
    /// `snapshot_dir` will be created (including parents) if it does not already exist.
    pub fn new(tmux: Arc<dyn TmuxAdapter>, snapshot_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&snapshot_dir)?;
        Ok(Self { tmux, snapshot_dir })
    }

    /// Snapshot a single session by name.
    ///
    /// Queries tmux for windows and panes, builds a [`SessionSnapshot`], writes
    /// the JSON to `<snapshot_dir>/<session_name>.json`, and returns the snapshot.
    pub fn save_session(&self, session_name: &str) -> Result<SessionSnapshot> {
        let raw_windows = self.tmux.list_windows(session_name)?;

        let mut windows = Vec::with_capacity(raw_windows.len());
        for window in &raw_windows {
            let raw_panes = self
                .tmux
                .list_panes(session_name, &window.index.to_string())?;

            let panes = raw_panes
                .iter()
                .map(|p| PaneSnapshot {
                    working_dir: p.current_dir.clone(),
                    index: p.index,
                })
                .collect();

            windows.push(WindowSnapshot {
                name: window.name.clone(),
                layout: window.layout.clone(),
                panes,
            });
        }

        let snapshot = SessionSnapshot {
            name: session_name.to_string(),
            windows,
        };

        let path = self.snapshot_dir.join(format!("{session_name}.json"));
        let json = serde_json::to_string_pretty(&snapshot)?;
        std::fs::write(&path, json)?;

        Ok(snapshot)
    }

    /// Snapshot every active session.
    ///
    /// Per-session failures are logged and skipped — the remaining sessions are
    /// still processed.  Returns the list of successfully produced snapshots.
    pub fn save_all(&self) -> Result<Vec<SessionSnapshot>> {
        let sessions = self.tmux.list_sessions()?;
        let mut snapshots = Vec::with_capacity(sessions.len());

        for session in &sessions {
            match self.save_session(&session.name) {
                Ok(snap) => snapshots.push(snap),
                Err(e) => {
                    tracing::error!(
                        session = %session.name,
                        error = %e,
                        "Failed to save session snapshot; skipping"
                    );
                }
            }
        }

        Ok(snapshots)
    }

    /// Restore a session from a [`SessionSnapshot`].
    ///
    /// Currently creates the tmux session and sends `cd` keys to the first pane
    /// of the first window so the shell lands in the correct working directory.
    ///
    /// Full window/pane restore requires additional `TmuxAdapter` methods
    /// (`new_window`, `split_window`, `send_keys`) which have not yet been added
    /// to the trait. Those can be wired up here once they exist.
    pub fn restore_session(&self, snapshot: &SessionSnapshot) -> Result<()> {
        self.tmux.new_session(&snapshot.name)?;

        // Navigate the first pane of the first window to its recorded working dir.
        // Additional windows/panes beyond the first are noted but require trait extension.
        if let Some(first_window) = snapshot.windows.first() {
            if let Some(first_pane) = first_window.panes.first() {
                // TODO: call tmux.send_keys(session, "0", "0", &format!("cd {}", first_pane.working_dir))
                // once send_keys is added to TmuxAdapter.
                let _ = &first_pane.working_dir; // referenced to avoid dead-code warning
            }

            // TODO: for windows[1..] call tmux.new_window(session, &window.name)
            // once new_window is added to TmuxAdapter.
            // TODO: for panes[1..] within each window call tmux.split_window(session, window_index)
            // once split_window is added to TmuxAdapter.
        }

        Ok(())
    }

    /// Read all `*.json` files from `snapshot_dir`, deserialise them as
    /// [`SessionSnapshot`], and attempt to restore each one.
    ///
    /// Returns a [`RestoreReport`] that separates successes from failures.
    /// A failure to read or restore a single snapshot never aborts the run.
    pub fn restore_all(&self) -> Result<RestoreReport> {
        let mut report = RestoreReport {
            restored: Vec::new(),
            failed: Vec::new(),
        };

        let entries = std::fs::read_dir(&self.snapshot_dir)?;

        for entry in entries {
            let path = match entry {
                Ok(e) => e.path(),
                Err(e) => {
                    tracing::warn!(error = %e, "Could not read directory entry; skipping");
                    continue;
                }
            };

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let session_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("(unknown)")
                .to_string();

            let result = (|| -> Result<SessionSnapshot> {
                let contents = std::fs::read_to_string(&path)?;
                let snapshot: SessionSnapshot = serde_json::from_str(&contents)?;
                Ok(snapshot)
            })();

            let snapshot = match result {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %e,
                        "Failed to deserialise snapshot; skipping"
                    );
                    report.failed.push((session_name, e.to_string()));
                    continue;
                }
            };

            match self.restore_session(&snapshot) {
                Ok(()) => report.restored.push(snapshot.name),
                Err(e) => {
                    tracing::error!(
                        session = %snapshot.name,
                        error = %e,
                        "Failed to restore session; skipping"
                    );
                    report.failed.push((snapshot.name, e.to_string()));
                }
            }
        }

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{RawPane, RawSession, RawWindow};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // MockTmux
    // -----------------------------------------------------------------------

    struct MockTmux {
        sessions: Vec<RawSession>,
        /// Maps `(session, window_index)` → panes.  Stored flat; tests use a
        /// single window index so the same panes are returned regardless of window.
        panes: Vec<RawPane>,
        windows: Vec<RawWindow>,
        /// Simulates failure for sessions whose name is in this list.
        fail_sessions: Vec<String>,
        calls: Mutex<Vec<String>>,
    }

    impl MockTmux {
        fn new(
            sessions: Vec<RawSession>,
            windows: Vec<RawWindow>,
            panes: Vec<RawPane>,
        ) -> Self {
            Self {
                sessions,
                windows,
                panes,
                fail_sessions: Vec::new(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_fail_sessions(mut self, names: Vec<&str>) -> Self {
            self.fail_sessions = names.iter().map(|s| s.to_string()).collect();
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
            Ok(self.sessions.clone())
        }

        fn list_windows(&self, session: &str) -> Result<Vec<RawWindow>> {
            self.record(format!("list_windows:{session}"));
            if self.fail_sessions.contains(&session.to_string()) {
                anyhow::bail!("simulated list_windows failure for {session}");
            }
            Ok(self.windows.clone())
        }

        fn list_panes(&self, session: &str, window: &str) -> Result<Vec<RawPane>> {
            self.record(format!("list_panes:{session}:{window}"));
            Ok(self.panes.clone())
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
            Ok(())
        }

        fn start_server(&self) -> Result<()> {
            self.record("start_server");
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

    fn raw_session(name: &str) -> RawSession {
        RawSession {
            name: name.to_string(),
            created: 0,
            attached_clients: 0,
            activity: 0,
        }
    }

    fn raw_window(index: u32, name: &str) -> RawWindow {
        RawWindow {
            index,
            name: name.to_string(),
            layout: "main-vertical".to_string(),
        }
    }

    fn raw_pane(index: u32, dir: &str) -> RawPane {
        RawPane {
            index,
            pid: 100 + index,
            current_dir: dir.to_string(),
            current_command: "bash".to_string(),
        }
    }

    fn make_service(mock: Arc<MockTmux>) -> (SnapshotService, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let svc = SnapshotService::new(mock, tmp.path().to_path_buf())
            .expect("SnapshotService::new");
        (svc, tmp)
    }

    // -----------------------------------------------------------------------
    // save_session — creates correct JSON file
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_session_creates_json_file() {
        let mock = Arc::new(MockTmux::new(
            vec![],
            vec![raw_window(0, "editor")],
            vec![raw_pane(0, "/home/user")],
        ));
        let (svc, tmp) = make_service(mock);

        svc.save_session("dev").expect("save_session should succeed");

        let json_path = tmp.path().join("dev.json");
        assert!(
            json_path.exists(),
            "expected JSON file at {json_path:?}"
        );
    }

    // -----------------------------------------------------------------------
    // save_session — snapshot content matches tmux data
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_session_snapshot_content() {
        let mock = Arc::new(MockTmux::new(
            vec![],
            vec![
                raw_window(0, "editor"),
                raw_window(1, "server"),
            ],
            vec![
                raw_pane(0, "/home/user"),
                raw_pane(1, "/tmp"),
            ],
        ));
        let (svc, tmp) = make_service(mock);

        let snap = svc.save_session("myapp").expect("save_session should succeed");

        // Returned snapshot has correct structure
        assert_eq!(snap.name, "myapp");
        assert_eq!(snap.windows.len(), 2);
        assert_eq!(snap.windows[0].name, "editor");
        assert_eq!(snap.windows[1].name, "server");
        assert_eq!(snap.windows[0].panes.len(), 2);
        assert_eq!(snap.windows[0].panes[0].working_dir, "/home/user");
        assert_eq!(snap.windows[0].panes[1].working_dir, "/tmp");

        // JSON file on disk deserialises to the same snapshot
        let raw = std::fs::read_to_string(tmp.path().join("myapp.json"))
            .expect("read file");
        let from_disk: SessionSnapshot =
            serde_json::from_str(&raw).expect("deserialise");
        assert_eq!(from_disk.name, snap.name);
        assert_eq!(from_disk.windows.len(), snap.windows.len());
    }

    // -----------------------------------------------------------------------
    // save_all — saves multiple sessions
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_all_multiple_sessions() {
        let mock = Arc::new(MockTmux::new(
            vec![raw_session("alpha"), raw_session("beta")],
            vec![raw_window(0, "shell")],
            vec![raw_pane(0, "/var")],
        ));
        let (svc, tmp) = make_service(mock);

        let snaps = svc.save_all().expect("save_all should succeed");

        assert_eq!(snaps.len(), 2);
        assert!(tmp.path().join("alpha.json").exists());
        assert!(tmp.path().join("beta.json").exists());
    }

    // -----------------------------------------------------------------------
    // save_all — continues on individual session failure
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_all_continues_on_failure() {
        // "bad" will fail list_windows; "good" should still be saved.
        let mock = Arc::new(MockTmux::new(
            vec![raw_session("bad"), raw_session("good")],
            vec![raw_window(0, "main")],
            vec![raw_pane(0, "/tmp")],
        ).with_fail_sessions(vec!["bad"]));

        let (svc, tmp) = make_service(mock);

        let snaps = svc.save_all().expect("save_all itself should not fail");

        // Only "good" succeeds
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].name, "good");
        assert!(!tmp.path().join("bad.json").exists());
        assert!(tmp.path().join("good.json").exists());
    }

    // -----------------------------------------------------------------------
    // restore_all — reads JSON files and attempts restoration
    // -----------------------------------------------------------------------

    #[test]
    fn test_restore_all_reads_and_restores() {
        let mock = Arc::new(MockTmux::new(
            vec![raw_session("proj")],
            vec![raw_window(0, "work")],
            vec![raw_pane(0, "/src")],
        ));
        let (svc, _tmp) = make_service(mock.clone());

        // First save so there is a JSON on disk
        svc.save_session("proj").expect("save first");

        let report = svc.restore_all().expect("restore_all should succeed");

        assert_eq!(report.restored, vec!["proj"]);
        assert!(report.failed.is_empty());

        // Verify new_session was called
        let log = mock.call_log();
        assert!(
            log.contains(&"new_session:proj".to_string()),
            "expected new_session:proj in {log:?}"
        );
    }

    // -----------------------------------------------------------------------
    // restore_all — reports failures without aborting
    // -----------------------------------------------------------------------

    #[test]
    fn test_restore_all_reports_invalid_json_failure() {
        let mock = Arc::new(MockTmux::new(vec![], vec![], vec![]));
        let (svc, tmp) = make_service(mock);

        // Write a broken JSON file
        std::fs::write(tmp.path().join("broken.json"), b"not valid json")
            .expect("write broken file");

        // Also write a valid one
        let good_snap = SessionSnapshot {
            name: "good".to_string(),
            windows: vec![],
        };
        std::fs::write(
            tmp.path().join("good.json"),
            serde_json::to_string(&good_snap).unwrap(),
        )
        .expect("write good file");

        let report = svc.restore_all().expect("restore_all should not propagate errors");

        assert_eq!(report.restored, vec!["good"]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, "broken");
    }
}
